//! 项目搜索执行层——Zed `crates/project/src/project_search.rs` 的语义对齐版，
//! **无任何内容索引**（Zed 也没有：全仓无 tantivy/trigram/ripgrep，见
//! `docs/模块设计/083-项目搜索.md` §1 的源码取证）。
//!
//! Zed 的流水线是两阶段：路径候选（worktree 内存树）→ include/exclude glob
//! 过滤 → **磁盘粗筛只找一个首命中**（`handle_find_first_match`）→ 命中的才
//! 开成 Buffer 做全量 range 精算。两阶段之所以存在，是因为它的「精算」必须
//! 经过 Buffer/MultiBuffer 快照（语法树、锚点、折叠）；PF 没有这层包袱，
//! 一次读盘就能把「命中行 + 上下文行」一起收齐，故合并为单阶段。**保持的
//! 语义**：
//!
//! 1. 候选路径尊重 .gitignore（`include_ignored` 开关打开时全量走 ignore
//!    crate 的 WalkBuilder，同 Zed 的 Scan Ignored）；
//! 2. 路径过滤前置于内容读取（include/exclude 不匹配的文件根本不开盘）；
//! 3. 二进制早退（头 1KB 判 `ByteContent::Binary`，Zed 同判据）；
//! 4. 结果按路径序流式回传（Zed `maintain_sorted_search_results` 的排序
//!    契约：UI 拿到的批次永远单调，增量追加不会回跳）；
//! 5. 上限 5000 文件 / 10000 命中行（Zed `MAX_SEARCH_RESULT_FILES`/
//!    `MAX_SEARCH_RESULT_RANGES` 同值），触顶发 `Limit`。
//!
//! 与 Zed 的差异只有一处：单线程串行扫（Zed 按 CPU 分段并行）。PF 的候选
//! 清单通常来自 `git ls-files`，十万级文件也在几十毫秒量级；并行留作后续
//! 优化（顺序性一旦并行就要引入 Zed 那套乱序批次缓冲，先不做）。

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc::UnboundedSender;

use crate::editor::file_content::{
    ByteContent, FILE_ANALYSIS_BYTES, analyze_byte_content, decode_text,
};

/// 上下文行数（Zed `excerpt_context_lines` 默认 2）
pub(crate) const CONTEXT_LINES: u32 = 2;
/// 命中文件数上限（Zed `Search::MAX_SEARCH_RESULT_FILES`）
pub(crate) const MAX_FILES: usize = 5_000;
/// 命中行数上限（Zed `Search::MAX_SEARCH_RESULT_RANGES`）
pub(crate) const MAX_MATCH_LINES: usize = 10_000;
/// 文件名命中上限（Zed 的文件查找器是独立面板，PF 合并进同一个结果列表，
/// 因此单独封顶，防止 `src` 这类词把内容结果挤到看不见）
pub(crate) const MAX_PATH_HITS: usize = 50;
/// 单文件扫描上限：Zed 不设（它的 buffer 本来就能开大文件），PF 的文件
/// 编辑器闸在 10MB，这里取 8MB 与之同量级，避免一个日志文件卡死搜索
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// 单文件命中行上限（防单文件刷满全局额度）
const MAX_LINES_PER_FILE: usize = 500;

/// 搜索开关位（对应 Zed `SearchOptions` 的四个本项目关心的位）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchOpts {
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
    pub(crate) regex: bool,
    pub(crate) include_ignored: bool,
}

/// 编译后的查询（Zed `SearchQuery::Regex` 的等价物：纯文本也编成正则）。
pub(crate) struct CompiledQuery {
    re: regex::Regex,
}

impl CompiledQuery {
    pub(crate) fn new(query: &str, o: SearchOpts) -> Result<Self, String> {
        if query.is_empty() {
            return Err(String::new());
        }
        let mut pat = if o.regex {
            query.to_string()
        } else {
            // Zed `escaped_regex`：非正则模式按字面量转义
            regex::escape(query)
        };
        if o.whole_word {
            pat = wrap_word_boundary(&pat, query);
        }
        let mut b = regex::RegexBuilder::new(&pat);
        // multi_line：`^`/`$` 按行生效（Zed build_regex 同开）
        b.case_insensitive(!o.case_sensitive).multi_line(true);
        match b.build() {
            Ok(re) => Ok(Self { re }),
            Err(e) => Err(e.to_string()),
        }
    }

