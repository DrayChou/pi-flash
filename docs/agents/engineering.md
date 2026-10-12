# Rust / 工具链工程杂项

覆盖全仓（crates/**、脚本、手工流程），不专属于某个模块。随踩随补。

- 大补丁用脚本文件，勿用 bash heredoc（会静默截断）；方法链中间插入语句后漏/多 `),` → 结构性编译错误。
- `gen` 是 edition 2024 保留字。
- python 脚本整文件重写会把 CRLF 翻成 LF：改前探 `\r`、写回保原行尾；子串替换锚点带行首 `\n` 防缩进误匹配。
- Windows 路径：`Path::components()` RootDir 保留原始分隔符（不可做路径 key）；盘符大小写双写（`d:\` vs `D:\`）跨写方真实存在 → 归一化键小写化（pi-link `path_key`）。
- 深递归 JSON 测试（deep_tree）在 Windows 默认测试栈溢出 → `RUST_MIN_STACK` ≥32MB（Quick Start 的测试命令已带）。
- UI 自动化断言交互 bug 必须走**真实按键链**（`input.keys`）：直调 setter 测不出 on_change 类缺陷（菜单 latch 泄漏即漏测案例）。

## 发版脚本（scripts/release.sh）踩过的坑

- **esbuild 路径会随 npm hoist 变**：断言原写死 `node_modules/@earendil-works/pi-coding-agent/node_modules/@esbuild`，新版 pi 把 `@esbuild/win32-x64` 提升到 `vendor/pi/node_modules/@esbuild` → 裁非宿主平台那步整脚失败。两处都认（先试嵌套再回落顶层）。
- **烟测「启动零输出」与更新探查鸡生蛋**：release 构建默认跑更新检查（`PI_FLASH_UPDATE_CHECK` 未设时 `!cfg!(debug_assertions)` 为真），而 GitHub Release 还没建时 `/releases/latest` 必 404 → `eprintln!("[updater] 检查更新失败…")` 把门禁打红。烟测须 `export PI_FLASH_UPDATE_CHECK=0`（门禁只验启动，不验更新）。
- **CHANGELOG 收编不能假设首行是版本段**：文件顶部的 `# 更新日志` 标题 + 说明段会被原 `tail -n +2` 吃掉，且同版本段已存在时会写出两份 → 已改成幂等 `upsert_section`（同版本段原地替换，否则插到首个 `## [` 之前，前言原样保留）。收编前还要去**段首空行**，否则 `<!-- EN -->` 前后空行会在英文段标题下多出一行。
- 烟测会 `taskkill` 全部 `pi-flash.exe`：发版前必须让用户自己优雅关掉实例（草稿/回合会丢），别替他 kill。

## npm 相关（本机环境事实）

- 本机 `~/.npmrc` 里 `//registry.npmjs.org/:_authToken` **已失效**（`/-/whoami` 返401）→ `npm unpublish/publish` 这类写操作会静默 no-op：`npm unpublish` 竟 exit 0 并打印 `- pkg@version`，但 registry 的 `versions` 毫无变化（内部 PUT `/-rev/<rev>` 返 404）。**写操作后必须回 registry 核对 versions**，别信退出码。
- 同一版本号不可重发：测试期手发过的版本，正式发版前必须先 unpublish（注意 72h 窗口），否则 `publish-npm` OIDC job 报 “cannot publish over the previously published versions”。
