# pi-flash 进度记录

> 唯一进度台账（AGENTS.md 只留铁律与路径）。**维护规则**：
> ① 每轮工作更新「当前状态」，并在「里程碑索引」顶部加**一条一行**纪要；
> ② 实现细节不进本文件——commit message 即详录（本仓库惯例：提交信息自带
>    根因/方案/验证），检索用 `git log --oneline --grep=<关键词>`；
> ③ 用户定案沉淀进下方「产品定案」节；陷阱住 `docs/agents/`（pi-rpc / gpui /
>    engineering），随踩随补去那边。
>
> 2026-10-10 压缩：2716 行 → 现行规模。详版全文在 git 历史——压缩提交的
> 父提交是最后全量版（`git log -p -- docs/progress.md`）。旧文件「当前状态
> （2025-09，…）」两节的 2025 日期为早期 agent 幻觉，已随压缩删除——
> **项目实际始于 2026-09-24**（首次提交 80d80d2）。

## 当前状态（2026-10-13）

- workspace：`crates/pi-link`（协议层，127+3 测试）+ `crates/app`（GPUI 界面，
  ~173 测试）+ `vendor/pi`（**钉版 1.1.0**，2026-10-10）；gpui 0.2.2 vendored。
- 发版：**v0.1.0 已上线**（GitHub Releases 三资产 + npm `pi-flash@0.1.0`）；
  发版操作唯一路径 `docs/RELEASE.md`；OIDC trusted publishing 已就绪，待
  npmjs 配 Trusted Publisher 后发版不碰 npm。
- 功能已通：流式聊天/steer/中断/图片、富渲染（KaTeX/mermaid/raw HTML/表格）、
  工具卡片与会话分支（fork/clone + agent 轮锚点）、模型/思考 7 档/工具预设、
  @ 文件检索、! shell、内置终端、文件树（Zed 移植）+ 文件编辑器、git 面板、
  设置页签×6（「远程控制」页代码保留、入口已隐藏）、三语 i18n、七主题、
  pif-ui UI 自动化测试链路。
- 待用户实机验收：pi 1.1.0 升级、065 拖选手感、080 发布链真机复验。
- 主线文档：`docs/PORT_PLAN.md`（复刻计划）、`docs/模块设计/*`（分模块设计）、
  `docs/bugs.md`（bug 追踪）。beads 清空，新工作 `bd create`。

## 里程碑索引（新 → 旧；一行一条，细节看对应提交）

- 2026-10-16 **拖拽小手光标两修**（用户实机反馈）：① 样式——图形三轮定稿：
  lucide 描边图改画法两版均被否，最终用户供图实心路径 + 白填充黑描边（系
  统箭头配方，两主题都可辨）；② hover/拖拽同显小手、放下才
  显握住——Windows 平台层 `handle_cursor_changed` 只在句柄 Some/None 翻转时
  `SetCursor`，而拖拽期系统不派发 `WM_SETCURSOR`，改句柄变了即设（陷阱沉淀
  gpui.md）。

- 2026-10-16 **树定位滚动修复**：reveal_tree_row 手写正 y set_offset 违背
  gpui「向下滚为负」约定被夹回 0——长树定位永不跳转；改 Zed project_panel
  autoscroll 同款 scroll_to_item_with_offset(ix, Center, depth)，deferred
  到 paint 夹紧（陷阱沉淀 gpui.md 滚动节）。

- 2026-10-16 **文件标签驱动树跨项目同步**：激活文件 tab 时树定位选中
  （同项目 reveal_tree_row）；标签属已知其它项目则 remount_workspace 切
  工作区（不动会话，PF 与 Zed 多 worktree 的分歧定案）；入口统一挂
  activate_panel_tab，游离文件不同步。

- 2026-10-15 **消息区右键菜单**：单项「复制」= 复制当前消息划选（无划选
  时空转），样式复用输入面板右键菜单 menu_card/menu_item 套件（挂
  psp_overlays 同层 msg_menu_layer，dlg.msg_menu 存锚点，snapshot 增
  msg_menu 键供 pif-ui 断言）。vendor 补丁：TextView mouse-down 仅左键
  起选/清除——右键开菜单不重置/不清划选（浏览器同语义）。

