## 待验证（本轮修复）——切项目后点项目行「＋ 新建会话」直接崩（进程消失）

用户口径：从文件树切项目到 agentic-design-pattern → 打开一个 md → 点 pi-flash
项目标签栏的新建会话，app 直接崩溃（无报错框，进程瞬间没了）。

- ✅ 定位手法（值得复用）：release 崩得没日志，先看 Windows 事件日志
  `Application Error` → 异常码 `0xc0000409`、异常数据 `7` = Rust panic
  撞上不可 unwind 边界（gpui 鼠标事件回调）后的 fastfail，**不是**栈溢出
  （那是 `0xc00000fd`）；转储在 `%LOCALAPPDATA%\CrashDumps\pi-flash.exe.*.dmp`，
  `payload-x.y.z\pi-flash.exe` 与本地 `target\release\pi-flash.exe`
  **字节一致**（md5 相同）且带 `pi_flash.pdb` → 自写 `pdbtool`（`pdb` crate 读
  全局符号表）把转储栈上的 RVA 翻成函数名，直接看到
  `new_session_in` → `switch_project` → `rebuild_tree` → `flatten_pins`；
  再照 panic 现场留在栈上的字符串字面量（`called \`Result::unwrap()\` on an
  \`Err\` value`）用 debug 构建复现，拿准行号 `services/file_tree.rs:116`。
- ✅ 根因两层：
  1. **触发面（真凶）**：`switch_project` 自己抄了一遍换 cwd 的重挂流程，
     漏掉 **Pin 收藏夹**——那是按项目存的（`pins_for(ws_key)`，只有
     `remount_workspace` 刷）。切项目后 `ws.pinned` 还是上一个项目的绝对
     路径；Pin 视图下 `rebuild_tree` → `flatten_pins(new_cwd, 旧 pins)` →
     `IgnoreStack::is_abs_path_ignored` → `abs_path.strip_prefix(abs_base_path)
     .unwrap()` 当场 panic；panic 发生在鼠标事件回调（非 unwind 边界）→
     `panic_cannot_unwind` → 进程直接 abort。只 pin 过文件、没切过项目的人
     碰不到，所以显得「偶发」。
  2. **炸点本身**：`is_abs_path_ignored` 的 `Some` 分支比 `RepoExclude`
     分支少一句归属检查——后者有 `!abs_path.starts_with(ignore.path()) →
     交上一层`，前者直接 `strip_prefix().unwrap()`。任何「规则来自不包含
     本路径的 base」都会炸（Pin 脏数据、目录被移走的旧条目都能触发）。
- ✅ 修法：① `switch_project` 不再自抄，改调 `remount_workspace`（与
  `open_session` 池命中路径同一条），Pin 清单/退出空 Pin 视图自动跟着换；
  ② `is_abs_path_ignored` 的 `Some` 分支补齐归属检查，跨 base 路径交上一层
  （语义与 `RepoExclude` 分支一致，本项目内的命中照旧置灰）。
- ✅ 验证：新测 `flatten_pins_survives_out_of_project_pin`（外项目绝对路径
  进 Pin + 本项目命中仍置灰两向都锁）；`cargo test -p app` 296 全绿、
  `cargo test -p pi-link` 142 全绿。pif-ui 隔离实例走原始序列复验：
  agentic 里 pin 文件 → 开 Pin 视图 → 切到 pi-flash → `files` 面断言
  `pin_view=false` 且 `pins=[]`；原崩溃序列（切项目 → 开 md → `project.
  new_session_in` / psp ＋）跑通，app.log 零 panic；防御层单跑（留在
  pi-flash 却 pin 了 agentic 文件再开 Pin 视图）出两行不炸。

## 待验证（本轮修复）——每次启动都从 payload 目录启动（未注册的安装目录常驻项目列表）

用户口径：每次启动 pi-flash 都落在 `...node_modules\pi-flash\payload`，这个目录从没
注册过，从项目列表删掉下次启动又回来。

- ✅ 根因（三层叠加，单修哪层都复发）：
  1. **交接 spawn 不设 cwd**（`updater::handoff_if_newer`）：双击老入口
     `payload\pi-flash.exe` → 交接拉起新 exe `payload-0.1.12\pi-flash.exe`，
     子进程继承 cwd = 老 payload 目录。
  2. **exe 目录探针失灵**（`shell/chat.rs`）：探针只认「cwd == exe 目录」；
     新 exe 目录已是 `payload-0.1.12` ≠ 继承的 `payload` → 双击兑底没命中，
     老 payload 目录被当用户项目采纳：`register_project` 入册 + 经
     `set_last_open` 写脏 `__last` → 此后每次启动都恢复成 payload，自我延续。
  3. **存量不洗**：`projects.rs::purge_app_paths` 写了没人调——注册表里的
     payload 脏条目删了下次又写回。