    /// 一行里的全部命中区间（空宽命中丢弃）。
    pub(crate) fn spans(&self, line: &str) -> Vec<Range<usize>> {
        self.re
            .find_iter(line)
            .filter(|m| m.start() < m.end())
            .map(|m| m.start()..m.end())
            .collect()
    }

    /// 字符串是否命中（文件名/路径匹配用同一套模式，同 Zed 的
    /// `match_full_paths` 语义）。
    pub(crate) fn hits(&self, s: &str) -> bool {
        self.re.is_match(s)
    }

    /// 整段文本的命中处数（替换前统计「将要改掉多少次」）。
    pub(crate) fn count_matches(&self, text: &str) -> usize {
        self.re
            .find_iter(text)
            .filter(|m| m.start() < m.end())
            .count()
    }

    /// 整段文本的第一处命中区间（`ReplaceNext` 只改这一处）。
    pub(crate) fn find_first(&self, text: &str) -> Option<Range<usize>> {
        self.re
            .find_iter(text)
            .find(|m| m.start() < m.end())
            .map(|m| m.start()..m.end())
    }

    /// 全部替换（`ReplaceAll`）。`$1` 之类捕获组引用按 regex crate 语义生效
    /// （Zed 的替换框同用 `$n`）。
    pub(crate) fn replace_all(&self, text: &str, replacement: &str) -> String {
        self.re.replace_all(text, replacement).into_owned()
    }
}

/// 整词边界只在查询词贴着「词字符」的一侧补 `\b`（Zed build_regex 同款：
/// 无脑两侧都补会让 `foo bar` 这类含空格的查询永远匹配不上）。
fn wrap_word_boundary(pat: &str, src: &str) -> String {
    let wordish = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(pat.len() + 4);
    if wordish(src.chars().next()) {
        out.push_str(r"\b");
    }
    out.push_str(pat);
    if wordish(src.chars().next_back()) {
        out.push_str(r"\b");
    }
    out
}

/// include/exclude glob 集合（Zed `PathMatcher` 的等价物）。
pub(crate) struct PathFilters {
    include: globset::GlobSet,
    exclude: globset::GlobSet,
}

impl PathFilters {
    pub(crate) fn new(include: &[String], exclude: &[String]) -> Result<Self, String> {
        Ok(Self {
            include: build_globset(include)?,
            exclude: build_globset(exclude)?,
        })
    }

    /// 路径是否入选。逐字对照 Zed `SearchQuery::match_path`：从全路径向上
    /// 逐级先查 exclude 再查 include，任一级 exclude 命中即淘汰；include
    /// 命中即入选；爬到空路径仍无结论 = 淘汰。
    pub(crate) fn selected(&self, rel: &str) -> bool {
        let mut p = PathBuf::from(rel);
        loop {
            if self.exclude.is_match(&p) {
                return false;
            }
            if self.include.is_empty() || self.include.is_match(&p) {
                return true;
            }
            if p.as_os_str().is_empty() || !p.pop() {
                return false;
            }
        }
    }
}

fn build_globset(globs: &[String]) -> Result<globset::GlobSet, String> {
    let mut b = globset::GlobSetBuilder::new();
    for g in globs {
        let g = g.trim();
        if g.is_empty() {
            continue;
        }
        // literal_separator：`*` 不跨目录（gitignore/wax 语义，Zed 同）
        let glob = globset::GlobBuilder::new(g)
            .literal_separator(true)
            .case_insensitive(cfg!(windows))
            .build()
            .map_err(|e| format!("{g}: {e}"))?;
        b.add(glob);
    }
    b.build().map_err(|e| e.to_string())
}

/// 一个文件里的一处命中行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineHit {
    /// 1-based 行号
    pub(crate) line: u32,
    /// 行内命中区间（字节）
    pub(crate) spans: Vec<Range<usize>>,
}

