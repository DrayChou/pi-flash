---
title: "pi-flash 文件树展开卡顿排查报告"
subtitle: "子目录仓库 + `git status -uall` 全仓扫描，导致每次点击文件夹在主线程重建 270 万条 git 记录"
date: 2026-10-19
project: "danielbill/pi-flash"
version_tested: "0.2.0"
platform: "Windows 10 19045 x64"
severity: "high"
status: "root cause confirmed; patch proposed"
---

# pi-flash 文件树展开卡顿排查报告

> 给作者：本报告按「现象 → 可复现实验 → 源码 → 根因 → 修复」组织。所有源码行号基于 0.2.0 源码树 `HEAD 1639b9a`（2026-10-19）；文末附可直接 `git apply` 的 `patch.diff`。  
> 报告中的 `HOME` / `PROJECT` 是占位符，实际测试环境见 §2。  
> 整理方式：用户机器实测 + 源码阅读 + 数据结构微基准；AI 辅助整理。未在 app 内埋点，因此「点击耗时」给的是**同逻辑微基准下限**，不是 instrumentation 数值。

---

## 0. 机器可读摘要

```json
{
  "issue": "project-panel folder expand freezes for several seconds",
  "pi_flash_version": "0.2.0",
  "source_head": "1639b9a",
  "os": "Windows 10 19045.6456 x64",
  "git": "2.56.0.windows.1",
  "project_path": "C:/Users/Administrator/pi-cwd-20260913",
  "repo_root": "C:/Users/Administrator",
  "severity": "high",
  "root_cause": [
    "git_status_files() runs `git status --porcelain=v1 -z --untracked-files=all` without a pathspec; cwd is a subdirectory of an abandoned HOME git repo that contains Windows junctions",
    "the command enumerates 2702629 status records (433919069 bytes, 99.873 s)",
    "every folder click calls rebuild_tree() synchronously on the UI thread; flatten() clones all 2.7M paths into a HashMap and computes changed ancestor dirs (7.554 s lower bound in a C++ mirror)",
    "porcelain paths are repo-root-relative but the code joins them onto cwd, producing 342265 fake ancestor directories and wrong git badges",
    "the recursive fs watcher re-runs the 100 s scan on every write; the generation guard discards stale results but does not cancel in-flight git processes"
  ],
  "key_files": [
    "crates/app/src/services/git.rs:80",
    "crates/app/src/services/git.rs:119",
    "crates/app/src/function_panel/git_panel.rs:745",
    "crates/app/src/function_panel/git_panel.rs:766",
    "crates/app/src/function_panel/file_tree.rs:1030",
    "crates/app/src/function_panel/file_tree.rs:1331",
    "crates/app/src/services/file_tree.rs:163",
    "crates/app/src/services/file_tree.rs:191",
    "crates/app/src/startup.rs:503",
    "crates/app/src/startup.rs:540"
  ],
  "metrics": {
    "unscoped_status_seconds": 99.873,
    "unscoped_status_records": 2702629,
    "unscoped_status_bytes": 433919069,
    "scoped_status_seconds": 0.095,
    "scoped_status_records": 3289,
    "scoped_status_bytes": 345887,
    "project_full_walk_seconds": 0.16,
    "project_files": 16714,
    "project_dirs": 1930,
    "microbench_map_seconds": 1.865,
    "microbench_ancestor_seconds": 5.506,
    "microbench_total_seconds": 7.554,
    "fake_ancestor_dirs": 342265
  },
  "fix_summary": "add `-- .` to git status and numstat; resolve porcelain paths via `git rev-parse --show-toplevel`; consume the second NUL record of rename/copy entries"
}
```

---

## 1. 结论（TL;DR）

**卡顿不在文件树的 `read_dir`，而在「git 状态列表」被放大到了 270 万条。**

完整因果链：

