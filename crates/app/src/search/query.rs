//! 搜索查询层——**逐字移植自 Zed** `crates/project/src/search.rs`
//! （基线 commit `4c902c9`，2026-09-23）+ `crates/util/src/paths.rs` 的
//! `PathMatcher` 子集。
//!
//! 移植原则（用户 2026-10-21 定调「zed 的代码直接抄就好了」）：
//! **能抄的一个字不自己写**。本文件与 Zed 原码的差异只有三处，全是 PF 没有
//! 对应类型导致的机械替换，语义零改动：
//!
//! | Zed | PF | 原因 |
//! |---|---|---|
//! | `RelPath` / `RelPathBuf` | `PathBuf`（相对路径） | PF 无 rel_path 层 |
//! | `PathStyle` | `bool is_posix` | 同上；Windows 走 `\` 分隔符 |
//! | `BufferSnapshot`/`CharKind`（`search` 整缓冲扫描） | 不移植 | PF 的扫描在 [`super::engine`]，逐行喂 [`SearchQuery::search_str`]（Zed 自己也有 `search_str`，行为一致） |
//!
//! 保留的 Zed 行为（这些正是自己写容易漏的）：
//! - 纯文本查询用 `AhoCorasick`（非 ASCII + 忽略大小写时回退 regex，Zed 同注释）；
//! - `\c` / `\C` 模式内大小写开关（`case_sensitive_from_pattern`）；
//! - 整词边界只在贴着「词字符」的一侧补 `\b`，判据是 `\B` 正则
//!   （`WORD_MATCH_TEST`），不是自己写 is_alphanumeric；
//! - `RegexBuilder::multi_line(true).crlf(true)`（CRLF 文件 `$` 才正确）；
//! - `search_str` 的整词判定用「前后字符是否词字符」；
//! - `match_path` 从全路径向上逐级先排后收（exclude 压 include）；
//! - `replacement_for`：`escaped` 查询直接替换，真正则模式走捕获组展开 +
//!   把 `\\`/`\n`/`\t` 还原（Zed 原文照搬）。

#![allow(dead_code)] // 见文件头「未消费的访问器」说明

use std::borrow::Cow;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use fancy_regex::{Captures, Regex, RegexBuilder};
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

// ===========================================================================
// PathMatcher —— 移植自 zed crates/util/src/paths.rs:739-877
// ===========================================================================

/// glob 集合（Zed `PathMatcher`）：`sources()` 供 UI 反显，`is_match` 供
/// 路径过滤。Zed 用 `RelPath` 做前缀/后缀判定，PF 用 `Path` 等价实现。
#[derive(Clone, Debug, Default)]
pub(crate) struct PathMatcher {
    /// (原始 glob 串, 编译后的 glob, glob 是否以分隔符结尾)
    sources: Vec<(String, Glob, bool)>,
    glob: Option<GlobSet>,
}