- 2026-10-15 **消息划选一期回归修复**（用户实机反馈 4 项）：①Ctrl+C 无效
  = gpui 键盘派发**绑定先于监听器**（window.rs:3843；composer 空选区
  `ctrl-c→input::Copy` 吞键早退）→ main.rs 注册同 "Input" 深度后注册的
  `ctrl-c→CopyMessageSelection` 抢占，渲染根 on_action 分流：有消息选区
  复制消息、否则转发 input::Copy（None-context 绑定禁用——会抢终端监听
  器的键）；②文件路径 code span 不可点 → vendor `set_path_link_detector`
  钩子（detector = as_file_path → LinkMark → 应用内 tab）；③inline code
  accent 实色底 → secondary 淡底；④表头不加粗/CJK 列截断 → render_table
  表头加粗+底色、去 truncate 改自然换行；⑤表格各行列位漂移（字节长当
  fraction >100% 靠 flex 收缩碰摊）→ 钳制 [6,44] 归一化成和为 1 的分数
  全表共用（自研渲染器同坑同修）。详见 gpui.md keymap 节与 vendor
  清单；UI 验收留给用户。

- 2026-10-15 **agent 消息划选一期**（gpui-component TextView，社区方案
  discussion #1147 / PR #1169，评估见 docs/调研/TextView消息划选评估.md）：
  定稿文本块走 `TextView::markdown().selectable()`（逐行感知划选 + 高亮，
  RenderOnce 包装 AgentMarkdown 在渲染期持 window/cx 构造——list 条目闭包
  借用锁死 cx 传不进），流式块/用户消息仍走自研渲染器；id 带文本哈希防
  会话切换串 keyed state。vendor 三补丁：①`text_view.rs` 选区归属全局
  `take_last_selected_text` + on_action_copy 空选区守卫 ②`text/mod.rs`
  链接路由钩子 `set_link_handler`（本地文件 → 应用内 tab，复用
  dispatch_md_link）③inline/node 两处 open_url 改走钩子。Ctrl+C 兜底挂
  渲染根 on_key_down（TextView 从不持焦、组件自身键绑定不可达；点选区外
  自动清除天然消歧）。二期待办：代码块语言标签/复制按钮对齐 v56、流式
  帧时间与长会话内存实测。UI 验收留给用户。

- 2026-10-13 **架构整理下半场（bead pzy/4ea/o5z，发版前整理续）**：五巨石
  拆分（messages/runtime/input/terminal/markdown 全部入红线，protocol.rs
  假超标随口径修正消除）+ input_area/composer_bar 内切破 300；RPC 直发
  收口 `SessionRuntime::rpc` 门面（域外 12 处归零）；shell/types.rs 单域
  类型十枚下沉；Chat 158 直属字段重组为 8 簇子结构（pool/composer/layout/
  psp/ws/dlg/cfg/aux，跨域摸字段自此在代码里可见可查）；check_arch 口径
  改非测试行数并挂进发版 checklist；ARCHITECTURE.md §2/§3/§8 对齐实际
  （草稿双宿主镜像协议、RPC 纪律成文）。全程纯搬家零行为变更，cargo
  零警告 + app 262+4 / pi-link 133 测试全绿；UI 验收留给用户。

- 2026-10-12 **终端选区可编辑（实机反馈）**：有选区时 Backspace/Delete = 删
  选中内容、粘贴 = 用它替换选区（不再只能删行尾）。PTY 的行缓冲归 shell，
  所以按 xterm `delete-selection` 思路实现：光标左移 N 个字符落到选区首 →
  前向删除 N 次（`deletion_plan` 算键数，单测覆盖中段/词尾多给一格空白/
  只选空白/跨行/alt screen）。宽字符按「字符」而非「格子」计数（CJK 一格
  一个 DEL、一条 Left）→ 中文行也对；删完光标停在接缝处（GUI 文本框语义，
  与粘贴替换配套）。删不了的情形（选区在滚回区/跨屏幕行/全屏程序）只清选区，
  键照旧交给 shell。
- 2026-10-12 **终端选词/选行 + 拖选越界自动滚动**：双击 = 选词、三击 = 选整
  （逻辑）行，边界直接用 alacritty 的 `semantic_search_left/right` 与
  `line_search_left/right`（zed 同口径，含折行逻辑行与 CJK 宽字符）；拖动
  到终端区上下之外朝鼠标方向自动滚动（每 1 行高 1 行、单次封顶 4 行，鼠标
  事件驱动；alt screen 无回滚缓冲不滚）；drag 监听不再要求命中 bounds（否则
  拖出去就断），改成用 `sel_anchor` 判定是否在拖，并在 MouseUp / 点别处时
  清锚点（修掉「残留锚点让别处拖动改到终端选区」的隐患）。