- ✅ 修法（四处合围）：① 交接 spawn 在「cwd == 自己 exe 目录」（双击签名）
  时显式给子进程 `current_dir` = 子 exe 自己的载荷目录，让它同样命中 exe
  目录兑底；终端/薄壳场景 cwd 是用户语义，保持继承。② 新纯函数
  `workspace::cwd_inside_install_root`：exe 目录名为 `payload`/`payload-<版`
  `本>`（npm 薄壳布局）时，cwd 落在安装根（包目录）内一律按 exe 目录启动
  处理，不采纳为项目（堵快捷方式「起始位置」= 包根、手动 cd 进载荷目录
  等残余路径）；`PI_FLASH_CWD` 显式指定不参与放宽。③ `startup::boot` 第
  一步调 `purge_app_paths()` 洗注册表存量（幂等）。④ 写入口堵死：
  `set_last_open`/`set_last_workspace` 拒记安装根内路径，`rebuild_projects`
  不给安装根内 cwd 建常驻项目组，`register_project` 护栏拦新增。
- ✅ 验证：`cargo test -p app` 295 全绿、`cargo test -p pi-link` 142 全绿
  （含新测 `cwd_inside_install_root_matches_payload_layout_only`、
  `install_root_covers_sibling_payload_dirs`）。真机待复验：双击老
  `payload\pi-flash.exe` → 交接后落上次工作区而非 payload；项目列表无
  payload 条目。
- ✅ 顺带：`cleanup_removes_only_older_payloads` 陈旧测试修复——版本写死
  `0.1.11` 当「更新的」，crate 版本涨过 0.1.11 后必挂；改为相对
  `current_version()` 推导（older = 0.0.9，newer = `9<cur>`）。

## 待验证（本轮修复）——用户消息鼠标穿透（滚轮同帧连动会话列表）

用户口径：会话里用户消息会鼠标穿透。

- ✅ 根因：v63-3 为修操作栏悬停显影删掉了用户气泡的 `occlude()`（它挡祖
  先 hover，必须删）；但 gpui 的滚轮派发给**所有命中 hitbox（含祖先）**
  （buglist 设置页「滚列表页面同帧跟滚」同源），长用户气泡（>300px 内滚）
  上滚轮时气泡与外层会话列表**同帧一起滚**——感知即「滚用户消息穿透滚
  列表」。左键点击无穿透通道（祖先链上无可感知的左键 handler；TextView
  起选有 is_hovered 守卫）。
- ✅ 修法（`turn.rs` ububble 挂 `on_scroll_wheel` 边界拦截）：滚动前已在
  对应边界才放行给外层列表（原生嵌套滚动语义：内滚到底，下一滚才带动外
  层）；未溢出（max_offset=0）恒放行。两个 gpui 语义坑：① 滚动 offset
  存储域为 **[-max, 0]**（0=顶）；② 同 div 内置滚动 handler 注册晚于自
  定义 listener，bubble 相位逆序派发 → listener 先收、内置后滚，
  `offset()` 里已含本次 delta，判边界须减回去取滚动前值（EPS 1px 容差）。
- ✅ 验证：`cargo test -p app` 293 全绿；真机待复验：滚长用户气泡，气泡
  独滚、列表不动；气泡滚到顶/底后再滚一档，列表接续。

## 待验证（本轮修复）——editor 点击可见行，该行瞬移屏幕底部（滚动位被拽回内容底）

用户口径：把文件最后一行滚到屏幕中间（下方留空），鼠标点击该行，该行跳到
屏幕底部。（021 md 源码编辑器实测；任何 code editor 场景皆然）

- ✅ 根因（vendor gpui-component `input/state.rs`，两个钳制范围不一致 +
  scroll_to 无条件落账）：
  1. 滚轮路径 `update_scroll_offset` 钳的是 `scroll_size`（内容高 +
     `empty_bottom_height` 半视口垫底）——允许 overscroll，末行能合法停进
     视口中部（滚动位比纯内容底更深）。
  2. 点击 → `move_to` → `scroll_to`：光标行明明完整可见（上下两支越界判据
     都不命中），但函数尾部**无条件**把 offset 重钳进纯内容范围
     `[-(wrapper.len()×行高-H), 0]` 并写 `deferred_scroll_offset`——停在
     垫底区的滚动位被拽回内容底（视口上收半视口-行高），点击行瞬移屏幕
     底部。
  3. 2026-10-15 鬼畜振荡修复笔记里「滚轮含垫底与本处纯内容钳制并存不冲突」
     的判断不成立——冲突就出在「光标可见也落账」这一步。
- ✅ 修法：内容钳制收进上下两支越界分支（只作用于「解」）；函数尾改为
  `scroll_offset != scroll_handle.offset()` 才写 deferred + notify——光标
  可见时滚动位一字节不动，用户停好的 overscroll 位保留。分支解的钳制行为
  与 10-15 修复完全一致（解不越内容底、不裁顶），composer 振荡修复不受
  影响。

## 待验证（本轮修复）——editor 打开 minified js 卡到死（单行 94 万字符）

用户口径：打开 `素材-2026柴油裂解价差-echarts.min.js`（1,030,855 字节、
45 行、末行 **941,277 字符**、次长行 86,887 字符）编辑器假死。（该文件
就是 echarts 库本体，minified——「引用外部 echarts」与本 bug 无关。）

- ✅ 根因（vendor gpui-component，两条都在**每帧**路径上）：
  1. 高亮：`highlight_lines` 按**逻辑行**取整行字节范围送
     `SyntaxHighlighter::styles`——94 万字符行 `match_styles` 产出 ~20 万条
     capture，再进 `unique_styles` 的 O(区间×样式) 二重扫（每帧 ~10^10 次
     比较）→ 分钟级/帧 = 假死；86,887 字符的第 35 行首屏就落在
     visible_range 里，打开即 ~10^8/帧 的卡。
  2. shaping：`layout_lines` 对可见逻辑行的**全部软换行段**逐段
     `shape_line` + 整段可见行物化成一个 String——94 万字符行 ~6000 段
     /941KB 每帧重付。