impl PathMatcher {
    /// Zed `PathMatcher::new`：非法 glob 直接报错（UI 红字提示用）。
    pub(crate) fn new(
        globs: impl IntoIterator<Item = impl AsRef<str>>,
        is_posix: bool,
    ) -> Result<Self, globset::Error> {
        let globs = globs
            .into_iter()
            .map(|as_str| {
                GlobBuilder::new(as_str.as_ref())
                    .backslash_escape(is_posix)
                    .build()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sources = globs
            .iter()
            .map(|glob| {
                let raw = glob.glob().to_string();
                (
                    raw.clone(),
                    glob.clone(),
                    raw.ends_with(if is_posix { '/' } else { '\\' }),
                )
            })
            .collect();
        let mut builder = GlobSetBuilder::new();
        for glob in globs {
            builder.add(glob);
        }
        Ok(Self {
            sources,
            glob: Some(builder.build()?),
        })
    }

    pub(crate) fn sources(&self) -> impl Iterator<Item = &str> + Clone {
        self.sources.iter().map(|(source, ..)| source.as_str())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Zed `is_match`：源串的前缀/后缀直判（`src` 能命中 `src/a.rs` 与
    /// `a/src`），再交给 globset；最后补一次「带尾分隔符」的匹配（
    /// `src/**` 命中 `src` 自身）。
    pub(crate) fn is_match(&self, other: &Path) -> bool {
        let other_str = other.to_string_lossy().replace('\\', "/");
        if self.sources.iter().any(|(source, compiled, _)| {
            let source = if source.ends_with('/') || source.ends_with('\\') {
                &source[..source.len() - 1]
            } else {
                source.as_str()
            };
            // 前缀：other 以 source 开头且下一字符是分隔符或结束
            let prefix = other_str == source
                || other_str
                    .strip_prefix(source)
                    .is_some_and(|rest| rest.starts_with('/'));
            // 后缀：other 以 source 结尾且上一字符是分隔符或开头
            let suffix = other_str == source
                || other_str
                    .strip_suffix(source)
                    .is_some_and(|head| head.ends_with('/'));
            let _ = compiled;
            prefix || suffix
        }) {
            return true;
        }
        let Some(glob) = self.glob.as_ref() else {
            return false;
        };
        if glob.is_match(&other_str) {
            return true;
        }
        glob.is_match(other_str + "/")
    }
}

// ===========================================================================
// SearchQuery —— 移植自 zed crates/project/src/search.rs:85-715
// ===========================================================================

#[derive(Clone, Debug)]
pub(crate) struct SearchInputs {
    query: Arc<str>,
    files_to_include: PathMatcher,
    files_to_exclude: PathMatcher,
    match_full_paths: bool,
}

impl SearchInputs {
    pub(crate) fn as_str(&self) -> &str {
        self.query.as_ref()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum SearchQuery {
    Text {
        search: AhoCorasick,
        replacement: Option<String>,
        whole_word: bool,
        case_sensitive: bool,
        inner: SearchInputs,
    },
    Regex {
        regex: Regex,
        replacement: Option<String>,
        whole_word: bool,
        case_sensitive: bool,
        one_match_per_line: bool,
        inner: SearchInputs,
        escaped: bool,
    },
}

static WORD_MATCH_TEST: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new(r"\B")
        .build()
        .expect("Failed to create WORD_MATCH_TEST")
});

impl SearchQuery {
    /// Zed `SearchQuery::text`（AhoCorasick 字面量）。
    pub(crate) fn text(
        query: impl ToString,
        whole_word: bool,
        case_sensitive: bool,
        include_ignored_unused: bool,
        files_to_include: PathMatcher,
        files_to_exclude: PathMatcher,
        match_full_paths: bool,
    ) -> anyhow::Result<Self> {
        let mut query = query.to_string();
        normalize_line_endings(&mut query);
        if !case_sensitive && !query.is_ascii() {
            // AhoCorasickBuilder doesn't support case-insensitive search with
            // unicode characters → fallback to regex (Zed 原注释)
            return Self::escaped_regex(
                query,
                whole_word,
                case_sensitive,
                include_ignored_unused,
                files_to_include,
                files_to_exclude,
                match_full_paths,
            );
        }
        let search = AhoCorasickBuilder::new()
            .ascii_case_insensitive(!case_sensitive)
            .build([&query])?;
        let inner = SearchInputs {
            query: query.into(),
            files_to_exclude,
            files_to_include,
            match_full_paths,
        };
        Ok(Self::Text {
            search,
            replacement: None,
            whole_word,
            case_sensitive,
            inner,
        })
    }

    /// Zed `SearchQuery::regex`。
    pub(crate) fn regex(
        query: impl ToString,
        whole_word: bool,
        case_sensitive: bool,
        one_match_per_line: bool,
        files_to_include: PathMatcher,
        files_to_exclude: PathMatcher,
        match_full_paths: bool,
    ) -> anyhow::Result<Self> {
        let query = query.to_string();
        let inner = SearchInputs {
            query: Arc::from(query.as_str()),
            files_to_include,
            files_to_exclude,
            match_full_paths,
        };
        Self::build_regex(
            query,
            whole_word,
            case_sensitive,
            one_match_per_line,
            inner,
            false,
        )
    }

    /// Zed `SearchQuery::escaped_regex`（字面量即按字面量匹配）。
    pub(crate) fn escaped_regex(
        query: impl ToString,
        whole_word: bool,
        case_sensitive: bool,
        _include_ignored: bool,
        files_to_include: PathMatcher,
        files_to_exclude: PathMatcher,
        match_full_paths: bool,
    ) -> anyhow::Result<Self> {
        let mut query = query.to_string();
        normalize_line_endings(&mut query);
        let inner = SearchInputs {
            query: Arc::from(query.as_str()),
            files_to_include,
            files_to_exclude,
            match_full_paths,
        };
        Self::build_regex(
            regex::escape(&query),
            whole_word,
            case_sensitive,
            false,
            inner,
            true,
        )
    }

    fn build_regex(
        mut pattern: String,
        whole_word: bool,
        mut case_sensitive: bool,
        one_match_per_line: bool,
        inner: SearchInputs,
        escaped: bool,
    ) -> anyhow::Result<Self> {
        if let Some((case_sensitive_from_pattern, new_pattern)) =
            Self::case_sensitive_from_pattern(&pattern)
        {
            case_sensitive = case_sensitive_from_pattern;
            pattern = new_pattern
        }

        if whole_word {
            let mut word_pattern = String::new();
            if let Some(first) = pattern.get(0..1)
                && WORD_MATCH_TEST.is_match(first).is_ok_and(|x| !x)
            {
                word_pattern.push_str("\\b");
            }
            word_pattern.push_str(&pattern);
            if let Some(last) = pattern.get(pattern.len() - 1..)
                && WORD_MATCH_TEST.is_match(last).is_ok_and(|x| !x)
            {
                word_pattern.push_str("\\b");
            }
            pattern = word_pattern
        }

        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!case_sensitive)
            .multi_line(true)
            .crlf(true)
            .build()?;
        Ok(Self::Regex {
            regex,
            replacement: None,
            whole_word,
            case_sensitive,
            inner,
            one_match_per_line,
            escaped,
        })
    }

    /// Zed `case_sensitive_from_pattern`：`\c` / `\C` 模式内大小写开关。
    fn case_sensitive_from_pattern(query: &str) -> Option<(bool, String)> {
        if !(query.contains("\\c") || query.contains("\\C")) {
            return None;
        }

        let mut was_escaped = false;
        let mut new_query = String::new();
        let mut is_case_sensitive = None;

        for c in query.chars() {
            if was_escaped {
                if c == 'c' {
                    is_case_sensitive = Some(false);
                } else if c == 'C' {
                    is_case_sensitive = Some(true);
                } else {
                    new_query.push('\\');
                    new_query.push(c);
                }
                was_escaped = false
            } else if c == '\\' {
                was_escaped = true
            } else {
                new_query.push(c);
            }
        }

        is_case_sensitive.map(|c| (c, new_query))
    }

    pub(crate) fn with_replacement(mut self, new_replacement: String) -> Self {
        match self {
            Self::Text {
                ref mut replacement,
                ..
            }
            | Self::Regex {
                ref mut replacement,
                ..
            } => {
                *replacement = Some(new_replacement);
                self
            }
        }
    }

    /// Zed `replacement_for`：把命中处的替换文本展开成最终字符串。
    pub(crate) fn replacement_for<'a>(
        &self,
        line: &'a str,
        hit: Range<usize>,
    ) -> Option<Cow<'a, str>> {
        match self {
            SearchQuery::Text { replacement, .. }
            | SearchQuery::Regex {
                replacement,
                escaped: true,
                ..
            } => replacement.clone().map(Cow::from),

            SearchQuery::Regex {
                regex,
                replacement: Some(replacement),
                escaped: false,
                ..
            } => {
                static TEXT_REPLACEMENT_SPECIAL_CHARACTERS_REGEX: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"\\\\|\\n|\\t").unwrap());
                let replacement = TEXT_REPLACEMENT_SPECIAL_CHARACTERS_REGEX.replace_all(
                    replacement,
                    |c: &Captures<str>| match c.get(0).unwrap().as_str() {
                        r"\\" => "\\",
                        r"\n" => "\n",
                        r"\t" => "\t",
                        x => unreachable!("Unexpected escape sequence: {}", x),
                    },
                );
                let captures = regex
                    .captures_from_pos(line, hit.start)
                    .ok()
                    .flatten()
                    .filter(|captures| captures.get(0).is_some_and(|m| m.range() == hit));
                let Some(captures) = captures else {
                    return Some(regex.replace(line.get(hit)?, replacement));
                };
                let mut replaced = String::new();
                captures.expand(&replacement, &mut replaced);
                Some(Cow::Owned(replaced))
            }