1. `PROJECT` 自身没有 `.git`，它位于 `HOME` 的一个 2023 年遗留、没有 commit 的 git 仓库里。
2. `git_status_files()` 执行 `git status --porcelain=v1 -z --untracked-files=all`，**没有 pathspec**。`git status` 在子目录运行时仍扫描整个仓库，`-uall` 会递归列出所有未跟踪文件；`HOME` 里还有 Windows junction，导致重复递归。
3. 实测该命令在 `PROJECT` 下产生 **2,702,629 条记录、433,919,069 字节、99.873 秒**。
4. 这些记录被整体存进 `ws.git_files`。
5. 用户点击任意文件夹时，`on_click` 在 **UI 线程**同步调用 `rebuild_tree()` → `flatten()`：
   - 为 270 万条记录建 `HashMap<PathBuf, GitStatus>`；
   - 为 270 万条记录遍历祖先目录。
6. 按同样数据结构做的微基准：**1.865 s + 5.506 s = 7.554 s**（C++/O2，已是最乐观下限；Rust 还要多做 270 万次 `PathBuf::clone`，通常更慢）。
7. 附带问题：`git status` 输出的是**仓库根相对路径**，代码却用 `cwd.join(file_path)`，于是 270 万条路径全部变成 `PROJECT/AppData/...` 这类假路径，产生 **342,265 个假祖先目录**，git 徽标/变更圆点也对不上真实文件。

**最小修复**：给 `git status` / `git diff --numstat` 加 `-- .`；把 porcelain 路径按 `git rev-parse --show-toplevel` 的仓库根解析，而不是按 cwd 解析；同时正确处理 `-z` 下 rename/copy 的第二条 NUL 记录。

---

## 2. 环境与版本

| 项 | 值 |
|---|---|
| pi-flash | 0.2.0（安装包 `pi-flash-0.2.0-win32-x64`，exe 时间 2026-10-18 16:35） |
| 源码 | `danielbill/pi-flash`，`HEAD 1639b9a`（2026-10-19 20:49 +0800），`crates/app/Cargo.toml` version 0.2.0 |
| OS | Windows 10 10.0.19045.6456 x64 |
| CPU / RAM | Intel i5-12490F / 15.8 GB |
| Git | `git version 2.56.0.windows.1` |
| `HOME` | `C:\Users\Administrator` |
| `PROJECT` | `C:\Users\Administrator\pi-cwd-20260913` |
| 触发操作 | 在左侧项目区点击文件夹展开（例如 `03-研究`） |

> 说明：安装的 exe 构建于 10-18，源码阅读基于 10-19 的 0.2.0 HEAD；`git_status_files` / `flatten` / fs watcher 相关代码在 0.2.0 中均在。若作者本地行号有偏移，以函数名为准。

---

## 3. 现象

- 左 dock 的项目区（文件树）点开文件夹时，界面卡住数秒；期间窗口不重绘、点击无响应。
- 项目根只有 6 个目录 + 1 个 `README.md`（`.pi`、`01-进行中`、`02-项目`、`03-研究`、`04-归档`、`99-临时`、`README.md`），子目录即时条目也只有 1–14 项，不符合「目录太大读盘慢」的直觉。
- 打开一段时间后更明显；有时不点文件夹也会卡一下。
- 文件树上基本看不到 git 徽标/目录变更圆点（与 §5.3 的路径 bug 一致）。

---

## 4. 实测证据

### 4.1 仓库拓扑：`PROJECT` 被 `HOME` 的遗留仓库罩住

```bash
$ cd "$PROJECT"
$ git rev-parse --show-toplevel
C:/Users/Administrator
```

`HOME/.git` 的实际情况：

```bash
$ git -C "$HOME" log --oneline -1
fatal: your current branch 'main' does not have any commits yet

$ git -C "$HOME" ls-files | wc -l
0

$ git -C "$HOME" count-objects -v
count: 1
size: 0
in-pack: 0
packs: 0
size-pack: 0
prune-packable: 0
garbage: 0
size-garbage: 0

$ ls -la "$HOME/.git/index"
ls: cannot access '.../.git/index': No such file or directory

$ ls -la "$HOME/.git/index.lock"
-rw-r--r-- 1 Administrator 197121 0 2026-10-16 08:10 .../.git/index.lock
```