- 2026-10-12 **终端复制粘贴补齐**（实机反馈「不能拷贝粘贴」）：上次的焦点/输入
  目标修复已覆盖「复制读到后台终端的选区、粘贴写进后台终端」这个根因；本次再
  补入口——终端右键菜单（复制/粘贴/全选，挂 psp_overlays 同层浮层）+ Windows
  习惯键（Ctrl+Insert 复制、Shift+Insert 粘贴、Ctrl+Shift+A 全选）；Ctrl+C 有
  选区即复制且**不清选区**、无选区仍是 ^C 中断；菜单开着时 ESC 先收菜单（不把
  ^[ 送进 PTY）。裁定口径承袭 xterm/pi-web：拖动选区、bracketed paste 走
  `terminal::paste_bytes`。
- 2026-10-12 **终端输入/焦点修复（实机验收后反馈）**：双终端切 tab 后键入
  打进后台终端（表现为「数字打不进去/有时无法输入」）——`active_terminal`
  只是索引缓存，切/关 tab 不同步；现在 `active_term`、激活/关 tab 三处均以
  **渲染面**（`active_panel_tab` 上的终端）校正，自动化的
  `input.focus terminal` 同口径；点终端 tab 立即交焦点（文件 tab 同走
  `pending_focus_file`），渲染帧也把焦点回收给当前显示的终端。
- 2026-10-12 **终端开合两改（用户定夺）**：① 开终端不再切 dock 到文件树
  （psp 项目菜单「在终端中打开」留在会话列表面板）；② 同项目不再按 cwd
  复用终端——每次新开一个终端 tab，重名允许、不跳回已有终端；关 tab 同步
  校正 active_terminal 下标。
- 2026-10-11 **081 应用内自动更新落地**：延迟加载阶段后台检查 GitHub
  release（timeout 30s / 重试 2 次隔 10min / 不阻塞主进程），on=后台下载
  zip→SHA-256→解压 `payload.stage-<ver>`+标记，薄壳启动器 swap（e2e 新增
  ⑩⑪ 共 32 项绿）；off=新会话页版本号 `↑` 可点击开 release 页；新版本首启
  自动开 topbar「更新日志」tab（release notes 落 `~/.pi-flash/changelog-cache/`）。
- 2026-10-10 **设置页减负**（用户定夺）：删「加载扩展」（v70.3 逃生口
  退役——默认档路径上无效，`client::spawn` 去第三参；扩展装载恒开，
  隔离归档位）与「自动保存」（恒开，行为不变）两开关 + 字段/词条。
- 2026-10-10 **032 复制反馈动画恢复**：会话区操作栏复制 → ✓ 已复制（主题色，
  图标同色）1.5s 复位（runtime `copy_flash` + `spawn_flash_clear`，用户/agent
  两栏均接，点亮期间栏保持显影）；aac8fe0 曾按当时设计稿删除。
- 2026-10-10 **修复：双击 exe 启动凭空多出「debug」项目**：进程 cwd = exe
  所在目录（target\debug）被采纳为工作区，rebuild_projects 把它钉成常驻
  空组、删了下次启动又回来；现 cwd==exe 目录时不采纳，回退上次工作区
  （`__last`，startup_restore 关也用——那只管恢复会话），再兜底主目录。
- 2026-10-11 **full+ 档 `-e` 源消重**（bead `pi-flash-v2g`）：spawn 配方里
  `npm:x` 先解析成 managed 安装路径（`~/.pi/agent/npm/node_modules/<pkg>`，
  user 根优先、项目根兜底，未安装回退 `-e npm:x`）——消除
  `~/.pi/agent/tmp/extensions` 第二份 node_modules（同一插件双份 + 版本漂移 +
  spec 变化重装）。pi 对本地目录包根 `collectPackageResources` 全套挂载，
  资源面与 `-e npm:x` 等价。实现：`pi_link::extensions::managed_npm_source_path`
  + `tools_recipe::resolve_ext_sources_at`；存量 tmp 可随手清，下次不再生成。

- 2026-10-10 **060 远程控制入口隐藏**（用户定夺）：topbar 手机钮删除 + 设置
  「远程控制」页签撤出导航；060 管线/扫码弹窗/自动化 WX_QR_OPEN 原样保留，
  重开 = 导航数组加回一行（或 git revert）。

- 2026-10-10 **vendor pi 1.0.0→1.1.0**（eeaa351）：RPC 面纯增量；22 命令名 +
  CORE_TOOLS 核对无缺口；get_commands 仍无内置命令；live 探针 488 模型同形。
- 2026-10-10 **080-2 发版收口**：v0.1.0 全链路上线；publish-npm.yml（OIDC）；
  RELEASE.md 规范入库；`[perf]` 门控/CHANGELOG 路径/pathspec 提交三坑修入铁律。
- 2026-10-10 **080 软件分发**（xnl）：npm 薄壳 + GH Releases 载荷；esbuild 裁剪
  284MB→下载 ~120MB；e2e 19/19（安装/自愈/SUMS 兜底/更新同一条命令）。
- 2026-10-10 **064/065 编辑卡顿治理**（zr8）：根因 = paint 尾无条件 notify 连坐 +
  syntect 零缓存；LRU 128（280 行块 92ms→0.017ms）+ notify 门控 + PI_FLASH_PERF
  仪器；选区改逐行 quad 去 lyon 曲面细分（对齐 Zed）。
- 2026-10-08 **062/063 大文件提速**（5hf/5z6）：已开 tab 判重提前；set_value
  专用 reset 路径 + TextWrapper 零物化（4.6MB 152→26ms）；>256KB tree-sitter
  全量 parse 后台化 + 纪元作废。
- 2026-10-07 **031 @检索 + !shell**（s49）：pi-web file-fuzzy 移植；bash RPC +
  乐观卡 + 增量渲染；get_messages 携带 bashExecution 的去重。
- 2026-10-07 **034 档位改版**：四条定稿（见定案节）；full = `-ne`+显式 `-e`
  结构性零注入；`pi_link::extensions`/`session_ext`；full+plugin 选择面板。
- 2026-10-07 **023 文件编辑展示页**：CodeEditor 底座 + topbar 标签混排 + 外部
  改动检测 + 面包屑；废弃 GitDiff 旧入口。
- 2026-10-07 **pif-ui 自动化框架**（2kq）+ v1.1 六修：快照/操作/wait/keys，
  隔离实例；误删活登记判死收紧、--arg 转义、input.focus、wait 三断言。
- 2026-10-06 **010 启动装载**：`~/.pi-flash` 自有目录 + 迁移；磁盘目录层
  `catalog.rs`（无进程冷启动可用）；startup.rs 收拢后台任务与闸门。
- 2026-10-06 **012 新会话页**（1bk）+ **004 打开项目菜单**（fresh 语义：新会话页
  选项目 = 强制新草稿，不恢复旧会话）。
- 2026-10-06 **文件树 Zed 移植**：96 图标主题 + natural_sort + gitignore 压栈 +
  vlist 虚拟化 + watcher 去抖；auto_fold 链式折叠；vlist 句柄每帧 new 修复。
- 2026-10-05 **fork 三层根因修复**：serde_json 128 层递归上限吃掉 get_tree →
  `parse_value`；pi 侧深树溢出 → `get_entries` 扁平锚点；`follow_session_file`
  统一 draft/fork/clone 跟随。
- 2026-10-05 **「新分支」改挂 agent 轮**（用户定案）+ 尾部走 clone + 分支自动
  改名（截 20 字）。
- 2026-10-04~05 **v63 会话导航**：刻度条（轮次均分切条 + 20 上限百分比桶，
  逆映射公式两版修正终定）+ 400px 悬浮摘要面板 + 虚拟化 + 总结栏。
- 2026-10-04 **v59 滚屏七修定稿**：`chat_list.rs` 收拢全部滚屏状态；钉顶垫片
  常数化（与测量精度解耦）；快流判定抖动回归锁；PAD_BOTTOM=180。
- 2026-10-04 **v61/v62 性能与字号**：LineLayoutCache 存档层 + markdown 解析
  缓存 + 图片解码缓存；字号改绝对像素差体系；StyledText 字号只认容器继承的
  根因修复（sized_text）。
- 2026-10-04 **设置改版 + v60 最近会话**：主题持久化去污染（app_settings.json，
  严禁写 pi settings.json）；Dropdown 组件；加载改时间窗口 + 每组 10 条分页。
- 2026-10-04 **思考 7 档补全**；模型目录上收 Chat 层项目级共享 + 草稿 lazy
  connect；agent 输出样式对齐 pi-web（间距/margin 折叠/strong 700/JetBrains
  Mono 打包/VS Dark+ 高亮）。
- 2026-10-03 **模型弹窗六项** + Enter 双重租约 defer 修复 + set_model 异步
  响应回填；**PI_FLASH_RPC_LOG** 线缆日志；**vendor 0.87.1→1.0.0**。
- 2026-10-02 **v56 消息渲染对齐 pi-web**（48u，30 commits）：轮结构/工作详情组/
  工具卡/diff 双栏/token 估算/compaction；**v57** KaTeX（RaTeX）/mermaid
  （mermaid-rs）/raw HTML（scraper 安全子集）；实测五修（-ne 隔离、pill_anchor、
  粘贴崩溃两连、长文本折行）。
- 2026-09-30 **v54 主界面按设计稿全量重构**（布局骨架/psp 一体列表/composer
  胶囊/消息区/设置弹窗/七主题）；收尾验收；10-01 v54.6 IME panic 修复 +
  v54.7 主题一致性。
- 2026-09-24~28 **M1-M6 PORT_PLAN 全交付**：pi-link 协议层；分支导航；文件树；
  git 面板；内置终端（alacritty 0.26 + ConPTY）；模型/Provider 面板；插件/技能/
  工具面板；扩展 UI 协议；子代理面板；主题运行时切换；i18n 三语；LLM 生成
  标题；check_arch 收官（a1bfff0/b68faca，09-27）。
- 2026-09-24 **项目起点**：GPUI smoke + pi rpc 桥接 spike（80d80d2）→ 复刻
  pi-web 策略 + 钉版 vendor 决策 + PORT_PLAN（d0c4a87）→ M1 骨架 workspace +
  pi-link（typed RPC + fixture 测试）+ vendor 0.87.1 + markdown 渲染（0b7a9c2）。

## 产品定案与已知行为（勿当 bug 修）

- **功能面板与主窗口解绑（2026-10-13）**：状态栏 dock tab 只切功能面板，
  不再联动内容区（旧「会话 tab=chat / files tab=浏览区」绑定过时拆除，
  该联动会把对话窗口顶成第一个打开的文档）；内容区由 titlebar tab 条
  独立管理（置顶会话 tab 回会话视图）。

- **终端开合**（2026-10-12 用户定夺）：开终端**不切 dock 面板**（在会话列表
  面板开终端，dock 留在原处，只有内容区切到终端 tab）；同项目**不限终端
  数量**——每次打开都新开 tab（不按 cwd 复用/聚焦跳转），tab 标题 = 目录名、
  允许重名。

- **滚屏**（pi-web useAgentSession 语义）：发言翻页 = 用户消息钉视口顶；
  「回复只到屏幕中央 + 下方空白」= 钉顶态本身（垫片是真实列表条目）；
  「一滑掉屏底、空白消失」= 滚轮退役锚点（pi-web 同款）。均非 bug。
- **fork**：分支点挂 **agent 轮**操作栏（语义 = 保留到本轮回复为止）；尾部无
  下一条用户消息时走 `clone`；分支自动改名 = 截 20 字不加后缀；用户消息栏
  只剩 复制/编辑。
- **档位四条定稿**：full 禁任何插件注入（个人扩展照常）；自定义 = full + 插件
  清单；对话中途不可改（运行中工具预设置灰）；每会话保存一份；默认 = 自定义；
  configured 档已删。
- **扩展装载**：主会话恒加载扩展（pi-web parity，v70.3 拍板）；全局开关
  已删（2026-10-10 用户定夺：默认档路径上本就无效、无自动恢复语义，
  隔离归档位系统——full/full+ 自带 `-ne`+精确集）。`client::spawn` 两参。
- **编辑器自动保存**：恒开（023 行为不变）；开关已删（2026-10-10 用户
  定夺：无配置价值）。
- **composer 控件分类**：工具预设 = spawn 参数 → 运行中禁换；思考强度/模型 =
  实时 RPC → 随时可换；fork 与压缩运行中均禁。
- **渲染规则**：用户气泡不渲染 HTML（发出内容凭证）；用户消息时间戳
  「9月17日 16:20」hover 淡入；无计费行、无「已复制」反馈；流式当轮平铺
  渲染（isLiveTail），轮末才折叠「工作详情」。
- **psp 加载**：时间窗口 7/14/30 天 + 每组 10 条「显示更多」分页（zcode 式）；
  窗口即归档——显示层隐藏，数据不删。
- **其它**：md 默认打开源码态（用户二改）；html 文件走系统浏览器（wry 内嵌
  三轮失败定案绕行）；主题持久化只写 app_settings.json；模型弹窗短名/思考
  off = 预期（非推理模型）；新会话页判据 = isEmptyNew。