/// 命中文件的完整结果：命中行 + 已按上下文合并好的摘录块。
#[derive(Clone, Debug)]
pub(crate) struct FileHit {
    /// 相对 cwd 的 `/` 分隔路径
    pub(crate) rel: String,
    pub(crate) hits: Vec<LineHit>,
    /// 摘录块（每块 = 若干连续行 `(1-based 行号, 行文本)`）。相邻命中行
    /// 的上下文窗口重叠时并成一块，避免同一段代码在结果里重复出现。
    pub(crate) blocks: Vec<Vec<(u32, String)>>,
}

/// 后台 → 前台的流式消息。
pub(crate) enum SearchMsg {
    /// 文件名命中（一次性发，供结果区顶部「文件名匹配」组使用）
    PathHits(Vec<String>),
    Hit(FileHit),
    /// 触到文件/行数上限
    Limit,
    /// 结束；scanned = 实际读盘过的文件数
    Done {
        scanned: usize,
    },
}

/// 候选路径（相对 cwd、`/` 分隔、已排序）。
///
/// 默认走 `git ls-files --cached --others --exclude-standard`（与 @ 检索、
/// 文件搜索弹窗同一事实源，天然尊重 .gitignore），非 git 目录回退 BFS；
/// `include_ignored` 时改走 ignore crate 的全量遍历。
pub(crate) fn candidate_paths(cwd: &Path, include_ignored: bool) -> Vec<String> {
    if !include_ignored {
        let mut files = crate::services::file_index::load_listing(cwd).files;
        files.sort();
        files.dedup();
        return files;
    }
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(cwd)
        // 四个 ignore 源全关 = 连被忽略目录也进候选集（Zed Scan Ignored 同义）
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        // .git 目录自身永远不扫（对象库是二进制垃圾场）
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(cwd) else {
            continue;
        };
        out.push(rel.to_string_lossy().replace('\\', "/"));
    }
    out.sort();
    out
}

/// 单文件扫描：跳过二进制/超限，返回命中行与摘录块。None = 无命中或不可读。
pub(crate) fn scan_text(rel: String, text: &str, q: &CompiledQuery) -> Option<FileHit> {
    let mut hits: Vec<LineHit> = Vec::new();
    for (ix, raw) in text.lines().enumerate() {
        // CRLF：regex 的 multi_line 不给 `$` 做 CRLF 容忍（fancy_regex 开
        // 了 crlf(true)，regex 没这个开关），手动剥尾 CR 等价
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let spans = q.spans(line);
        if spans.is_empty() {
            continue;
        }
        hits.push(LineHit {
            line: ix as u32 + 1,
            spans,
        });
        if hits.len() >= MAX_LINES_PER_FILE {
            break;
        }
    }
    if hits.is_empty() {
        return None;
    }
    let hit_lines: Vec<u32> = hits.iter().map(|h| h.line).collect();
    let blocks = build_blocks(text, &hit_lines, CONTEXT_LINES);
    Some(FileHit { rel, hits, blocks })
}

/// 把命中行按 ±ctx 上下文窗口合并成摘录块（重叠即并块）。
fn build_blocks(text: &str, hit_lines: &[u32], ctx: u32) -> Vec<Vec<(u32, String)>> {
    let total = text.lines().count() as u32;
    if total == 0 || hit_lines.is_empty() {
        return Vec::new();
    }
    // 块的闭区间：相邻命中行间隔 <= 2*ctx+1 就并块（两块会重叠或紧邻）
    let merge_gap = 2 * ctx + 1;
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for &line in hit_lines {
        match ranges.last_mut() {
            Some(last) if line <= last.1 + merge_gap => last.1 = line,
            _ => ranges.push((line, line)),
        }
    }
    // 一次性取行文本（只物化需要的块，不整文件克隆）
    let lines: Vec<&str> = text.lines().collect();
    ranges
        .into_iter()
        .map(|(first, last)| {
            let start = first.saturating_sub(ctx).max(1);
            let end = (last + ctx).min(total);
            (start..=end)
                .map(|n| {
                    let raw = lines.get(n as usize - 1).copied().unwrap_or("");
                    (n, raw.strip_suffix('\r').unwrap_or(raw).to_string())
                })
                .collect()
        })
        .collect()
}