- `.git` 目录创建时间：**2023-04-23 21:12:09 +0800**。
- 没有 commit、没有 index、没有 refs；`count-objects` 只有 1 个对象（空 tree）。
- `remote.origin.url` 指向一个与 pi-flash 无关的私有仓库，URL 已隐去。
- `index.lock` 是 2026-10-16 留下的 0 字节残留，说明曾有一次 git 写操作被中断。

`HOME` 下存在 Windows 兼容性 junction（目录联接）：

```text
Application Data
Cookies
Local Settings
My Documents
NetHood
PrintHood
Recent
SendTo
Templates
「开始」菜单
```

`git status --untracked-files=all` 会把这些 junction 当普通目录跟随，输出中出现重复路径段，例如：

```text
?? Local Settings/Application Data/Application Data/.../file
warning: could not open directory 'Local Settings/Application Data/...': Filename too long
```

这就是 270 万条记录的主要来源。

### 4.2 `git status`：带不带 `-uall`、带不带 pathspec 的差距

在 `PROJECT` 下执行（Git Bash，单次实测）：

| 命令 | 耗时 | 记录数 | 输出大小 |
|---|---:|---:|---:|
| `git status --porcelain=v1 -z --untracked-files=normal` | ~0.06 s | 目录级 | 很小 |
| `git status --porcelain=v1 -z --untracked-files=all` | **99.873 s** | **2,702,629** | **433,919,069 B（414 MB）** |
| `git status --porcelain=v1 -z --untracked-files=all -- .` | **0.095 s** | **3,289** | **345,887 B（338 KB）** |

关键点：

- 不带 `-- .` 时，虽然 `cwd` 是 `PROJECT`，`git status` 仍输出 `AppData/...`、`Local Settings/...`、`Desktop/...` 等**整个 `HOME` 仓库**的路径。
- 不带 `-uall` 时，git 只把未跟踪目录折叠成一条，不递归，所以看起来很快；`-uall` 才是灾难开关。
- 加 `-- .` 后只扫描 `PROJECT`，记录数降到 3,289，耗时 0.095 s。

### 4.3 项目自身并不大

对 `PROJECT` 做完整递归遍历（Python `os.walk`，不含 junction 展开）：

```text
files 16714  dirs 1930  walk_sec 0.16
```

根目录即时条目：

```text
01-进行中: 1
02-项目: 3
03-研究: 14
04-归档: 2
99-临时: 5
```

所以 `read_dir`、文件数量、UI 列表虚拟化都不是本次卡顿的主因。

### 4.4 点击路径的微基准：7.554 s 下限

把 `flatten()` 的两段逻辑按相同数据结构写成 C++17 微基准（`-O2`），输入为上面真实的 2,702,629 条 `git status -z` 输出：

```text
records=2702629 joined_map=2702629 fake_ancestor_dirs=342265
read=182ms parse+join+hashmap=1865ms ancestor_walk=5506ms total=7554ms
```

对应 app 内的代码：

```rust
// crates/app/src/services/file_tree.rs:191
let git_map: HashMap<PathBuf, GitStatus> = git_files
    .iter()
    .map(|f| (f.path.clone(), f.status))   // 2.7M 次 PathBuf clone + HashMap 插入
    .collect();
let mut changed_dirs = changed_ancestor_dirs(cwd, git_files);  // 2.7M 次祖先遍历
```

说明：

- C++ 基准包含读取/解析 414 MB 的时间；app 点击时省掉读取，但要多做 270 万次 `PathBuf::clone`，量级相同。
- 这是**下限**：Rust 的 `PathBuf` 哈希、分配器、gpui 主线程调度都会让它更慢。
- 微基准源码见 `bench/flatten_microbench.cpp`。

### 4.5 旁证：进程内存

实测 `pi-flash.exe`（启动于 21:09:45）：

```text
WorkingSet   427 MB
PrivateBytes 273 MB
CPU          235.6 s（累计）
```

