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

- **标准 debug home：`./pi-flash-dev`**（`PI_FLASH_DIR` 指向它，已进 `.gitignore`）。跑 dev 与任何测试都用它，别再临时造 home：
  `PI_FLASH_DIR="$(pwd -W)/pi-flash-dev" cargo run -p app`。它跨轮保留（设置 / 更新状态 / 缓存），所以测试实例不用重新登录。
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
