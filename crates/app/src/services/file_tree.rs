//! 文件树数据层：展平后的可见行缓存 + gitignore 过滤。
//!
//! - [`IgnoreStack`] 逐字搬自 Zed `crates/worktree/src/ignore.rs`（依托
//!   `ignore` crate 的 Gitignore 匹配，与 Zed 同一实现）；本仓暂不接
//!   global gitignore（Zed 的 global_ignore_root 来自其 fs 层，后续补）。
//! - [`flatten`] 把「根目录 + 展开集合」变成排序好的 [`TreeRow`] 扁平表：
//!   每目录读一次盘、按 Zed 语义排序（`paths_sort`）、跳过扫描排除项、
//!   标记 ignore 条目、回填 git 徽标与「子树有变更」目录圆点。渲染层
//!   （function_panel::file_tree）只消费这个缓存，不再碰磁盘。

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::git::{GitFile, GitStatus};
use super::paths_sort::{SortMode, SortOrder, compare_entry_names};

// ---------------------------------------------------------------------------
// IgnoreStack —— 逐字搬自 Zed crates/worktree/src/ignore.rs（去掉
// global_ignore_root / repo_root 字段相关的全局忽略分支）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct IgnoreStack {
    pub top: Arc<IgnoreStackEntry>,
}

#[derive(Debug)]
pub enum IgnoreStackEntry {
    None,
    RepoExclude {
        ignore: Arc<Gitignore>,
        parent: Arc<IgnoreStackEntry>,
    },
    Some {
        abs_base_path: Arc<Path>,
        ignore: Arc<Gitignore>,
        parent: Arc<IgnoreStackEntry>,
    },
    All,
}

#[derive(Debug)]
pub enum IgnoreKind {
    Gitignore(Arc<Path>),
    RepoExclude,
}

use ignore::gitignore::Gitignore;

impl IgnoreStack {
    pub fn none() -> Self {
        Self {
            top: Arc::new(IgnoreStackEntry::None),
        }
    }

    /// Zed 语义：明确无视 ignore 规则时全收（接「显示被忽略文件」设置时启用）
    #[allow(dead_code)]
    pub fn all() -> Self {
        Self {
            top: Arc::new(IgnoreStackEntry::All),
        }
    }

    pub fn append(self, kind: IgnoreKind, ignore: Arc<Gitignore>) -> Self {
        let top = match self.top.as_ref() {
            IgnoreStackEntry::All => self.top.clone(),
            _ => Arc::new(match kind {
                IgnoreKind::Gitignore(abs_base_path) => IgnoreStackEntry::Some {
                    abs_base_path,
                    ignore,
                    parent: self.top.clone(),
                },
                IgnoreKind::RepoExclude => IgnoreStackEntry::RepoExclude {
                    ignore,
                    parent: self.top.clone(),
                },
            }),
        };
        Self { top }
    }