这与持有数百万个 `PathBuf` 的量级相符，但不是定量证明；主证据是 §4.2 和 §4.4。

---

## 5. 源码链路

### 5.1 `git_status_files()`：没有 pathspec 的全仓扫描

```rust
// crates/app/src/services/git.rs:80
pub fn git_status_files(cwd: &Path) -> Vec<GitFile> {
    let Some(out) = run_git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    else {
        return Vec::new();
    };
    // ...
    let full = cwd.join(file_path);   // line 119
    files.push(GitFile { path: full, status, staged });
}
```

问题有两层：

1. **没有 `-- .`**：`git -C cwd status` 仍扫描整个仓库；`-uall` 递归所有未跟踪目录。
2. **路径基准错了**：porcelain 输出是仓库根相对路径，不是 cwd 相对路径。`cwd.join()` 在 `cwd == repo_root` 时碰巧正确，在 `cwd` 是子目录时全部拼错。

### 5.2 点击文件夹 → UI 线程同步 `rebuild_tree()` → `flatten()`

```rust
// crates/app/src/function_panel/file_tree.rs:1030
el = el.on_click(move |_, _, cx| {
    let _ = weak.update(cx, |c, cx| {
        c.ws.tree_selected = Some(dir_path.clone());
        if !c.ws.expanded_dirs.remove(&dir_path) {
            c.ws.expanded_dirs.insert(dir_path.clone());
        }
        c.rebuild_tree();   // line 1036：同步，UI 线程
        cx.notify();
    });
});
```

```rust
// crates/app/src/function_panel/file_tree.rs:1331
pub(crate) fn rebuild_tree(&mut self) {
    // ...
    self.ws.tree_rows = std::sync::Arc::new(if self.ws.pin_view {
        services::file_tree::flatten_pins(...)
    } else if self.ws.changes_only {
        services::file_tree::flatten_changes(&cwd, &self.ws.git_files)
    } else {
        services::file_tree::flatten(&cwd, &self.ws.expanded_dirs, &self.ws.git_files)
    });
}
```

```rust
// crates/app/src/services/file_tree.rs:163
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
```

这条路径是同步的：点击事件 → `rebuild_tree` → `flatten` → 270 万次 clone + 祖先遍历，全部在 UI 线程完成。渲染层虽然做了虚拟化，但**卡在建索引阶段，还没到渲染**。

### 5.3 路径拼接 bug：假祖先目录 + 错误的 git 徽标

实际 porcelain 输出（带 `-- .` 时）：

```text
pi-cwd-20260913/01-进行中/示例目录/示例.md
```

代码 `cwd.join(file_path)` 得到：

```text
C:\Users\Administrator\pi-cwd-20260913\pi-cwd-20260913\01-进行中\...
```

不带 `-- .` 时更夸张：

```text
C:\Users\Administrator\pi-cwd-20260913\AppData\...
C:\Users\Administrator\pi-cwd-20260913\Local Settings\...
```

因为所有拼接结果都以 `cwd` 开头，`changed_ancestor_dirs` 的 `d.starts_with(cwd)` 对所有 270 万条都成立，于是：

- 实测产生 **342,265 个假祖先目录**；
- `git_map.get(&path)` 永远匹配不到真实文件，文件树的 git 徽标/变更圆点失效；
- git 面板的路径也指向项目内不存在的假文件。

### 5.4 放大器：fs watcher 反复触发 100 秒扫描

```rust
// crates/app/src/startup.rs:503
fn spawn_fs_watch_pump(cx: &mut gpui::Context<Chat>) {
    // ...
    let ok = this.update(cx, |chat, cx| {
        chat.refresh_git(cx);   // line 540
        // ...
    });
}
```