- ✅ 修法（**视口字节窗**，对齐 2026-10-12 注入高亮修复的思路）：
  prepaint 按行高几何定位每条软换行段的 y，算出与视口相交的字节窗
  （上下各留 2 行余量，方向键/选区端点取 x 用）；`highlight_lines` 逐行
  查询范围先与窗求交；`layout_lines` 窗外段垫等 len 的空 `ShapedLine`
  （段数/字节 len/y 槽位全保留，光标/点击/选区定位按 len 累加，x 取 0
  只发生在屏外行）；`closest_index_for_*`/`index_for_position` 字节累加
  改用覆写后的 `len()`；`LineLayout::paint` 跳空 text 段。
- ✅ 首轮修复的回归（用户实测抓到）：开 js 不卡了，接着开含中文注释的
  py 文件崩（`direct_write.rs shape_text: byte index 67 is not a char
  boundary`，0xc0000409）。根因：视口窗裁剪后 `highlight_lines` 返回的
  样式区间不再**从 visible 文本 0 起连续平铺**，而消费端
  （layout_lines → runs_for_range → shape_text）按顺序吃 `run.len`、
  不读 `range.start`——缺口让所有 run 边界相对文本整体错位；纯 ASCII
  的 min.js 错位不炸（高亮悄悄错位，肉眼难察），py 的中文字符被从字节
  中间切开直接 panic。修法：窗前/窗后各补一段默认样式 filler，恢复
  平铺不变式（`combine_highlights` 是边界扫描线、输入有缺口输出就丢段，
  filler 必须自己补）。此修复同时纠正了滚动后高亮整体错位的暗病。
- ✅ 顺带修一个被掩住的 panic：`calculate_visible_range` 非等高路径
  `visible_range` 初始值用了 `text_wrapper.len()`（**软换行行数**），
  与循环赋值的逻辑行号混用——滚到文件底部 break 不触发时 end 跑到几千，
  旧代码靠 `slice_lines` 越界钳制掩住，改逐行迭代后 pagedown 到深处直接
  panic（`line should exists in text_wrapper`）。已钳到 `lines.len()`。
- ✅ 验证（pif-ui 隔离实例实测，debug 构建）：打开后稳态 `app.info`
  45-70ms（修复前分钟级假死）；pagedown×45 到文件底 + pageup 折返，
  94 万字符行整屏软换行 + 高亮渲染正常；大行内方向键移动正常；普通
  rust 文件无回归；`cargo test -p app`(4) + `cargo test -p pi-link`(134+3)
  全绿。回归场景复测（filler 补丁后）：gen.py（182 行含中文注释）开文件
  /pagedown×12/pageup×12/方向键/连续开 js+html+md+csv+py 六文件 全程
  存活，CJK 注释与 Python 高亮渲染正确；js 深处高亮锚定正确。
  已知边界：在超长行内打字仍整行 rewrap（~0.1-0.5s/键），打开
  （全量 wrap + 首次 tree-sitter parse）一次 ~1-2s（debug 口径）。

## 待验证（本轮修复）——外部改动不同步：cwd 外文件根本没挂监听

用户口径：在 Obsidian 里改了文件，pf 编辑器窗口不同步（预览/源码都不变）。
要求变化检测对齐 zed。

- ✅ 根因：`attach_fs_watch` 只递归 watch `self.cwd`——**cwd 外打开的文件
  （Obsidian 库、pick_open_files、远程 FILE_OPEN）从未被监听**，notify 没
  事件 → `check_external_file_changes`（disk_sig 复查 + 自动重载/标冲突）
  一次都不会被触发。实测：开工作区外文件后外部写入，`files.watch_roots`
  为空、`ext_probe.runs` 不涨、`content_len/editor_len` 不变（完全无感）；
  同一动作在 cwd 内文件上正常（hits=1，自动重载）。
- ✅ 修法（对齐 Zed 的监听面）：Zed 对不在任何 worktree 里的文件建
  **single-file worktree**（只监听该文件所在目录）；这里等价实现：
  `services::watcher::watch_dir`（非递归）+ `Chat::retain_file_watches`——
  为每个「不在 cwd 下」的打开文件按父目录挂一条监听（共享同一条 fs 信号
  通道），打开/关闭/切项目后重算，不再被需要的目录卸掉（句柄 drop = 停
  监听）。目录级信号不区分具体文件（事件类型/原子替换/临时文件都不必猜），
  统一交给 `check_external_file_changes` 按 (mtime,len) 复查。
- ✅ 顺带对齐 zed 的两个「重载语义」缺口（同一个用户场景直接撞上）：
  1. `set_value` 把光标扔到文末 + 滚动归零，外部每存一次就跳一次屏；
     新增 `InputState::reload_value`（vendored gpui-component）：整文换新但
     保留光标字节偏移与滚动像素（越界钳制）——Zed 的外部 reload 靠 diff
     保留 anchor，这里是位置近似的轻量版。
  2. markdown 预览不跳顶且读的是**缓冲区**：块数变化（外部改动/编辑动了
     段落结构）时先把 `list.logical_scroll_top()` 锚点记下、`reset` 后
     `scroll_to` 还原；有未保存修改时预览取编辑器文本而非磁盘真值（Zed
     的 preview 渲染 buffer，不是文件）——冲突挂着时预览不再显示磁盘旧文。