    pub fn is_abs_path_ignored(&self, abs_path: &Path, is_dir: bool) -> bool {
        if is_dir && abs_path.file_name() == Some(std::ffi::OsStr::new(".git")) {
            return true;
        }

        match self.top.as_ref() {
            IgnoreStackEntry::None => false,
            IgnoreStackEntry::All => true,
            IgnoreStackEntry::RepoExclude { ignore, parent } => {
                // Ignore rules from a repository that does not contain this path.
                if !abs_path.starts_with(ignore.path()) {
                    return IgnoreStack {
                        top: parent.clone(),
                    }
                    .is_abs_path_ignored(abs_path, is_dir);
                }

                match ignore.matched(abs_path, is_dir) {
                    ignore::Match::None => IgnoreStack {
                        top: parent.clone(),
                    }
                    .is_abs_path_ignored(abs_path, is_dir),
                    ignore::Match::Ignore(_) => true,
                    ignore::Match::Whitelist(_) => false,
                }
            }
            IgnoreStackEntry::Some {
                abs_base_path,
                ignore,
                parent: prev,
            } => match ignore.matched(abs_path.strip_prefix(abs_base_path).unwrap(), is_dir) {
                ignore::Match::None => IgnoreStack {
                    top: prev.clone(),
                }
                .is_abs_path_ignored(abs_path, is_dir),
                ignore::Match::Ignore(_) => true,
                ignore::Match::Whitelist(_) => false,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// TreeRow + flatten
// ---------------------------------------------------------------------------

/// One visible row of the flattened tree (render order == Vec order).
#[derive(Debug, Clone)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    /// git status of a file row (badge).
    pub git: Option<GitStatus>,
    /// directory row whose subtree contains changes (amber dot).
    pub changed_dot: bool,
    /// gitignored entry（Zed parity：显示但置灰；被忽略目录内部整体继承
    /// ignored，不再逐项匹配——git 语义下 negation 救不回被忽略目录内内容）
    pub ignored: bool,
}

/// git 变更文件的全部祖先目录（相对 cwd 的每一级），目录圆点上浮用。
fn changed_ancestor_dirs(cwd: &Path, files: &[GitFile]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    for f in files {
        let mut dir = f.path.parent();
        while let Some(d) = dir {
            if d == cwd {
                break;
            }
            if d.starts_with(cwd) {
                out.insert(d.to_path_buf());
            }
            dir = d.parent();
        }
    }
    out
}

/// Flatten every directory in `expanded` into rows. Sorting/filtering
/// follow Zed semantics: dot files **visible**（v61 起对齐 Zed，此前误随
/// pi-web 藏点）, only the default scan exclusions (`.git` etc.) skipped,
/// gitignored entries shown dimmed (`ignored`), directories first, natural sort.
///
/// 021 定稿（二改）：**根行不再进树**——项目名由操作栏承载（树顶 40px
/// 固定行，名字居左、操作钮右对齐，右键弹根目录菜单），树体直接从根的
/// 子项排起（depth 0 起，不再有一层幻缩进）。
///
/// 「只看变更」不走这里——那是 [`flatten_changes`]（直接吃 git status
/// 结果，不扫盘；本函数扫盘展开，大仓库上量级完全不同）。
pub fn flatten(cwd: &Path, expanded: &HashSet<PathBuf>, git_files: &[GitFile]) -> Vec<TreeRow> {
    let git_map: HashMap<PathBuf, GitStatus> = git_files
        .iter()
        .map(|f| (f.path.clone(), f.status))
        .collect();
    let mut changed_dirs = changed_ancestor_dirs(cwd, git_files);
    // 根行圆点：任何变更存在即点亮
    if !git_files.is_empty() {
        changed_dirs.insert(cwd.to_path_buf());
    }

    let stack = root_ignore_stack(cwd);

    let mut out = Vec::new();
    if expanded.contains(cwd) {
        walk(cwd, 0, expanded, &git_map, &changed_dirs, &stack, false, &mut out);
    }
    out
}

/// 仓库根的 ignore 栈（`.git/info/exclude` 压栈底，再压根 `.gitignore`；
/// 子目录的 .gitignore 在 walk 下钻时继续压栈）。flatten / flatten_pins
/// 共用的初始化。
fn root_ignore_stack(cwd: &Path) -> IgnoreStack {
    let mut stack = match build_gitignore_at(cwd, ".git/info/exclude") {
        Some(gi) => IgnoreStack::none().append(IgnoreKind::RepoExclude, Arc::new(gi)),
        None => IgnoreStack::none(),
    };
    if let Some(gi) = build_gitignore_at(cwd, ".gitignore") {
        stack = stack.append(IgnoreKind::Gitignore(Arc::from(cwd)), Arc::new(gi));
    }
    stack
}

/// Pin 收藏夹视图（021 三改：Pin = 与「只看变更」同级的独立视图，本质是
/// 文件树过滤器）——每个收藏条目是一棵根树：根行 depth 0（name =
/// file_name，**不链式折叠**——pin 的是显式节点），展开的目录从该路径
/// `walk(depth 1)` 下钻，子级排序/gitignore/链式折叠全复用主树语义。
/// 渲染层以 `depth == 0` 判定收藏根行（子行恒 depth ≥ 1）。
/// 根行失效（外部已删）跳过；调用方（toggle）负责清失效条目，这里只兜底。
pub fn flatten_pins(
    cwd: &Path,
    pins: &[PathBuf],
    expanded: &HashSet<PathBuf>,
    git_files: &[GitFile],
) -> Vec<TreeRow> {
    let git_map: HashMap<PathBuf, GitStatus> = git_files
        .iter()
        .map(|f| (f.path.clone(), f.status))
        .collect();
    // 无根行，cwd 不进 changed_dirs（主树 flatten 的「根圆点」语义不适用）
    let changed_dirs = changed_ancestor_dirs(cwd, git_files);
    let stack = root_ignore_stack(cwd);

    // 展示序：名称字母序（大小写不敏感），同名按完整路径（Windows 盘内
    // 路径大小写不敏感，键用小写化）。非绝对路径不进视图——gitignore 匹配
    // （is_abs_path_ignored）按绝对路径 strip_prefix，收相对路径必炸；正常
    // 入口（tree_pin_toggle）已归一，这里兜历史脏数据。
    let mut roots: Vec<&PathBuf> = pins
        .iter()
        .filter(|p| p.exists() && p.is_absolute())
        .collect();
    roots.sort_by(|a, b| {
        let ka = a
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| a.to_string_lossy().to_lowercase());
        let kb = b
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| b.to_string_lossy().to_lowercase());
        ka.cmp(&kb).then_with(|| a.to_string_lossy().cmp(&b.to_string_lossy()))
    });

    let mut out = Vec::new();
    for p in roots {
        let is_dir = p.is_dir();
        let expanded_root = is_dir && expanded.contains(p);
        let ignored = stack.is_abs_path_ignored(p, is_dir);
        out.push(TreeRow {
            path: p.clone(),
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string()),
            depth: 0,
            is_dir,
            expanded: expanded_root,
            git: git_map.get(p).copied(),
            changed_dot: changed_dirs.contains(p),
            ignored,
        });
        if expanded_root {
            // 根已 ignored → 子级整体继承（walk 的 parent_ignored 同语义）；
            // 否则压入根目录自己的 .gitignore 再下钻
            let child_stack = if ignored {
                stack.clone()
            } else {
                let mut s2 = stack.clone();
                if let Some(gi) = build_gitignore_at(p, ".gitignore") {
                    s2 = s2.append(IgnoreKind::Gitignore(Arc::from(p.as_path())), Arc::new(gi));
                }
                s2
            };
            walk(
                p,
                1,
                expanded,
                &git_map,
                &changed_dirs,
                &child_stack,
                ignored,
                &mut out,
            );
        }
    }
    out
}