- `refresh_git()` 在 `crates/app/src/function_panel/git_panel.rs:745`，后台执行 `git_status_files + git_numstat`（`background_spawn`，line 766）。
- 任何被 watcher 认为 interesting 的项目内写入（过滤 `.git` 与编辑器临时文件后）都会触发 watcher → 再跑一次 100 秒 / 414 MB 的扫描。
- `git_refresh_gen` / `git_files_cwd` 只保证「过期结果不换入」，**不会取消已经在跑的 git 进程**；连续写文件时可能堆积多个 git 进程。
- 扫描完成后的换入回调仍然在 UI 线程调用 `chat.rebuild_tree()`（line 774），所以**不点文件夹也可能冻一下**。

---

## 6. 根因链（压缩版）

```text
PROJECT 没有 .git
  → git 仓库根 = HOME（2023 年遗留、无 commit、无 index）
  → git_status_files(cwd=PROJECT) 执行 git status -uall，无 pathspec
  → git 扫描整个 HOME，跟随 junction，重复递归
  → 2,702,629 条记录 / 414 MB / 99.873 s
  → ws.git_files 持有 270 万条 GitFile
  → 用户点击文件夹
  → on_click → rebuild_tree()（UI 线程）
  → flatten(): HashMap clone 270 万 + changed_ancestor_dirs 270 万
  → UI 冻结（微基准下限 7.554 s）

并行放大器：
任何项目内写入 → fs watcher → refresh_git() → 再跑 99.873 s
  → 完成后 ws.git_files = files; rebuild_tree()（UI 线程）→ 再冻一次
```

---

## 7. 影响面

1. **本机这个项目**：每次展开文件夹卡数秒；周期性地因为 fs watcher 完成换入而卡顿；内存多出数百 MB。
2. **任何「项目是某个大仓库子目录」的用户**：即使没有 junction，只要仓库根在项目上方，`git status -uall` 都会扫描整个仓库。对 monorepo / 家目录仓库 / 下载目录仓库，同样会退化，只是倍数不同。
3. **git 功能正确性**：只要 `cwd != repo_root`，文件树徽标和 git 面板路径就是错的（§5.3）。
4. **磁盘/CPU**：每次 fs 事件触发 100 秒扫描；无取消机制，连续写入时可能堆积 git 进程。
5. **「只看变更」视图**：`flatten_changes` 会对全部 `git_files` 排序；270 万条时同样退化（只是不做祖先遍历）。

---

## 8. 建议修复

### 8.1 最小 patch

完整可应用补丁见同目录 `patch.diff`（基于 `HEAD 1639b9a`）。核心改动：

```diff
-    let Some(out) = run_git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])
-    else {
+    let Some(out) = run_git(
+        cwd,
+        &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."],
+    ) else {
         return Vec::new();
     };
+    // Porcelain paths are relative to the repository root, not to cwd.
+    let repo_root = run_git(cwd, &["rev-parse", "--show-toplevel"])
+        .map(|s| PathBuf::from(s.trim()))
+        .unwrap_or_else(|| cwd.to_path_buf());
     let mut files = Vec::new();
-    // -z: NUL-separated records; rename records embed a TAB then the new name
-    for rec in out.split('\0') {
+    // -z: records are NUL-separated. Rename/copy records have a second
+    // NUL-separated record (the old path) that has no "XY " prefix.
+    let mut records = out.split('\0');
+    while let Some(rec) = records.next() {
+        // ... existing prefix parsing (xy / rest / x_byte / staged) ...
+        let is_rename_or_copy = matches!(xy.as_bytes()[0], b'R' | b'C')
+            || matches!(xy.as_bytes()[1], b'R' | b'C');
         // ...
-        let full = cwd.join(file_path);
+        let full = repo_root.join(file_path);
         files.push(GitFile { path: full, status, staged });
+        if is_rename_or_copy {
+            let _ = records.next(); // consume old-path record
+        }
     }
```

`git_numstat` 同样加 pathspec：

```diff
-        &["diff", "--no-color", "--no-ext-diff", "--numstat", "HEAD"],
+        &["diff", "--no-color", "--no-ext-diff", "--numstat", "HEAD", "--", "."],
```

### 8.2 逐条解释