/// 读盘 + 扫描一个文件。文件不存在/不可读/二进制/超限一律 None（Zed 对
/// 读失败也是 `log_err` 后跳过，搜索不因单个文件失败而中断）。
fn scan_file(cwd: &Path, rel: &str, q: &CompiledQuery) -> Option<FileHit> {
    let abs = cwd.join(rel);
    let meta = std::fs::metadata(&abs).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(&abs).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let binary = {
        let head = &bytes[..bytes.len().min(FILE_ANALYSIS_BYTES)];
        matches!(analyze_byte_content(head), ByteContent::Binary)
    };
    if binary {
        return None;
    }
    // 纯 UTF-8 且无 ESC 控制序列 → 零拷贝直接用；否则走 BOM/UTF-16/chardetng
    // 解码（与文件编辑器同一条口径，GBK 遗留文件也能搜到）
    let owned;
    let text: &str = match std::str::from_utf8(&bytes) {
        Ok(s) if !s.contains('\x1b') => s,
        _ => {
            owned = decode_text(bytes).ok()?.text;
            owned.as_str()
        }
    };
    scan_text(rel.to_string(), text, q)
}

/// 搜索主循环（阻塞，跑在后台执行器上）。`cancel` 置位即尽快返回。
pub(crate) fn run_search(
    cwd: PathBuf,
    q: CompiledQuery,
    filters: PathFilters,
    opts: SearchOpts,
    cancel: Arc<AtomicBool>,
    tx: UnboundedSender<SearchMsg>,
) {
    let paths = candidate_paths(&cwd, opts.include_ignored);

    // 文件名命中：与内容命中同一套模式，直接测相对路径（Zed 的文件查找器
    // 走 fuzzy 打分，PF 合并进同一面板后改用「查询式命中」，语义更好解释：
    // 输入 `main.rs` 或 `src/.*\.rs` 都能把文件本身捞出来）
    let path_hits: Vec<String> = paths
        .iter()
        .filter(|p| q.hits(p))
        .take(MAX_PATH_HITS)
        .cloned()
        .collect();
    if !path_hits.is_empty() && tx.unbounded_send(SearchMsg::PathHits(path_hits)).is_err() {
        return;
    }

    let mut scanned = 0usize;
    let mut files = 0usize;
    let mut lines = 0usize;
    let mut hit_limit = false;
    for rel in &paths {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        // 路径过滤前置于读盘（Zed handle_scan_path 的第一道闸）
        if !filters.selected(rel) {
            continue;
        }
        scanned += 1;
        let Some(hit) = scan_file(&cwd, rel, &q) else {
            continue;
        };
        files += 1;
        lines += hit.hits.len();
        if tx.unbounded_send(SearchMsg::Hit(hit)).is_err() {
            return;
        }
        if files > MAX_FILES || lines > MAX_MATCH_LINES {
            hit_limit = true;
            break;
        }
    }
    if hit_limit {
        let _ = tx.unbounded_send(SearchMsg::Limit);
    }
    let _ = tx.unbounded_send(SearchMsg::Done { scanned });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(src: &str, o: SearchOpts) -> CompiledQuery {
        CompiledQuery::new(src, o).expect("query compiles")
    }

    #[test]
    fn literal_query_is_escaped() {
        let c = q("fn(", SearchOpts::default());
        assert_eq!(c.spans("fn(a: u8)"), vec![0..3]);
        assert!(c.spans("other").is_empty());
    }

    #[test]
    fn case_insensitive_by_default_sensitive_on_request() {
        let c = q("Foo", SearchOpts::default());
        assert_eq!(c.spans("foo Foo"), vec![0..3, 4..7]);
        let c = q(
            "Foo",
            SearchOpts {
                case_sensitive: true,
                ..Default::default()
            },
        );
        assert_eq!(c.spans("foo Foo"), vec![4..7]);
    }

    #[test]
    fn whole_word_needs_word_edges() {
        let o = SearchOpts {
            whole_word: true,
            ..Default::default()
        };
        let c = q("foo", o);
        assert_eq!(c.spans("a foo b"), vec![2..5]);
        assert!(c.spans("foobar").is_empty());
        // 含空格的多词查询不能被两侧 \b 夹死
        let c = q("foo bar", o);
        assert_eq!(c.spans("x foo bar y"), vec![2..9]);
    }

    #[test]
    fn regex_mode_and_bad_pattern() {
        let o = SearchOpts {
            regex: true,
            ..Default::default()
        };
        let c = q(r"\d+", o);
        assert_eq!(c.spans("ab 12 cd"), vec![3..5]);
        assert!(CompiledQuery::new("(", o).is_err());
    }

    #[test]
    fn crlf_and_line_numbers() {
        let c = q("hit", SearchOpts::default());
        let hit = scan_text(
            "a.txt".into(),
            "nope\r\nhit here\r\nnope\r\nhit again\r\n",
            &c,
        )
        .expect("two hits");
        assert_eq!(
            hit.hits.iter().map(|h| h.line).collect::<Vec<_>>(),
            vec![2, 4]
        );
        // 块内文本已剥 CR
        let all: Vec<&str> = hit
            .blocks
            .iter()
            .flatten()
            .map(|(_, t)| t.as_str())
            .collect();
        assert!(all.iter().all(|t| !t.contains('\r')));
        assert!(all.contains(&"hit here"));
    }

    #[test]
    fn context_blocks_merge_when_overlapping() {
        let c = q("needle", SearchOpts::default());
        // 40 行正文，命中只在 1,2,3 与 30：前三个命中 ±2 行并成一块，
        // 行 30 的窗口与它不相接 → 独立成块
        let mut lines: Vec<String> = (1..=40).map(|n| format!("line {n}")).collect();
        for n in [1usize, 2, 3, 30] {
            lines[n - 1].push_str(" needle");
        }
        let text = format!("{}\n", lines.join("\n"));
        let hit = scan_text("a.txt".into(), &text, &c).expect("hits");
        assert_eq!(hit.hits.len(), 4);
        assert_eq!(hit.blocks.len(), 2, "两个不相邻的摘录块");
        // 第一个块：行 1..=5（连续命中 1,2,3 ±2）
        assert_eq!(hit.blocks[0].first().map(|(n, _)| *n), Some(1));
        assert_eq!(hit.blocks[0].last().map(|(n, _)| *n), Some(5));
        // 第二个块：行 28..=32（命中 30 ±2）
        assert_eq!(hit.blocks[1].first().map(|(n, _)| *n), Some(28));
        assert_eq!(hit.blocks[1].last().map(|(n, _)| *n), Some(32));
    }

    /// 相邻命中（间隔 <= 2*ctx+1）的上下文窗口会重叠 → 必须并成一块，
    /// 否则同一段代码在结果里重复出现（Zed 按命中分 excerpt，PF 合并）。
    #[test]
    fn near_hits_share_one_block() {
        let c = q("x", SearchOpts::default());
        let text = "x\nx\nx\n\n\n\n\nx\n"; // 行 1,2,3,8
        let hit = scan_text("a.txt".into(), text, &c).expect("hits");
        assert_eq!(hit.blocks.len(), 1);
        assert_eq!(hit.blocks[0].first().map(|(n, _)| *n), Some(1));
        assert_eq!(hit.blocks[0].last().map(|(n, _)| *n), Some(8));
    }

    #[test]
    fn filters_follow_zed_upward_walk() {
        let f = PathFilters::new(&["src".into()], &["src/vendor".into()]).expect("filters");
        assert!(f.selected("src/a.rs"));
        assert!(f.selected("src/deep/b.rs"));
        // exclude 在祖先级命中即淘汰子孙
        assert!(!f.selected("src/vendor/c.rs"));
        // 不含 src 的路径：include 无人在祖先链上位命中 → 淘汰
        assert!(!f.selected("docs/x.md"));

        // include 为空 = 全收（除 exclude）
        let f = PathFilters::new(&[], &["*.lock".into()]).expect("filters");
        assert!(f.selected("docs/x.md"));
        assert!(!f.selected("Cargo.lock"));
    }

    #[test]
    fn glob_star_does_not_cross_separator() {
        let f = PathFilters::new(&["src/*.rs".into()], &[]).expect("filters");
        assert!(f.selected("src/a.rs"));
        assert!(!f.selected("src/deep/a.rs"));
        let f = PathFilters::new(&["src/**/*.rs".into()], &[]).expect("filters");
        assert!(f.selected("src/deep/a.rs"));
    }
}