- ✅ 验证（`cargo test -p app` 181 全绿 + pif-ui 事务式，全在隔离实例里
  对**工作区外**文件跑）：
  - 开文件 → `files.watch_roots` = 其所在目录；外部写入 → `ext_probe.hits`
    1、`content_len/editor_len` 同步跟上（截图 `tmp/ui-w3/synced.png`）。
  - 缓冲区脏 + 外部写入 → `conflict = "Changed"`（本地缓冲保留，banner
    两个按钮可见，截图 `tmp/ui-watch2/conflict.png`）；`file.reload`（新
    op，与横幅「重新加载」同一方法）→ conflict 清、内容 = 磁盘
    （截图 `tmp/ui-w4/after-reload.png`）。
  - 预览态 + 未保存修改：预览显示本地缓冲区文本
    （`tmp/ui-w4/preview-live-buffer.png`）。
  - 外部删文件 → `conflict = "Deleted"`；文件重新出现 → 自动重载回同步。
- ⏳ 真机待复验：Obsidian 里改文件，pf 预览/源码应自动跟随（源码态光标与
  阅读滚动位置保留）；正在输入（脏缓冲）时改文件 → 顶部横幅提示，点
  「重新加载」变磁盘版、点「保留我的版本」继续用本地版。

## 待验证（本轮修复）——023 fileView：面包屑 `D:\` 拆两段 + 预览/编辑器都不自动回行