/// 「只看变更」的**扁平**行表（021 二改定稿：不要树状，如 pi-web 截图
/// ——头行 N 个文件 +x -y，行 = 文件名前置 + 目录段后置）。
/// 直接吃 `git status` 结果构造，**不读盘**：大仓库上毫秒级（此前
/// changes_only 全量下钻每切一次重扫整棵树，是「检索很慢」的根因）。
/// 行字段约定：`name` = 相对路径（'/' 分隔，渲染层拆目录段/文件名段），
/// depth 恒 0，`git` = 状态徽标，路径字典序。
pub fn flatten_changes(cwd: &Path, git_files: &[GitFile]) -> Vec<TreeRow> {
    let mut files: Vec<&GitFile> = git_files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
        .into_iter()
        .map(|f| {
            let name = f
                .path
                .strip_prefix(cwd)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .replace('\\', "/");
            TreeRow {
                path: f.path.clone(),
                name,
                depth: 0,
                is_dir: false,
                expanded: false,
                git: Some(f.status),
                changed_dot: false,
                ignored: false,
            }
        })
        .collect()
}

/// Per-directory `.gitignore`, or `None` when the directory has none.
fn build_gitignore_at(dir: &Path, rel: &str) -> Option<Gitignore> {
    let path = dir.join(rel);
    if !path.is_file() {
        return None;
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
    let _ = builder.add(&path);
    Some(builder.build().unwrap_or_else(|_| Gitignore::empty()))
}

/// Zed 默认 `file_scan_exclusions`：任何层级匹配这些名字的条目整个不进树
/// （Zed project panel 同款名单；除此之外 dot 文件默认可见）。
const SCAN_EXCLUSIONS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "CVS",
    ".DS_Store",
    "Thumbs.db",
    ".classpath",
    ".settings",
];