| 改动 | 作用 |
|---|---|
| `git status ... -- .` | 把扫描限制在 `cwd`；本环境 99.873 s → 0.095 s，2,702,629 → 3,289 条 |
| `rev-parse --show-toplevel` + `repo_root.join` | 修正子目录场景的绝对路径；徽标/圆点/git 面板才能匹配真实文件 |
| `-z` 下不按 TAB 切 rename | `-z` 的 rename 是 `R  new\0old\0` 两条记录，没有 TAB；原代码的 `split_once('\t')` 永远不命中 |
| 消费 rename/copy 的第二条记录 | 防止把 old path 当成一个独立 status 条目（会产生错误的 Added/Modified 行） |
| `git diff --numstat ... -- .` | 只统计项目内的 +/−，避免家目录仓库里项目外的改动污染徽标 |

**行为变化（有意）**：`git status` 现在只返回 `PROJECT` 内的变更。文件树和 git 面板本来展示的就是这个项目；如果需要「全仓库视图」，应另做一个全局 git 视图，而不是把全仓库塞进项目面板。

### 8.3 验收标准 / 测试建议

建议加一个使用临时 git 仓库的集成测试（或手工验证）：

1. 建临时仓库 `R`，`R/sub` 为项目 cwd，`R/sibling` 放 10 万个未跟踪文件。
2. 在 `R/sub` 下调用 `git_status_files`：
   - 只返回 `R/sub` 下的条目；
   - 不包含 `R/sibling` 的路径；
   - 返回绝对路径以 `R/sub` 为前缀。
3. `R/sub` 内放一个 modified tracked 文件：断言 `GitFile.path` 能被 `flatten` 的 `git_map.get(&tree_path)` 命中。
4. rename 一个文件：断言只产生 1 条 `Renamed`，路径是新路径，没有被误解析的旧路径条目。
5. 性能门限（建议）：`git status -- .` 在 10 万文件仓库的子目录下 < 0.5 s；`flatten` 的 map/ancestor 构建只与项目内变更数相关，不随仓库总文件数增长。
6. 手动：在 pi-flash 里打开一个「大仓库的子目录」项目，点文件夹应无多秒冻结；文件树的 M/A/U 徽标应出现。

### 8.4 可选加固（不阻塞本次修复）

1. **缓存 `git_map` / `changed_dirs`**：只在 `ws.git_files` 变化时重建，不要每次点击都重建。当前 `flatten()` 每次都 `clone` 全部 `PathBuf`，即使修好 scoping，在超大仓库/大量变更下仍不便宜。
2. **减少 clone**：`HashMap<&Path, GitStatus>` / `Arc<Path>` / 预计算索引，避免 270 万次堆分配。
3. **`rebuild_tree` 后台化**：大树上可以后台构建，再原子换入；但要注意与展开状态、选中的竞态（Zed 的 worktree 模型更彻底）。
4. **取消在途 git**：`git_refresh_gen` 目前只丢弃结果；可以在新请求到来时 `kill` 旧子进程，或串行化（single-flight）扫描。
5. **防御性日志**：当 `repo_root != cwd` 且 `git_files.len()` 超过阈值时，打一条带 `repo_root` / `cwd` / 记录数的 warning，方便下次定位。

---

## 9. 用户侧临时规避（不改代码）

### 方案 A：移走遗留的 `HOME/.git`（本环境最省事）

先确认它确实不是你在维护的仓库：

```bash
git -C "$HOME" log --oneline -1      # 本环境：no commits yet
git -C "$HOME" ls-files | wc -l      # 本环境：0
```

然后改名备份（不要直接删）：

```powershell
Rename-Item C:\Users\Administrator\.git C:\Users\Administrator\.git.bak-20261019
```

移走后，`PROJECT` 不再是 git 仓库，`git status` 立即返回非仓库错误；文件树点击不再卡，但 git 面板/徽标会为空。确认无影响后，备份目录可以自行删除。

### 方案 B：让项目成为独立仓库（需要 git 功能时）

```powershell
cd C:\Users\Administrator\pi-cwd-20260913
git init
```