用户口径（编辑器窗口截图）：①面包屑把 `D:\` 拆成 `D:` 与 `\` 两段；②不论
markdown 预览还是源码编辑，长行都不自动回行（右缘溢出被裁）。参考 zed 修。

- ✅ 根因 ①（面包屑）：`file_nav_bar` 用 `Path::components()` 逐组件出 crumb，
  Windows 上 `Prefix("D:")` 与 `RootDir` 是**两个**组件 → 两个 crumb。且分段累积
  路径 `acc` 固定从 `chat.cwd` 起 join，工作区外文件（截图里 `D:\my_obsidian\…`
  ≠ cwd）每段都拼成 `cwd\D:\…`，**面包屑目录段的兄弟文件菜单指向错目录**。
- ✅ 根因 ②（预览不回行）：`render_doc_item_inner` 的内容段是 flex 项
  `flex_span(92., None)`（basis 0 + grow 92 + shrink 0，min_w 未设）。flex 项的
  **自动最小尺寸 = min-content**，而 gpui 文本在 `AvailableSpace::MinContent` 下
  `wrap_width = None`（`elements/text.rs` 实测路径）→ min-content = **整段单行宽**，
  于是 92% 段被撑到整段宽、正文永远拿不到可用宽（一行到底）。
- ✅ 根因 ③（编辑器不回行）：`ensure_file_editor` 显式 `.soft_wrap(false)`（当初为
  vendored gpui-component 的「等高行」快路径：行 y = row×行高 闭式解）。
- ✅ 修法（对齐 zed：预览块 `min_w_0`、编辑器 soft wrap 可开）：
  1. 面包屑分段抽成 `content::breadcrumb_segments(cwd, path) -> Vec<(文本, 段绝对路径)>`：
     前缀组件与紧随的 `RootDir` **合成一段**（`D:\` / `\\server\share\`），累积路径
     从空 PathBuf 起按组件真累积（相对 cwd 时起点仍是 cwd）；渲染与自动化快照共用。
  2. 内容段补 `.min_w(px(0.))` → 段宽 = 字面 92/100 容器宽，StyledText 得 definite
     宽自然折行（zed markdown 里同样到处 `min_w_0()`）。
  3. 文件编辑器 `.soft_wrap(true)`：vendored text_wrapper 的折行是**增量**的（只重算
     改动行范围），未折行时 `is_uniform()` 仍走等高快路径，折行后走线性可见区间。
- ✅ 验证（`cargo test -p app`，179 全绿）+ pif-ui 事务式对拍（隔离实例，直接开
  截图那个工作区外文件）：
  - `snapshot files --only crumbs` = `["D:\\", "my_obsidian", "gitee_vault", …]`
    （修复前是 `["D:", "\\", "my_obsidian", …]`）；新增 3 条单测锁分段与累积路径
    （相对/盘符+根合并/POSIX 根）。
  - `exec file.view_mode --arg mode=source|preview` + `shot`：源码态折行、预览态
    段落折行都可见（`tmp/ui-test-bc/source2.png`、`tmp/ui-test-bc/preview.png`）。
  - 自动化口：files 面新增 `crumbs`（渲染同一个 `breadcrumb_segments`，防两边漂移）。
- ⏳ 真机待复验：打开工作区外文件看面包屑是 `D:\ › my_obsidian › …`（点目录段
  列出的同级文件正确）；md 预览与源码态长行都回行、折行后光标/Ctrl+F 命中正常。
- 📌 取舍：编辑器**全部语言**开 soft wrap（不只 md）——用户口径「预览和编辑状态
  都要回行」；超长单行文件（如压缩过的 js）折行会走线性可见区间路径，比之前的
  等高快路径慢一点，换来的是不回行就不成立的可读性。

## 待验证（本轮修复）——新会话页「模型 ∨」弹窗列表空（no models match）

用户口径：第一次进（启动那个项目）「模型 ∨」有列表；**换一个目录**（切项目后落到
新会话页）再点「模型 ∨」，列表空了；而按钮上明明写着默认模型名。

- ✅ 根因：`Chat::filtered_models`（`actions_rename.rs`，v57 加 ↑↓/Enter 导航时
  抽出来的渲染+键盘同源函数）**直读 `models_by_cwd`，`.unwrap_or(&[])` 没有回落**。
  而 010-启动 那轮把目录口径改成「进程答案优先、无则回落到启动装载的全局清单
  （磁盘 `models.json` + `models-store.json` ∪ 自有缓存 `catalog-cache.json`）」，
  新的共享入口是 `Chat::catalog_for`——设置页、胶囊标签、`new_session_default`、
  微信 `/模型` 菜单**全都改走了它，只有这个弹窗漏了**（`catalog_for` 与
  `filtered_models` 是两次提交：1cc7309 在前、773e423 在后）。
- ✅ 触发面 = **该 cwd 没有活进程答过 `get_available_models`**。实测两条路：
  (a) 启动那个 runtime 由 `startup::spawn_initial_attach` **首帧即拉进程**（所以
  「第一次进有列表」）；(b) 切项目（`switch_project` → `new_session`）建的是
  **惰性无进程草稿**，而 `ensure_models_requested` 对无进程草稿只借「同 cwd 的活
  runtime」——新目录一个都没有，于是什么都不发（设计如此：选模型不许凭空拉进程）
  → `models_by_cwd` 无此 cwd 条目 → 列表空。胶囊标签走 `catalog_for` 有回落，所以
  出现「按钮有名、列表全空」的错位。
- ✅ 修法：`filtered_models` 改走 `self.catalog_for(&cwd)`（cwd 取活跃 runtime 的
  cwd），与其余入口同源；白名单/过滤逻辑不动（`compute_state` 对空 enabled 已有
  「全放行」回落，不是本轮原因）。
- ✅ 验证（pif-ui 事务式，两种构建同一条序列对拍）：隔离实例 →
  `project.switch` 到一个空目录 → `snapshot session.has_process == false`（复现态）
  → `exec model.picker_open` → `snapshot dialogs.model_rows`：
  **根因版 0（列表空）/ 修复版 5**（本机 `enabledModels` 白名单命中 5 条）。
  截图核对：弹窗列出 deepseek/DeepSeek Flash、glm/GLM 5.3 ×2、freeflow ×2。
- 📌 顺带补断言口（此前这个弹窗**没有任何自动化入口**，所以漏测）：
  `model.picker_open`（新 op，与「模型 ∨」点击路径同构：先 `ensure_models_requested`
  再开窗）+ dialogs 面新增 `model_rows`（弹窗可见行数）。复验命令：
  `pif-ui exec model.picker_open && pif-ui snapshot dialogs --only model_rows`。
- ⏳ 真机待复验：换目录后点「模型 ∨」列表有货、能选、选完胶囊跟着变；再切回原目录
  仍是进程答案那份目录（不能被全局清单盖掉）。

## 待验证（本轮修复）——文件树 / 设置·可用模型列表「滚不动」

用户口径：左边文件树与设置·模型·「可用模型」两处鼠标滚轮毫无反应（截图里模型
列表停在 Claude Fable 5 起、第一个开关右侧露出半截滚动条 thumb）。

- ✅ 根因：`ui::vlist` **每帧 `UniformListScrollHandle::new()`**。gpui 对这个
  句柄的契约是「存在视图里、每帧传给 uniform_list」（`uniform_list.rs` 里
  `UniformListScrollHandle` 的文档原话）——滚动位就挂在该句柄内部的
  `Rc<RefCell<Point>>`（`base_handle.offset`）上，由 div interactivity 的滚轮
  监听器写入、uniform_list 每帧 prepaint 再从同一句柄读回来做可见范围计算。
  每帧新建 = 滚轮写进「上一帧那个已经被扔掉的句柄」→ 下一帧以 0 重画：列表
  纹丝不动（滚动条 thumb 也永远钉在顶端，因为 offset 恒 0）。三个调用点同源
  全中：文件树（Fill）、可用模型列表（Capped 360）、字体弹层（Fill）。
- ✅ 修法：句柄表收进 vlist——`scroll_handle(id)`（thread_local
  `HashMap<&'static str, UniformListScrollHandle>`，按 id 复用）。本 app 单窗口
  （`main.rs` 只 `open_window` 一次）+ gpui 渲染单线程，等价于「句柄挂在视图
  上」，不必让三个调用点各穿一根句柄（字体弹层的渲染闭包拿不到 Chat）。约束：
  同一 id 同屏只出现一次（现三个 id 互不相同）。