#[allow(clippy::too_many_arguments)]
fn walk(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    git_map: &HashMap<PathBuf, GitStatus>,
    changed_dirs: &HashSet<PathBuf>,
    stack: &IgnoreStack,
    parent_ignored: bool,
    out: &mut Vec<TreeRow>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(PathBuf, String, bool, bool)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // Zed parity：dot 文件默认可见（此前「点开头全隐藏」是 pi-web 语义，
        // 与 Zed 不符）；仅扫描排除名单整个不进树。
        if SCAN_EXCLUSIONS.contains(&name.as_str()) {
            continue;
        }
        let Ok(ty) = e.file_type() else { continue };
        let is_dir = ty.is_dir();
        let path = e.path();
        let ignored = parent_ignored || stack.is_abs_path_ignored(&path, is_dir);
        entries.push((path, name, is_dir, ignored));
    }

    // Zed 排序语义：目录先、stem/扩展名两级键、自然排序（DirectoriesFirst
    // + Default；sort_mode 设置后续再接）。par_sort 换 sort：单目录条目量级
    // 下差异可忽略，保持稳定序。
    entries.sort_by(|(pa, na, fa, _), (pb, nb, fb, _)| {
        let ord = compare_entry_names((na, !*fa), (nb, !*fb), SortMode::DirectoriesFirst, SortOrder::Default);
        if ord == Ordering::Equal {
            pa.cmp(pb)
        } else {
            ord
        }
    });

    for (path, name, is_dir, ignored) in entries {
        if is_dir {
            // Zed auto_fold_dirs：只含一个子目录（且无其他子项）的目录不占
            // 行，链式折叠成「a/b/c」一行；手动展开过的目录断链；chevron
            // 逐级展开（展开集合里是真实目录路径）。根行在 flatten 已豁免。
            let mut chain_names = vec![name];
            let mut chain_paths = vec![path.clone()];
            let mut chain_ignored = ignored;
            let mut cur = path.clone();
            while !expanded.contains(&cur) {
                let Some((child_path, child_name)) = sole_child(&cur) else {
                    break;
                };
                if !child_path.is_dir() {
                    break;
                }
                chain_ignored |= stack.is_abs_path_ignored(&child_path, true);
                chain_names.push(child_name);
                chain_paths.push(child_path.clone());
                cur = child_path;
            }
            let open = expanded.contains(&cur);
            out.push(TreeRow {
                path: cur.clone(),
                name: chain_names.join("/"),
                depth,
                is_dir: true,
                expanded: open,
                git: None,
                changed_dot: chain_paths.iter().any(|p| changed_dirs.contains(p)),
                ignored: chain_ignored,
            });
            if open {
                // 被忽略目录内部整体继承 ignored，不再压子 .gitignore 也
                // 不再逐项匹配（git 语义：negation 救不回被忽略目录内内容）
                let child_stack = if chain_ignored {
                    stack.clone()
                } else {
                    let mut s2 = stack.clone();
                    if let Some(gi) = build_gitignore_at(&cur, ".gitignore") {
                        s2 = s2.append(
                            IgnoreKind::Gitignore(Arc::from(cur.as_path())),
                            Arc::new(gi),
                        );
                    }
                    s2
                };
                walk(
                    &cur,
                    depth + 1,
                    expanded,
                    git_map,
                    changed_dirs,
                    &child_stack,
                    chain_ignored,
                    out,
                );
            }
        } else {
            let open = false;
            out.push(TreeRow {
                path: path.clone(),
                name,
                depth,
                is_dir,
                expanded: open,
                git: git_map.get(&path).copied(),
                changed_dot: false,
                ignored,
            });
        }
    }
}

