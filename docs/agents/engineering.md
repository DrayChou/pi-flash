# Rust / 工具链工程杂项

覆盖全仓（crates/**、脚本、手工流程），不专属于某个模块。随踩随补。

- **Windows PowerShell 5.1 读无 BOM 的 UTF-8 `.ps1` 会按 ANSI（本机 GBK）解**：脚本里的中文字符串会炸成
  `TerminatorExpectedAtEndOfString`（解析期就挂）。`.ps1` 要么带 UTF-8 BOM，要么内部只用 ASCII（`-File` 跑时；
  `-Command` 走另一条路，不受影响）。
- **删除须进回收站**（仓库规则）：MSYS `rm` 不进回收站；用 PowerShell 的
  `[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile/DeleteDirectory($p,'OnlyErrorDialogs','SendToRecycleBin')`
  （先 `Add-Type -AssemblyName Microsoft.VisualBasic`）。一批小文件先 `Move-Item` 打进一个临时目录再整包回收，
  回收站里只留一条记录。
- 大补丁用脚本文件，勿用 bash heredoc（会静默截断）；方法链中间插入语句后漏/多 `),` → 结构性编译错误。
- `gen` 是 edition 2024 保留字。
- python 脚本整文件重写会把 CRLF 翻成 LF：改前探 `\r`、写回保原行尾；子串替换锚点带行首 `\n` 防缩进误匹配。
- Windows 路径：`Path::components()` RootDir 保留原始分隔符（不可做路径 key）；盘符大小写双写（`d:\` vs `D:\`）跨写方真实存在 → 归一化键小写化（pi-link `path_key`）。
- 深递归 JSON 测试（deep_tree）在 Windows 默认测试栈溢出 → `RUST_MIN_STACK` ≥32MB（Quick Start 的测试命令已带）。
- UI 自动化断言交互 bug 必须走**真实按键链**（`input.keys`）：直调 setter 测不出 on_change 类缺陷（菜单 latch 泄漏即漏测案例）。
- **git-bash 的 `grep -i -F --` 会 core dump**（GNU grep 3.0 / MSYS，SIGABRT，退出码 134）：`grep -qiF -- pat` / `grep -iF -- pat` 必崩（`-qF --`、`-qi`、`-q` 都正常）。脚本里做「路径子串包含」判断不要用 grep，用 bash `case "${line,,}" in *"${pat,,}"*)`——轮询里顺带省掉反复起进程的开销（`scripts/upd-e2e.sh` 实录）。
- **改脚本/文档别把行尾换行吃了**：`edit` 的 oldText 带 `\n` 而 newText 不带，会把两行并成一行。`scripts/upd-e2e.sh` 里 `$PUI shot …` 那行曾被并进前一行 `… || bad …` 的尾部，而 `A && B || C && D` 的链式求值照样打印 ✓ ——**假绿**。测试脚本改完必须 `bash -n` + 真跑一遍数断言（本次就是靠「断言数 19/19 但 PNG 不在」才发现）。

## target/ 磁盘占用（实测 26G 膨胀的教训）

- **deps/ 与 incremental/ 会无限囤积**：同一 crate 按指纹（feature/参数不同即换 hash）囤好几份副本，cargo 只偶尔清旧的。长期开发后 `target/debug` 膨胀到 26G（deps 15G + incremental 7.4G），大头是陈旧指纹而非当前构建。定期 `cargo clean`（或直接删这两个子目录）即可，全部可再生，无损源码。
- **多会话共用同一 target/**：agent 会话、编辑器 rust-analyzer、手动 `CARGO_INCREMENTAL=1` 的构建都会往里写。清理后“又长出来”不奇怪——先查有没有活跃 cargo/rustc/rust-analyzer 进程再删，别删正在写的缓存。
  **症状**：另一会话正在跑 `cargo test` 时，本会话重新链接会以 `LNK1104: cannot open file '…\deps\pi_flash-<hash>.exe'` 失败——同名测试二进制正被对方执行中。看到 LNK1104（而非编译错误）就是撞车：等对方跑完重试，别改代码。
- **2026-10 配置决策**：`[profile.dev] incremental = false`（根 Cargo.toml），incremental/ 常驻从 ~7G 砍到 0；代价是改动所在 crate 重编译慢 20~40%，快迭代临时 `CARGO_INCREMENTAL=1`（env 覆盖 profile），用完记得会重新囤。deps/ 瘦身早有铺垫（`[profile.dev.package."*"] debug = false`），勿动 gpui/gpui-component 的 `debug = 1`（断点进 vendor 内部用）。

## 发版脚本（scripts/release.sh）踩过的坑

- **esbuild 路径会随 npm hoist 变**：断言原写死 `node_modules/@earendil-works/pi-coding-agent/node_modules/@esbuild`，新版 pi 把 `@esbuild/win32-x64` 提升到 `vendor/pi/node_modules/@esbuild` → 裁非宿主平台那步整脚失败。两处都认（先试嵌套再回落顶层）。
- **烟测「启动零输出」与更新探查鸡生蛋**：release 构建默认跑更新检查（`PI_FLASH_UPDATE_CHECK` 未设时 `!cfg!(debug_assertions)` 为真），而 GitHub Release 还没建时 `/releases/latest` 必 404 → `eprintln!("[updater] 检查更新失败…")` 把门禁打红。烟测须 `export PI_FLASH_UPDATE_CHECK=0`（门禁只验启动，不验更新）。
- **CHANGELOG 收编不能假设首行是版本段**：文件顶部的 `# 更新日志` 标题 + 说明段会被原 `tail -n +2` 吃掉，且同版本段已存在时会写出两份 → 已改成幂等 `upsert_section`（同版本段原地替换，否则插到首个 `## [` 之前，前言原样保留）。收编前还要去**段首空行**，否则 `<!-- EN -->` 前后空行会在英文段标题下多出一行。
- 烟测会 `taskkill` 全部 `pi-flash.exe`：发版前必须让用户自己优雅关掉实例（草稿/回合会丢），别替他 kill。

## npm 相关（本机环境事实）

- 本机 `~/.npmrc` 里 `//registry.npmjs.org/:_authToken` **已失效**（`/-/whoami` 返401）→ `npm unpublish/publish` 这类写操作会静默 no-op：`npm unpublish` 竟 exit 0 并打印 `- pkg@version`，但 registry 的 `versions` 毫无变化（内部 PUT `/-rev/<rev>` 返 404）。**写操作后必须回 registry 核对 versions**，别信退出码。
- 同一版本号不可重发：测试期手发过的版本，正式发版前必须先 unpublish（注意 72h 窗口），否则 `publish-npm` OIDC job 报 “cannot publish over the previously published versions”。
- **npm 版本号一经使用永不可复用**（docs.npmjs.com/policies/unpublish：“Once `package@version` has been used, you can never use it again”，unpublish 不可撤销）。测试期手发过 x.y.z → 正式发版只能换新号，别指望 unpublish 后重发同一号（packument 的 `time` 墓碑会让 PUT 持续 400）。
- **本机 github.com 是「间歇不通」，不是永久不通**（2026-10-17 复测：真 0.1.9 通过 `api.github.com` 认到 0.1.10，且从 `github.com/.../releases/download` 直连下载成功跑到 95%）。所以薄壳安装链路可在通的时候真机验；不通时才走本地镜像（`PI_FLASH_MIRROR=http://127.0.0.1:8765`，把 `dist/` 里的 zip + `.sha256` + `SHA256SUMS` 摆成 `<base>/v<版本>/` 布局用 `python -m http.server` 挂上）。

## 标准 debug home 与 0.1.9 升级测试台

- **标准 debug home：`./pi-flash-dev`**（已进 `.gitignore`）。**2026-10-18 起 paths 层焊死双 home：debug/test 构建（cargo run / cargo test）默认自动落它，release 才落 `~/.pi-flash`；`PI_FLASH_DIR` 仍可显式覆盖**。历史命令带不带前缀都等价：`cargo run -p app`。它跨轮保留（设置 / 更新状态 / 缓存），所以测试实例不用重新登录。注意：`cargo run --release` 走正式 home，想隔离请显式 `PI_FLASH_DIR`。
- **测试实例与真实 pi 的边界**：`PI_FLASH_DIR` 只搬 app 自有文件；pi 侧车仍用真实 `~/.pi/agent`（凭据/会话/设置）——测试实例产生的会话会出现在正常会话列表里，这是故意的（免登录）。
- **0.1.9 升级测试台**（人工复现升级/打断；2026-10-17 搭）：`D:\pf-test-019\`
  - `payload\` = 真 0.1.9 载荷（`dist/pi-flash-0.1.9-win32-x64.zip` 解压）；`baseline\payload\` = 母本（硬链接克隆，还原用）
  - `① 启动 0.1.9 测试实例.cmd`（`PI_FLASH_DIR` → `./pi-flash-dev`）/ `② 查看现状.cmd`（只读清单：残留条目名 + 载荷 exe 的 sha256 是否还等于干净 0.1.9）/ `③ 一键恢复干净的 0.1.9.cmd`（杀本目录进程 → 打印残留清单 → 清空 → 母本硬链接还原 + 校验）
  - 真源可用，无需假源（见上一条）
  - 打断实测（下载完成、待重启时杀进程）：留下 `payload.stage-0.1.10`（263MB）+ `update-staged.json`，`payload\` 本身没被碰（老方案 stage 阶段不动在用目录）
  - `③` 的还原是**硬删**（工作副本可反复重建，不进回收站）；母本 `baseline\` 才是真副本，别手删
  - **怎么让新代码被测到**（真源下新代码永远"已是最新"，因为自我版本 = 编译进 exe 的 0.1.10 = 真源最新发布）：临时降版窗口——`crates/app/Cargo.toml` 的 version 改 0.1.9 → `cargo build -p app` → `python tools/rig.py rebuild` → 立刻改回 + `git checkout -- Cargo.lock`。实测全程 2 分钟、`git status`/`git diff` 零残留（做不到时先确认没有别的 cargo/rustc 在跑）。改完 updater 想再测一次安装链路，就得重走一遍
  - **新代码档启动必须带 `PI_FLASH_UPDATE_CHECK=1`**（debug 构建默认不检查更新，真实发版包是 release 默认查）：`rig.py start` 已代设
  - 实测（2026-10-17，真源 0.1.10 资产 + 新代码）：下载走 `github.com/.../releases/download` → 解压 → 落位 → **`安装收尾：读过 15033/15033 个文件，301 ms`**（8 线程）→ `update.state=ready`。即"安装期把每个文件读一遍"对 15k 文件几乎不增加安装耗时
- npm ≥11.5.1 的 `allowScripts` 门控会对薄壳 postinstall 报“not yet covered by allowScripts”警告（实测安装仍会跑脚本，失败则 npm 回滚）；命中拦截时的兜底是 `pi-flash.js` 启动时 `payloadOk()` 自愈补齐。

## 启动时凭空多一个终端窗口（Windows 子系统/spawn flags）

**症状**：PF 主窗口出现前先弹一个终端窗口（Windows Terminal，tab 名 = `pi-flash`，
里面只有一行 `libpng warning: iCCP: known incorrect sRGB profile`，之后无提示符）。

**根因**：`crates/app/src/main.rs` 缺 `#![windows_subsystem = "windows"]` →
链接出的 exe 是 **console subsystem** → 进程一启动 Windows 就给它分配控制台 →
机器若以 Windows Terminal 作默认终端宿主，系统替它开一个 WT 窗口（那行 iCCP 警告
是 WT 自己加载 PNG 图标的诊断，与 PF 无关）。GUI 程序还会白挂一个
`conhost.exe 0x4` 子进程。

**判定姿势（可复用）**：

- `Get-CimInstance Win32_Process` 看进程树：谁名下挂着 `conhost.exe`（父进程即
  `AllocConsole` 的那个）；`conhost.exe --headless …` 则是内置终端的 ConPTY
  服务端（不显示窗口，正常）。
- 进程内自查：`GetConsoleProcessList` + `GetConsoleWindow`，`attached=true` 即
  本进程已在控制台里（临时 extern "system" 打两个点，诊断完删）。
- 只想验证 gpui 是否有责：写个 `#![windows_subsystem = "windows"]` 的最小 bin
  链 gpui 开空窗——**实测 gpui 无罪**（无 conhost 子进程），病根在 app 自身。

**铁律**：

1. `main.rs` 的 `#![windows_subsystem = "windows"]` **勿删**（文件头注释已写缘由）。
2. **任何** `std::process::Command` spawn Windows 子进程都要带
   `CREATE_NO_WINDOW`（0x0800_0000）。GUI 宿主 spawn console 程序（git /
   powershell / node）时，子进程默认新建控制台 → 又一个 WT 窗口。已带 flag 的：
   `pi-link/{client,vendor,registry}.rs`、`services/token_probe.rs`；
   本轮补上：`services/git.rs`（新增 `git_cmd()` 统一口径）、`services/file_index.rs`。
   **测试代码也不例外**：updater.rs 测试里的 powershell（Compress-Archive 造
   zip 夹具）漏 flag，每跑一轮 `cargo test -p app updater::` 就弹一个带进度条
   的 PS 窗抢屏幕（2026-10-18 实锤，agent 代跑测试时尤甚）——已加
   `hidden_powershell()` 帮手统一口径。**并发 spawn 也不例外**：测试里裸
   `Command::new("git")` 多线程同时 spawn 会撞 conhost 分配竞态——code=-1073741502
   （0xC0000142 STATUS_DLL_INIT_FAILED）、stderr 为空、单跑又必过（2026-10-19
   status_tests 实锤）——测试也要走 `git_cmd()`，别自建 Command。
3. 读 PE Subsystem 别手撸偏移——本地 python 按 `pe+24+68` 读 debug exe 得出 2、
   release 得出 3，与实测行为矛盾；**以「进程树里有没有 conhost / 探针
   attached」为准**，别信自己算的字段。

**实测（debug exe，隔离 home，启动后 15s 采样）**：修复前 PF 名下有
`conhost.exe 0x4` + `GetConsoleProcessList` 在 `main` 第一行就 `attached=true`；
修复后同一探针 `attached=false`、进程树只剩 `node.exe`（pi 侧车）与 `git.exe`
（都 CREATE_NO_WINDOW），无 conhost、无 WT 新进程。

## git 子进程语义三坑：pathspec / porcelain 路径基准 / -z rename（2026-10-19 实锤）

外部用户实测报告（`docs/pi-flash-file-tree-freeze-20261019/`，含复现脚本与
微基准）钉死，`services/git.rs` 的坑，全是「cwd==repo_root 时看不出问题」型：

1. **`git status` 无 pathspec = 扫整仓**：cwd 是仓库子目录时照扫仓库根，
   `-uall` 还递归展开全部未跟踪目录——家目录遗留仓库 + Windows junction
   实测 270 万条 / 414MB / 99.9s。PF 把结果全存 `ws.git_files`，点文件夹时
   UI 线程 `flatten()` 全量重建 HashMap → 冻死数秒。status/numstat 一律带
   `-- .` 钉在项目内（同环境 0.095s / 3289 条）。
2. **porcelain 输出是仓库根相对路径，不是 cwd 相对路径**：`cwd.join()` 在
   cwd==repo_root 时碰巧对，子目录项目全拼成假路径——徽标永远不亮、
   `changed_ancestor_dirs` 造出几十万个假祖先目录。修法：`git rev-parse
   --show-toplevel` 定根，重定基到 cwd（与 read_dir 同形态，git_map 才命中）。
3. **`-z` 下 rename/copy 是两条 NUL 记录**（`R  new\0old\0`），没有 TAB 可切；
   按 TAB 解析永远不命中，old path 还会被当成独立条目。修法：X/Y 为 R/C 时
   消费紧跟的下一条记录。

## 格式化：只走 `scripts/fmt.sh`（2026-10-18 三次踩踏后立的规矩）

### 为什么不能跑 `cargo fmt`

`cargo fmt` 的语义是**洗整个 package**，不是「你改的那个文件」：

- 本仓 200+ `.rs` **绝大多数不是 rustfmt-clean**（原作者紧凑手写风：长行、
  紧凑字面量）。跑一次 → 几百个文件立刻变脏，纯排版 diff。
- 唯一命令 `rustfmt --edition 2024 <file>` 只改排版（换行/缩进/`use` 排序），
  **不改语义**；编译器、测试、reviewer 都不看排版 ⇒ 本仓里整包格式化
  **零收益、纯风险**。
- `cargo fmt -- <file>` 里的 `--` 是**给 rustfmt 追加参数**，主体照样整包
  （第三次踩踏就栽在这：以为带文件参数就收窄了范围）。

真正需要全仓格式化（统一风格）时，正确做法是**一次性专项**：`fmt.sh --all`
→ 单独一个 `style: 全仓 rustfmt` 提交 → 同时把 `cargo fmt --check` 挂进 CI
从此保持干净。绝不能夹在功能提交里顺手做。

### 附带雷：`i18n.rs` 的 TABLE 被 test 按行解析

`i18n_audit::keys_and_translations_are_complete` 按行解析 `i18n.rs` 的
TABLE（行 trim 后以 `(` 开头 + 拆出 4 列），表行**必须一行一条**。rustfmt
会把超宽表行折成多行 → audit 只解析到零头（实测 552 → 233）直接 panic
「TABLE 解析异常」。所以 `fmt.sh` **永远跳过** `crates/app/src/i18n.rs`。

误伤恢复：`git checkout HEAD -- crates/app/src/i18n.rs` 再重插新增表行
（表行统一 `, ` 分隔单行格式）。

### 附带雷：点名 `mod.rs` = 洗整个模块树（2026-10-19 实测）

rustfmt 递归解析 `mod` 声明：`rustfmt mod.rs` 会把**该目录全部子模块**
一起格式化。点名 `markdown/mod.rs` + `services/mod.rs` 两个文件，实际
脏了 12 → 33 个文件（markdown/\*、services/\* 全军覆没）。守则：改的是
`mod.rs` 本身且想只洗它，要么接受树洗、要么先 `--check` 看波及面、
要么事后 `git checkout HEAD -- <无辜文件>` 回滚。哨兵（涨 ≥20 报警）
就是为这一刻准备的——报警即核对 `git status`，别直接提交。

### 三层防线

| 层 | 位置 | 拦什么 |
|---|---|---|
| 入口防 | `scripts/fmt.sh` | **批量行为本身**：无参数报错（不存在「默认整包」默认值）、拒目录/包名/非 `.rs`/未跟踪文件、跳过 i18n.rs、跑完打印脏文件计数哨兵（一次涨 ≥20 提示疑似洪泛） |
| 提交防 | `.githooks/pre-commit`（`core.hooksPath=.githooks`） | **大批量入库**：单次暂存 ≥30 文件即拦（逃生 `--no-verify` 或 `PI_FLASH_ALLOW_BULK=1`） |
| 救火 | `scripts/fmt-flood-clean.sh` | 把工作区 `.rs` 洪泛封进 stash（可恢复），非 `.rs` 改动留在工作区 |

```bash
scripts/fmt.sh <自己改过的那几个 .rs>   # 唯一安全姿势
scripts/fmt.sh --check <file.rs>       # 只检查不写盘
scripts/fmt.sh --all                   # 全仓专项：已跟踪改动须为 0 + 输 yes
```

**为什么写死成脚本**：否定式规则（「禁止整包 fmt」）对 agent 约束力不足——
本仓连踩三次，第三次还是在 `i18n_audit` 打脸之后。把危险动作收敛成唯一
受控入口，让「整包」这个操作在命令行上根本不可达，比记住规则可靠。