- ✅ 验证：`cargo test -p app` **115 全绿**，新增 `ui::vlist` 两条——句柄按 id
  复用（同 id 同一 Rc / 不同 id 不串）、**滚轮 → 下一帧仍按新偏移构建行**的真
  布局锁（模拟 ScrollWheelEvent 后断言本帧构建的行号集合 = 视口移到了第 3 行）。
  反证有效：把 `scroll_handle(id)` 改回每帧 `new()`，新测试立刻红
  （`滚轮偏移必须写进 vlist 复用的句柄: left 0px`）。
- ⏳ 真机待复验：文件树滚轮上下滚到任意位置不弹回；设置·模型·可用模型列表滚轮
  滚动 + 右缘 thumb 跟随（可拖拽、轨道翻页）；顺带看字体下拉列表。
- ⚠️ 附带说明（不是本轮改出来的）：gpui 的滚轮派发给**所有命中的可滚动
  hitbox**（含祖先），所以设置页里滚列表时页面自身也会同时滚（pi-web 的原生
  嵌套滚动是先内后外）。要「列表滚到底才带动页面」需要给 vlist 加滚轮
  拦截（stop_propagation + 到底才放行），等真机手感反馈再定。

## 待验证（本轮修复）——操作栏「复制」图标不显示

用户口径：操作栏里「编辑」「新分支」图标都在，只有「复制」前面空着。

- ✅ 根因：`ui::icon(name)` = `gpui::svg().path("icons/{name}.svg")`，资产靠
  `assets.rs` 里手写的 `assets!()` 列表 + `include_str!` 编进二进制。
  `copy.svg` 文件在磁盘上，但**没写进那个列表** → `Assets::load` 返回 `None` →
  gpui 对取不到的 svg **静默画空白**（不报错、不 warn）。用户行与 agent 轮两处
  复制图标同源，所以一起消失；其余图标都登记了，故正常。
- ✅ 修法：`assets!()` 补 `"icons/copy.svg"`；新增
  `assets::tests::every_icon_file_and_call_site_is_registered` 双向锁（磁盘 svg
  必须全登记 + 源码每个 `icon("x")` 必须有资产）。旧测试只遍历宏列表，漏登的
  文件它看不见——bug 正是从这个盲区漏出去的。
- ✅ 验证：`cargo test -p app` 91 全绿；临时删掉登记行测试立刻报出
  `["copy.svg"]`（反证有效）。真机待复验：两处操作栏「复制」前出现双矩形图标。

（细节见 `docs/progress.md` 的「v63-4」。）

## 待验证（本轮修复）——用户消息操作栏悬停不显影

用户口径：鼠标停在**用户消息**上看不到底部操作栏（复制/编辑/新分支 + 时间），
agent 回复的轮块却正常；同一个行为不该有两套写法。

- ✅ 根因：用户气泡挂了 `div().occlude()`（v57 加的，注释写「禁止鼠标透传到下层
  消息」）。gpui `Window::hit_test` 从后往前收集 hitbox，一旦命中
  `HitboxBehavior::BlockMouse` 就 `break` —— 比它**先插入**的 hitbox（该气泡的
  **全部祖先**，含挂行级 `on_hover` 的 `msgrow-*`）连 `ids` 都进不去，
  `hitbox.is_hovered()` 恒 false。于是鼠标停在气泡上时行级 on_hover 永不触发 →
  `Chat.bar_hover` 不置位 → 操作栏 `opacity` 停在 0。agent 轮块没有遮挡后代，
  所以同一个功能在两处表现不同。连带副作用：`ids` 被截断也让滚轮在气泡上失效
  （外层会话列表收不到 scroll），气泡内滚到底后不能再滚会话。
- ✅ 修法（统一实现）：删掉气泡的 `occlude()`；把两段内联 `on_hover` 合并成
  `messages::bar_hover_wired(el, weak, ix)` 唯一接线，状态也从
  `user_bar_hover` / `turn_bar_hover` 两字段并为 `Chat.bar_hover: Option<usize>`
  （用户行 = msg_ix，agent 轮 = 轮首 msg_ix，角色不同故永不撞车）。
- ✅ 验证：`cargo test -p app` 90 全绿（新增 `messages::bar_hover_hit_test`
  正反两侧——不遮挡必须触发行悬停、遮挡必须被吞，锁住上面那条 gpui 语义）。
  真机待复验：鼠标停在用户气泡任意位置（含气泡内文字、长气泡滚动区）操作栏都显影。

（细节见 `docs/progress.md` 的「v63-3」。）

## 待验证（本轮修复）——发言钉顶 / 回复占满整屏

用户口径：每次发言，用户消息刷新到屏幕顶部；把整屏留给 agent 回复。

- ✅ agent 输出顶出屏幕、到输出结束都不自动上滚：快流下一帧 2~4 个事件，
  `sync` 第 5 步重测尾部把「承载整轮内容的轮首条目」打回 Unmeasured，同帧第二个
  事件结算就量到 0 → 钉顶/跟尾判定每事件翻转 → 视图被钉顶压住不跟尾。修法：第 5 步
  避开轮首条目（它本就每帧被布局重测）。另按口径把下边距 135→150：跟尾时内容末条
  停在「输入面板上沿 + 20px」。