此时 `repo_root == cwd`，`git status` 只扫描项目（16,714 个文件，实际 ~0.16 s 量级）。建议同时写 `.gitignore`，否则 git 面板会列出 3,000+ 个 untracked 文件。

> 注意：`HOME/.git/index.lock` 是 0 字节残留；如果你保留 `HOME` 仓库并要执行 `git add/commit`，先确认没有 git 进程再删除它，否则会报 `index.lock exists`。

---

## 10. 排除项：不是这些原因

| 猜测 | 为什么不是 |
|---|---|
| 文件太多，`read_dir` 慢 | 项目全量 walk 0.16 s；根目录 6 个目录 + 1 个 README；展开只读当前目录 |
| UI 列表没有虚拟化 | 渲染层已虚拟化；卡在渲染前的 `rebuild_tree`/`flatten`，不是画行 |
| 后台跑 git 导致主线程阻塞 | 扫描本身在 `background_spawn`；卡的是「点击时同步 `rebuild_tree`」和「扫描完成回调里的 `rebuild_tree`」 |
| git 本身慢 | 同一仓库加 `-- .` 后 0.095 s；慢的是「全仓 `-uall` + junction 递归」 |
| 项目 `.gitignore` 太多 | 与本次无关；scoped 后 3,289 条，已足够快 |

---

## 11. 附录

### A. 复现命令

见同目录 `repro.sh`。核心：

```bash
cd "$PROJECT"
git rev-parse --show-toplevel

# 注意：可能产生 400+ MB 输出，耗时 100 秒量级
git status --porcelain=v1 -z --untracked-files=all > /tmp/status-unscoped.bin
wc -c /tmp/status-unscoped.bin
tr -cd '\0' < /tmp/status-unscoped.bin | wc -c

# 对照：加 pathspec
git status --porcelain=v1 -z --untracked-files=all -- . > /tmp/status-scoped.bin
wc -c /tmp/status-scoped.bin
tr -cd '\0' < /tmp/status-scoped.bin | wc -c
```

### B. 包内文件

```text
README.md                       本报告
patch.diff                      建议补丁（基于 HEAD 1639b9a）
repro.sh                        复现脚本（Git Bash）
bench/flatten_microbench.cpp    7.554 s 微基准（C++17，仅模拟数据结构）
```

### C. 应用补丁

```bash
cd /path/to/pi-flash
git apply --check patch.diff
git apply patch.diff
cargo test -p app
```

> 本报告环境没有 Rust 工具链，**补丁未在本机编译/运行**；行号与上下文基于 `HEAD 1639b9a`。如果作者树已变化，按 §8.1 的语义手工合并即可。

### D. 限制与未验证项

1. 没有在 app 内埋点测量真实点击耗时；7.554 s 是同逻辑 C++ 微基准下限，真实值可能更高。
2. 安装 exe 构建于 2026-10-18，源码阅读基于 2026-10-19 的 0.2.0 HEAD；若两者在 `git.rs`/`file_tree.rs` 上有差异，以作者本地为准。
3. `HOME` 仓库的私有 remote URL 已隐去；`HOME` 路径与 junction 名称保留，因为它们是复现条件的一部分。
4. 未验证 Windows 盘符大小写/符号链接极端场景；`Path` 的组件比较在常规 `C:` 盘符下工作，若作者担心可在测试中加 `repo_root`/`cwd` 前缀一致性断言。
5. `git status -- .` 的行为已在 Git 2.56.0.windows.1 上实测；更低版本 Git 的 porcelain 路径语义相同，但建议作者在 CI 覆盖的最低 Git 版本上再跑一次。

---

## 12. 最小结论（给作者的一句话）

请在 `git_status_files()` 里给 `git status` 加 `-- .`，并用 `git rev-parse --show-toplevel` 解析 porcelain 路径；同时给 `git_numstat` 加 `-- .`。这三处能同时解决「子目录仓库全仓扫描」和「git 徽标路径错误」，本环境的点击卡顿会从 270 万条降到 3,289 条。