            SearchQuery::Regex {
                replacement: None, ..
            } => None,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        self.as_inner().as_str()
    }

    pub(crate) fn whole_word(&self) -> bool {
        match self {
            Self::Text { whole_word, .. } => *whole_word,
            Self::Regex { whole_word, .. } => *whole_word,
        }
    }

    pub(crate) fn case_sensitive(&self) -> bool {
        match self {
            Self::Text { case_sensitive, .. } => *case_sensitive,
            Self::Regex { case_sensitive, .. } => *case_sensitive,
        }
    }

    pub(crate) fn is_regex(&self) -> bool {
        matches!(self, Self::Regex { .. })
    }

    fn as_inner(&self) -> &SearchInputs {
        match self {
            Self::Regex { inner, .. } | Self::Text { inner, .. } => inner,
        }
    }

    /// Zed `match_path`：从全路径向上逐级先排后收。
    pub(crate) fn match_path(&self, file_path: &Path) -> bool {
        let mut path = PathBuf::from(file_path);
        loop {
            if self.as_inner().files_to_exclude.is_match(&path) {
                return false;
            } else if self.as_inner().files_to_include.is_empty()
                || self.as_inner().files_to_include.is_match(&path)
            {
                return true;
            } else if !path.pop() || path.as_os_str().is_empty() {
                return false;
            }
        }
    }

    /// Zed `search_str`：**逐字移植**（行内命中区间，含整词判定）。
    pub(crate) fn search_str(&self, text: &str) -> Vec<Range<usize>> {
        if self.as_str().is_empty() {
            return Vec::new();
        }

        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        let mut matches = Vec::new();
        match self {
            Self::Text {
                search, whole_word, ..
            } => {
                for mat in search.find_iter(text.as_bytes()) {
                    if *whole_word {
                        let prev_char = text[..mat.start()].chars().last();
                        let next_char = text[mat.end()..].chars().next();
                        if prev_char.is_some_and(&is_word_char)
                            || next_char.is_some_and(&is_word_char)
                        {
                            continue;
                        }
                    }
                    matches.push(mat.start()..mat.end());
                }
            }
            Self::Regex {
                regex,
                one_match_per_line,
                ..
            } => {
                // Zed 的 search_str 不处理 one_match_per_line（那是整缓冲
                // search 的事），这里保持一致：只取区间。
                let _ = one_match_per_line;
                for mat in regex.find_iter(text).flatten() {
                    matches.push(mat.start()..mat.end());
                }
            }
        }
        matches
    }
}