- ✅ agent 内容顶到屏顶、用户消息消失（每次消息都「顶上去」）：钉顶靠精算垫片
  （内容区高 − 锚下内容）撑着，但 gpui 在「锚下填不满视口」时会丢掉逻辑位改成
  贴底胶水，而胶水用**缓存条目高度**定位——内容在两帧之间变矮（轮末折叠、等待行/
  思考块收起、下一条消息开始，尤其模型派发后的静默期无 pi 事件）时，垫片偏大 →
  覆盖成立 → 逻辑位被夺 → 胶水把起点算得更高 → 锚点不在绘制范围里。修法：垫片在
  钉顶期改成**常数（一整屏内容区高）**，覆盖条件恒不成立，锚点位置与测量精度解耦。
- ✅ 流式五步走错：pi-flash 在第②步就给出折叠的「工作详情」（思考框看不见、
  徽章不显示），随后翻页乱。pi-web 的 `isLiveTail` 是**运行中这一轮平铺渲染**
  （每消息模型名行 + thinking/tool/text 就地展开），折叠行只在轮末整形才出现。
- ✅ 内容形状一变（轮末折叠/手动展开/chips 出现）贴底胶水就把锚点摆错位置，
  只能等下一条 pi 事件自愈：`take_frame_sync` 增加每帧垫片自检。
- ✅ 流式期间没有「模型名 + ↓token估算 + t/s 徽章」行、也看不到思考框：
  消息区判「工作中」误用过期的 `get_state.is_streaming` 快照（一轮内无人重拉 ⇒
  恒 false），已改用事件驱动 `agent_running`（与 composer 同源）。
- ✅ 发送后消息不在屏顶（或干脆整片白）：垫片公式漏减列表上下内边距
  （22 + 135 = 157px）——gpui 贴底胶水把末条底边钉在 `屏底 − padding.bottom`，
  少减这两个内边距，锚点会停在屏顶之上 135px，可见区只剩空白垫片。
- ✅ 回复流式增长时消息一路往屏顶外漂：垫片结算用的 `bounds_for_item` 对
  「逻辑位之前」的条目直接返回 None，而胶水态逻辑位恒为 item_count ⇒ 垫片只能
  结算一次，之后全靠一帧前旧高度。
- ✅ 回复结束后消息从屏顶跳到屏底：轮末退役了锚点（卸垫片），Bottom 对齐把
  短内容拽回屏底；pi-web 只让垫片收敛、滚动位保持。

（本轮修复细节与验证见 `docs/progress.md` 的「v59 滚屏三修」一节。）

## 0.1.0（全部修复于 0.1.1）
- 左侧导航栏问题：
  - ✅ 选中项目后，该项目的对话列表没有显示（已修复）
  - ✅ 新建 按钮后，页面应当刷新如下（已修复，含空态 Logo + 版本号）：
  ![新建页面](image.bmp)
  - ✅ 文件浏览器 窗口默认导航栏一半高度（已修复）

- 中间输入框问题：
  - ✅ 点击输入框不显示闪动光标，没有聚焦效果（已修复）
  - ✅ 点击思考强度，不应该是循环切换，而应该弹出菜单让用户选择（已修复）
  ![思考强度菜单](image_1.bmp)
  - ✅ 点击工具配置没反应，应该弹出工具配置菜单，让用户选择（已修复）
  ![工具配置菜单](image_2.bmp)
  - ✅ 点击 压缩 按钮，没反应（已修复）
  - ✅ 点击 声音提示 按钮，没反应（已修复）

- 左侧面板完全没实现：
  - ✅ 右侧面板宽度可拖动，且提供多标签，可在文件、terminal 之间切换，具体如何实现去查pi-web源码（已修复）
  - 文件浏览器 点击 terminal 按钮后的界面：
  ![打开terminal界面](image_3.bmp)
  - 文件浏览器 点击 文件后的界面：
  ![文件浏览界面](image_4.bmp)

- ✅ 上面命令面板没实现（已修复）：
查看pi-web源码复刻实现

## 待验证（本轮修复）
- ✅ 发送消息后到 agent 首个 token 之间没有等待动画（设计 §13 已设计，实现被回显逻辑掐掉）
  - 根因：`phase_waiting` 同时充当「等待行可见」与「乐观发送待回显」标记，pi 回显 user 消息（瞬时）把它清零 → 等待行一闪即灭
  - 修法：拆出 `pending_echo: Option<String>` 只做回显去重；`phase_waiting` 仅由 assistant `message_start`/`message_update`、`agent_settled`/`agent_end`、`prompt` 失败、Esc 中止清零
  - 顺带补齐设计缺口：等待行上方模型名（as-name）、「正在思考」后 1.4s/4 步循环省略号
  - 验证：`cargo build` 后发一条消息，确认 spark 旋转 + shimmer 一直转，直到 agent 首个 token 出现才消失
- ✅ **中文输入第二个字就崩**（`editor_input.rs:86` `assertion failed: self.is_char_boundary(n)`，退出码 0xc0000409）
  - 根因：IME 组合路径把 **char 下标**当 **字节下标**用 —— `utf16_to_char_offset()` 返回 `chars().enumerate()` 的下标，喂给 `String::replace_range`；ASCII 下 char==byte 所以不暴露，中文一个字 3 字节，第二个组合更新时越界到非字符边界 → panic 带走进程
  - 修法：改 `utf16_to_byte_offset()`（`char_indices` 累计 `len_utf16`）；新增 `safe_replace_range()` 夹取 + 对齐字符边界（平台 range 可能越界/反转）；`text_for_range` 用 `String::from_utf16_lossy`（原来逐 u16 `char::from_u32` 会把代理对拆成 U+FFFD）；两个写入方法补 `cx.notify()`
  - 遗留：chat composer 是全项目唯一手写的输入框（其余输入框都走 `ui::TextInput` = gpui-component `InputState`，自带 Windows IME）。根治方案 = composer 也换 `TextInput`；手写版只有「光标在末尾 + 退格」能力，无选区/方向键/复制粘贴
  - 验证：中文输入法连续输入（含中英混输）不再崩；拼音组合串与候选上屏正常