/// 目录的唯一子项（不过滤 dot/ignore——Zed 的 child_entries 同样是全量）；
/// 子项数 ≠ 1 或读失败返回 None。
fn sole_child(dir: &Path) -> Option<(PathBuf, String)> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut only: Option<(PathBuf, String)> = None;
    let mut count = 0;
    for e in rd.flatten() {
        count += 1;
        if count > 1 {
            return None;
        }
        only = Some((e.path(), e.file_name().to_string_lossy().to_string()));
    }
    only.filter(|_| count == 1)
}

// ---------------------------------------------------------------------------
// 测试：临时树 + gitignore + 排序语义（std::env::temp_dir，无新依赖）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTree(PathBuf);
    impl TempTree {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "piflash-tree-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            Self(base)
        }
        fn dir(&self, rel: &str) -> PathBuf {
            std::fs::create_dir_all(self.0.join(rel)).unwrap();
            self.0.join(rel)
        }
        fn file(&self, rel: &str) -> PathBuf {
            if let Some(p) = self.0.join(rel).parent() {
                std::fs::create_dir_all(p).unwrap();
            }
            std::fs::write(self.0.join(rel), "x").unwrap();
            self.0.join(rel)
        }
    }
    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn flatten_orders_and_lazily_expands() {
        let t = TempTree::new("flat");
        t.file("src/main.rs");
        t.file("a.txt");
        t.file("b.txt");
        t.file("z.md");
        t.dir("node_modules/pkg"); // 未展开的目录不应下钻

        // 未展开任何目录：空树（根行已由操作栏承载，021 二改）
        let rows = flatten(&t.0, &HashSet::new(), &[]);
        assert!(rows.is_empty());

        // 展开根 + node_modules：目录先 + 自然排序，根子项 depth 0 起
        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        expanded.insert(t.0.join("node_modules"));
        let rows = flatten(&t.0, &expanded, &[]);
        // 先序遍历：pkg 紧跟 node_modules（树形显示顺序）
        let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "node_modules".to_string(),
                "pkg".to_string(),
                "src".to_string(),
                "a.txt".to_string(),
                "b.txt".to_string(),
                "z.md".to_string(),
            ]
        );
        let node_modules = rows.first().expect("node_modules row");
        assert!(node_modules.is_dir && node_modules.expanded);
        assert_eq!(node_modules.depth, 0);
        let src = rows.iter().find(|r| r.name == "src").expect("src row");
        assert!(!src.expanded, "unexpanded dir stays collapsed");
        let pkg = rows.iter().find(|r| r.name == "pkg").expect("pkg row");
        assert_eq!(pkg.depth, 1);
        assert_eq!(pkg.path, t.0.join("node_modules").join("pkg"));
    }

    #[test]
    fn flatten_applies_gitignore_and_repo_exclude() {
        let t = TempTree::new("ig");
        std::fs::write(t.0.join(".gitignore"), "build/\n*.log\n!keep.log\n").unwrap();
        t.file("build/out.js");
        t.file("debug.log");
        t.file("keep.log");
        t.file("src/lib.rs");

        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        let rows = flatten(&t.0, &expanded, &[]);
        let by_name = |n: &str| {
            rows.iter()
                .find(|r| r.name == n)
                .unwrap_or_else(|| panic!("row {n}"))
        };
        // v60 起 ignored 条目显示但置灰标记（Zed parity），不再隐藏
        let build = by_name("build");
        assert!(build.ignored, "gitignored dir shown dimmed");
        let debug = by_name("debug.log");
        assert!(debug.ignored, "gitignored file shown dimmed");
        let keep = by_name("keep.log");
        assert!(!keep.ignored, "whitelist survives as normal");
        assert!(!by_name("src").ignored, "normal dir unmarked");
        // v61 起 dot 文件可见（Zed parity）：.gitignore 本身也是普通行
        assert!(!by_name(".gitignore").ignored, "dot file shown as normal row");
        // src/ 未展开不显示子文件，但目录本身在
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["build", "src", ".gitignore", "debug.log", "keep.log"]
        );

        // .git/info/exclude 也生效（同样显示但标记）；.git 目录本身不进树
        let git_dir = t.0.join(".git");
        std::fs::create_dir_all(git_dir.join("info")).unwrap();
        std::fs::write(git_dir.join("info").join("exclude"), "secret.txt\n").unwrap();
        t.file("secret.txt");
        let rows = flatten(&t.0, &expanded, &[]);
        let secret = rows
            .iter()
            .find(|r| r.name == "secret.txt")
            .expect("excluded file shown");
        assert!(secret.ignored, "repo-excluded file shown dimmed");
        assert!(rows.iter().all(|r| r.name != ".git"), ".git scanned out");
    }

    #[test]
    fn dot_files_visible_but_scan_exclusions_hidden() {
        let t = TempTree::new("dotvis");
        std::fs::write(t.0.join(".gitignore"), ".beads/\n").unwrap();
        t.file(".gitattributes");
        t.dir(".beads");
        t.file(".beads/issues.jsonl");
        std::fs::create_dir_all(t.0.join(".git").join("objects")).unwrap();
        t.file(".DS_Store");

        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        expanded.insert(t.0.join(".beads"));
        let rows = flatten(&t.0, &expanded, &[]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        // 目录先（.beads 展开随后子行），dot 文件按普通条目参与排序
        assert_eq!(
            names,
            vec![".beads", "issues.jsonl", ".gitattributes", ".gitignore"]
        );
        assert!(rows[0].ignored, "gitignored dot dir shown dimmed");
        assert!(rows[1].ignored, "ignored dir children inherit the mark");
        assert!(!rows[2].ignored && !rows[3].ignored, "plain dot files normal");
        // Zed 默认 file_scan_exclusions：.git / .DS_Store 整个不进树
        assert!(names.iter().all(|n| *n != ".git" && *n != ".DS_Store"));
    }

    #[test]
    fn auto_folds_single_child_dir_chains() {
        let t = TempTree::new("fold");
        t.file("a/x/y/f.txt");
        t.file("r.txt");

        // 全折叠：a/x/y 折成一行（Zed auto_fold_dirs 语义）
        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        let rows = flatten(&t.0, &expanded, &[]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a/x/y", "r.txt"]);
        assert_eq!(rows[0].path, t.0.join("a").join("x").join("y"));
        assert!(!rows[0].expanded);

        // 展开链尾：行名不变、行内打开，f.txt 深度 1
        expanded.insert(t.0.join("a").join("x").join("y"));
        let rows = flatten(&t.0, &expanded, &[]);
        assert_eq!(rows[0].name, "a/x/y");
        assert!(rows[0].expanded);
        assert_eq!(rows[1].name, "f.txt");
        assert_eq!(rows[1].depth, 1);

        // 展开中段：断链成 a/x + y 两行（y 仍保持展开，f.txt 随其下钻）
        expanded.insert(t.0.join("a").join("x"));
        let rows = flatten(&t.0, &expanded, &[]);
        assert_eq!(rows[0].name, "a/x");
        assert!(rows[0].expanded);
        assert_eq!(rows[1].name, "y");
        assert_eq!(rows[1].depth, 1);
        assert!(rows[1].expanded);
        assert_eq!(rows[2].name, "f.txt");
        assert_eq!(rows[2].depth, 2);
    }

    #[test]
    fn changed_dot_covers_deep_ancestors() {
        let t = TempTree::new("dot");
        let deep = t.dir("a/b/c");
        let changed = t.file("a/b/c/mod.rs");
        let git_files = vec![GitFile {
            path: changed,
            status: GitStatus::Modified,
            staged: false,
        }];
        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        expanded.insert(t.0.join("a"));
        expanded.insert(t.0.join("a").join("b"));
        expanded.insert(deep);

        let rows = flatten(&t.0, &expanded, &git_files);
        let a = rows.iter().find(|r| r.name == "a").unwrap();
        assert!(a.changed_dot, "ancestor dir a gets the dot");
        let c = rows.iter().find(|r| r.name == "c").unwrap();
        assert!(c.changed_dot);
        let m = rows.iter().find(|r| r.name == "mod.rs").unwrap();
        assert!(matches!(m.git, Some(GitStatus::Modified)));
        assert!(!m.changed_dot);
    }

    #[test]
    fn flatten_changes_is_flat_relative_paths() {
        let t = TempTree::new("chg");
        let m = t.file("a/b/mod.rs");
        let r = t.file("root.txt");
        let u = t.file("src/new.txt");
        let git_files = vec![
            GitFile { path: u, status: GitStatus::Untracked, staged: false },
            GitFile { path: r, status: GitStatus::Modified, staged: false },
            GitFile { path: m, status: GitStatus::Modified, staged: false },
        ];
        let rows = flatten_changes(&t.0, &git_files);
        // 扁平（depth 恒 0）+ 路径字典序 + '/' 分隔相对路径 + 徽标回填
        let rows: Vec<(String, usize, Option<GitStatus>)> = rows
            .into_iter()
            .map(|r| (r.name, r.depth, r.git))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("a/b/mod.rs".to_string(), 0, Some(GitStatus::Modified)),
                ("root.txt".to_string(), 0, Some(GitStatus::Modified)),
                ("src/new.txt".to_string(), 0, Some(GitStatus::Untracked)),
            ]
        );

        // 无变更 = 空表（渲染层出「暂无变更」空态）
        assert!(flatten_changes(&t.0, &[]).is_empty());
    }

    #[test]
    fn flatten_pins_roots_sorted_children_via_walk() {
        let t = TempTree::new("pins");
        t.file("src/deep/lib.rs");
        t.file("docs/guide.md");
        std::fs::write(t.0.join("docs/.gitignore"), "draft.md\n").unwrap();
        t.file("docs/draft.md");
        t.file("beta.txt");

        // pin 序故意乱：展示序 = 名称字母序；src 未展开 = 折叠根行
        let pins = vec![t.0.join("src"), t.0.join("beta.txt"), t.0.join("docs")];
        let rows = flatten_pins(&t.0, &pins, &HashSet::new(), &[]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["beta.txt", "docs", "src"]);
        assert!(rows.iter().all(|r| r.depth == 0), "未展开只有三根行");

        // 展开 docs：根行 depth 0 + 子行 depth 1（gitignore 在根目录内生效；
        // .gitignore 自身是普通可见行，v61 dot 文件语义）
        let mut expanded = HashSet::new();
        expanded.insert(t.0.join("docs"));
        let rows = flatten_pins(&t.0, &pins, &expanded, &[]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["beta.txt", "docs", ".gitignore", "draft.md", "guide.md", "src"]
        );
        let docs = &rows[1];
        assert!(docs.is_dir && docs.expanded && docs.depth == 0);
        let draft = &rows[3];
        assert_eq!(draft.depth, 1);
        assert!(draft.ignored, "根内 .gitignore 命中的子行置灰");
        assert!(!rows[4].ignored, "guide.md 是普通行");

        // 失效条目（外部已删）兜底跳过
        let stale = vec![t.0.join("nope"), t.0.join("beta.txt")];
        let rows = flatten_pins(&t.0, &stale, &HashSet::new(), &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "beta.txt");
    }
}