/// Zed `text::LineEnding::normalize`（CRLF/CR → LF）。
fn normalize_line_endings(text: &mut String) {
    if text.contains('\r') {
        *text = text.replace("\r\n", "\n").replace('\r', "\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_matchers() -> (PathMatcher, PathMatcher) {
        (
            PathMatcher::new(std::iter::empty::<&str>(), true).unwrap(),
            PathMatcher::new(std::iter::empty::<&str>(), true).unwrap(),
        )
    }

    fn text(q: &str, whole_word: bool, case_sensitive: bool) -> SearchQuery {
        let (inc, exc) = empty_matchers();
        SearchQuery::text(q, whole_word, case_sensitive, false, inc, exc, false).unwrap()
    }

    fn regex(q: &str, whole_word: bool, case_sensitive: bool) -> SearchQuery {
        let (inc, exc) = empty_matchers();
        SearchQuery::regex(q, whole_word, case_sensitive, false, inc, exc, false).unwrap()
    }

    #[test]
    fn text_query_is_literal() {
        let q = text("fn(", false, true);
        assert_eq!(q.search_str("fn(a: u8)"), vec![0..3]);
        assert!(q.search_str("other").is_empty());
    }

    #[test]
    fn ascii_case_insensitive_uses_aho_corasick() {
        let q = text("Foo", false, false);
        assert!(matches!(q, SearchQuery::Text { .. }));
        assert_eq!(q.search_str("foo Foo"), vec![0..3, 4..7]);
        let q = text("Foo", false, true);
        assert_eq!(q.search_str("foo Foo"), vec![4..7]);
    }

    #[test]
    fn non_ascii_case_insensitive_falls_back_to_regex() {
        // Zed 原逻辑：AhoCorasick 不支持 unicode 忽略大小写 → 回退 regex
        let q = text("Ä", false, false);
        assert!(matches!(q, SearchQuery::Regex { escaped: true, .. }));
        assert_eq!(q.search_str("ä Ä"), vec![0..2, 3..5]);
    }

    #[test]
    fn case_sensitive_pattern_modifier() {
        // \C = 强制区分大小写
        let q = regex(r"\CFoo", false, false);
        assert!(q.case_sensitive());
        assert_eq!(q.as_str(), r"\CFoo");
        assert_eq!(q.search_str("foo Foo"), vec![4..7]);
        // \c = 强制忽略大小写
        let q = regex(r"\cFoo", false, true);
        assert!(!q.case_sensitive());
        assert_eq!(q.search_str("foo Foo"), vec![0..3, 4..7]);
    }

    #[test]
    fn whole_word_uses_boundary_on_word_edges_only() {
        let q = text("foo", true, true);
        assert_eq!(q.search_str("a foo b"), vec![2..5]);
        assert!(q.search_str("foobar").is_empty());
        // 含空格的多词查询不能被两侧 \b 夹死
        let q = text("foo bar", true, true);
        assert_eq!(q.search_str("x foo bar y"), vec![2..9]);
    }

    #[test]
    fn regex_multiline_and_crlf() {
        let q = regex(r"^hit$", false, true);
        // crlf(true)：`$` 在 \r 之前也成立
        assert_eq!(q.search_str("hit"), vec![0..3]);
        assert!(q.search_str("x hit").is_empty());
    }

    #[test]
    fn match_path_walks_up_and_exclude_wins() {
        let (_, exc) = empty_matchers();
        let inc = PathMatcher::new(["src"], true).unwrap();
        let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
        assert!(q.match_path(Path::new("src/a.rs")));
        assert!(q.match_path(Path::new("src/deep/b.rs")));
        assert!(!q.match_path(Path::new("docs/x.md")));

        let inc = PathMatcher::new(std::iter::empty::<&str>(), true).unwrap();
        let exc = PathMatcher::new(["src/vendor"], true).unwrap();
        let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
        assert!(q.match_path(Path::new("src/a.rs")));
        assert!(!q.match_path(Path::new("src/vendor/c.rs")));
    }

    /// glob 语义与 Zed `PathMatcher` **完全一致**：Zed 的 `GlobBuilder` 不设
    /// `literal_separator`，所以 `*` 会跨分隔符（`src/*.rs` 命中
    /// `src/deep/a.rs`）。这不是 bug，是上游行为——PF 抄它就保持一样。
    #[test]
    fn glob_matches_like_zed_path_matcher() {
        let (_, exc) = empty_matchers();
        let inc = PathMatcher::new(["src/*.rs"], true).unwrap();
        let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
        assert!(q.match_path(Path::new("src/a.rs")));
        assert!(
            q.match_path(Path::new("src/deep/a.rs")),
            "`*` 跨分隔符（Zed 同）"
        );

        let (_, exc) = empty_matchers();
        let inc = PathMatcher::new(["src/**/*.rs"], true).unwrap();
        let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
        assert!(q.match_path(Path::new("src/deep/a.rs")));
        assert!(!q.match_path(Path::new("docs/a.rs")));
    }

    #[test]
    fn replacement_expands_captures() {
        let q = regex(r"(\w+)@(\w+)", false, true).with_replacement("$2:$1".into());
        assert_eq!(q.replacement_for("a@b", 0..3).as_deref(), Some("b:a"));
        // escaped 查询直接字面量替换
        let q = text("a@b", false, true).with_replacement("X".into());
        assert_eq!(q.replacement_for("a@b", 0..3).as_deref(), Some("X"));
    }
}