- ✅ 等待动画下方的 shimmer 骨架条 → **已按用户要求删除**（不再实现）
  - 该条是设计 §13 的 `.shimmer`（260×10 圆角 + 渐变扫过），本项目判定为视觉噪音：已有「正在思考…」文案，骨架条不承载信息
  - 同步删除：`session/mod.rs` 的 `shimmer_bar()` 函数 + phase row 的 child；设计 HTML 的 `.shimmer` CSS / `@keyframes slide` / `<div class="shimmer">`；设计说明 §13 已标注作废
  - 保留：模型名 + 旋转 spark +「正在思考…」（省略号循环）

## 待验证（2026-10-05 「新分支」按钮修复）
- ✅ **用户消息下的「新分支」点了没反应**
  - 根因①：pi-link `parse_line` 用 serde_json 默认解析（递归上限 128），
    而 `get_tree` 按 children 嵌套整条会话（85 条消息 → 树深 95 → 整行被
    丢弃、无事件）→ 用户消息拿不到 entry id → 按钮不挂 handler
  - 根因②：长会话（实测 2970 条 entry）上 **pi 自己** 的 `get_tree` 就
    `Maximum call stack size exceeded`，客户端再准也拿不到树
  - 根因③：pi fork 是进程内 rebind 到新分支文件，旧实现没跟着换
    `self.file`/pool key → 分支生效了但侧栏还高亮父会话
  - 修法：`json::parse_value`（unbounded_depth）+ reader 线程 16MB 栈 +
    非 UTF-8 行不再 break 流；新增 `get_entries` 扁平链回溯算锚点
    （pi-web sliceActiveBranch 同源）；`follow_session_file` 跟随重绑定；
    按钮补 pi-web forking 态（创建中…/主题色/禁用/栏常显）
  - 验证：① 打开一条 85+ 条消息的老会话，hover 用户消息 → 「新分支」可点，
    ② 点击后状态栏出现 forked，侧栏高亮切到新分支会话，标题改为新分支内容，
    ③ 在新分支里发一条消息正常流转；④ 长会话（1000+ 轮）里「新分支」同样可用
- ✅ **「新分支」入口位置反了 + 克隆出来的会话重名**（用户定案两改）
  - ① 入口从用户消息栏移到 **agent 轮操作栏**：`fork` 只能 before，分支点 =
    该用户消息的 parentId（这条用户消息不带过去），点自己却从它之前开始；
    正确语义 = 从 agent 回复处 clone，保留到本轮为止、其后丢弃。尾部轮
    （无下一条用户消息）走 rpc `clone`（整段复制）
  - ② 分支自动改名：原名截取 20 字（超长加 "…"，按字截断），fork 前捕获
    原 title，落地后 `set_session_name` 写到新分支文件上
    （初版为「前 15 字 + "2"」，用户连做两次 clone 后反馈后缀叠成 `…22` 很蠢 → 去掉后缀）
  - 验证：agent 回复 hover 出现「新分支」；点击后新分支侧栏名字为
    「<原名前15字>2」；分支内容停在点击的那轮回复之后
- ✅ **等待回复时稍一滚就跳底，之后滚轮彻底失灵（会话区滚屏）**
  - 根因：滚轮回调把任何"非贴底"滚动都判成用户主动离底（钉顶期逻辑位恒
    为 Some，滚一格就中）→ 退役锚点、摘掉整屏垫片 → 内容高 < 视口，Bottom
    贴底胶水把整块内容拽到屏底；砸底后滚动余量归零（内容短于视口 ⇒ 最大
    滚动位就是尾部位），滚轮从此无响应，想回钉顶只能再发一条消息。叠加
    vendor `list()` 滚轮累加器缺陷（同帧 N 事件把累计 delta 整包重放 N 次，
    流式全窗重建拖长帧时事件积压 → 一滚跳一大截）
  - 修法：滚轮改 `free_scroll` 软状态（chat_list.rs）——只暂停 sync 第 6 步
    重钉与 settle 跟尾翻转拽底，锚点与垫片保留：微滚平滑滑动、滚回贴底
    （几何上≡钉顶位）自动恢复钉顶、读历史不被流式拽走；`release` 真退役
    只留给发送失败/fork/整表重读。vendor `elements/list.rs` 滚轮改只应用
    本事件 delta + 真实行高（div 同款；升级 vendor 需重套）。有意偏离
    pi-web promptAnchorActive（原"勿当 bug 修"口径作废）
  - 验证：短会话发消息 → 钉顶后轻滚一格，内容平滑上移不跳底；滚回底部
    恢复钉顶视图；上滚读历史时流式继续、视口不被拽走；长回复照常转跟尾；
    `cargo test -p app` chat_list 单测 + glue_geometry 几何锁全绿
