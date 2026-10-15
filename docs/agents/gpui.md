# gpui 0.2.2（vendored）/ gpui-component 陷阱

改动会碰到的文件：`crates/app/src/**`（渲染层集中在 `ui/`、`session/`、`function_panel/`）。

依赖形态：gpui 0.2.2 与 gpui-component 经 `[patch.crates-io]` 指 `vendor/gpui`、`vendor/gpui-component`（workspace `exclude`；debug profile 给两者保留行号表，可断点进内部）。随踩随补。

## 滚动

- `overflow_y_scroll`/`track_scroll` 只在 `Stateful<Div>`（先 `.id()`）；`visible_on_hover` 不存在 → `.group()` + `group_hover`。
- `list()` 虚拟列表条目**只认上下 padding**，左右无效 → 外层容器包 `.px()`；item 宽度语义不可靠（psp 弃用改全量 div）。
- `uniform_list` 契约 = 容器 fixed/max 高（Fill 需 relative+flex_1 外包 + absolute 定死四角，否则画全部内容）；**UniformListScrollHandle 必须跨帧复用**（每帧 `new()` = 滚动全丢）→ `ui::vlist` thread_local 句柄表按 id 复用。
- 滚轮派发给**所有**命中可滚动 hitbox 含祖先（嵌套 = 内外同时滚，非先内后外）。
- 滚屏铁律：Bottom 对齐下内容填不满视口 → 强制 logical=None 贴底（钉顶必须靠 spacer 垫片，常数化最稳）；`ListState::reset()` 清滚动位（notify 前存、后恢复）；`scroll_to*` 不触发 notify（程序化滚动自补通知 + window.refresh）；滚动回调内**严禁**触碰 ListState（BorrowMutError → 置标记渲染帧补账）。
- **编辑器滚动手感对照 Zed（2026-10-16 排查结论，pi-flash 滚屏优化）**：Zed 滚动帧 = 视口行排版（gpui 两帧 LineLayoutCache 命中，只有新进入行 shape）+ 从 sum-tree 缓存读高亮（**无每帧树逃逸查询**）+ f64 滚动位置设备像素吸附；PF 已有同款排版缓存，差距在三处，均已修：
  1. **每帧高亮查询**：视口字节窗内每帧重跑 tree-sitter query + `unique_styles` 平方扫 + 每捕获 String 分配 → 捕获名 intern（highlighter.rs）+ unique_styles 边界扫描线 O(N log N)（语义经 app 侧差分测试 `crates/app/tests/vendor_highlight_diff.rs` 钉死，2000 组随机输入与旧实现全等）；
  2. **每 tick 双帧**：Scrollbar::paint 检测到 offset 变化会再 `cx.notify` 一次（scrollbar.rs），宿主（InputState）对同一变化已 notify → 每个 wheel tick 渲染两个全窗帧 → 改为 paint 时就地调度 FADE_OUT_DELAY 定时器，不再立即 notify。注意 PF 主题是 `ScrollbarShow::Always`（composer 常显，用户定夺），渐隐链路本就不跑，此改动对 PF 零视觉影响；
  3. **亚像素抖动**：行高默认 1.5×字号，非整倍缩放下是小数（1.5× 缩放、14px → 设备 31.5px），滚轮步进 = N×行高 → 整屏行落在小数设备像素 → `snap_line_height`（element.rs，request_layout/prepaint/paint 三处同一纯函数）吸附整设备像素。Zed 对应物是 editor element 的 pixel_snap。
  仍未对齐（结构性，另立项）：每次 notify 全 Chat 重建（见「交互与实体」节）；PF 侧 wheel 是 Lines 量化（WM_MOUSEWHEEL 系统行为，Zed Windows 同款），无 trackpad 像素路径。
- vendor 测试目标跑不了：`cargo test -p gpui-component` 报 "requires dev-dependencies and is not a member of the workspace"（workspace 排除所致，非编译错误）→ vendor 纯函数回归测试放 app 侧（见 crates/app/tests/vendor_highlight_diff.rs 模式：旧实现照抄做参照 + 差分）。

## 拖拽（窗口内 DnD）

- 0.2.2 API 面**全有**：`on_drag`（发起+造幽灵，Stateful 才行）/ `drag_over::<T>` /
  `on_drop::<T>` / `on_drag_move::<T>` / `can_drop` / `DragMoveEvent` /
  `cx.active_drag`·`has_active_drag`·`stop_active_drag` / OS 文件拖入
  （`ExternalPaths` + FileDropEvent）。**缺** `external_drag_payload` 一族——
  窗口内 drag 提升不成 OS 拖拽（拖不到资源管理器/其他窗口）。
- drop 派发 = MouseUp bubble 按**绘制逆序**（后代先）：监听器注册在子元素
  绘制之前（div.rs `paint_mouse_listeners` 先于 content 闭包），冒泡 `.rev()`
  → 第一个 TypeId 匹配且 hovered 的监听 `active_drag.take()` + refresh +
  stop_propagation。行上挂 `on_drop`、容器上挂兜底 `on_drop` 不会双触发；
  出了任何 drop 目标的 mouse-up = 整个拖拽取消（active_drag 置空 + refresh）。
- **拖拽期 hover 样式被抑制**（compute_style 在 `has_active_drag()` 时跳过
  hover/group_hover）→ `drag_over` 高亮是唯一落点反馈；拖动期每个 mouse-move
  全窗 refresh（幽灵跟随），drag_over 逐帧重估。
- 拖拽发起 = pending_mouse_down + 左键 + 位移 > `DRAG_THRESHOLD`(2px)。**按下
  即触发的动作（on_mouse_down）会在拖拽起手时先行误触发**——要拖拽的元素
  动作挂 `on_click`（mouse-up，与 drag 天然互斥：drag 起手会清 clicked_state）。
- `on_drag_move` 是 capture 相、**无悬停过滤**：所有在册元素每次 move 都收到，
  必须自查 `event.bounds.contains(&event.event.position)`；做「悬停自动展开」
  用它 + 目标去重状态 + 出界清目标 + timer 任务（pi-flash 文件树 500ms 案例在
  function_panel/file_tree.rs `tree_drag_wiring`）。
- 挂 `drag_over`/`on_drop`/`on_drag_move` 只需 `InteractiveElement`（无 id 的
  div 也行）；`on_drag` 必须 `Stateful`（.id()）。
- PF 案例（021 文件树拖拽移动）：file_tree_drag.rs（payload/幽灵/落点判定）+
  file_tree.rs `tree_drag_wiring` + file_actions.rs `tree_move_commit`。

## overlay 与 hitbox

- overlay 必须 `absolute().inset_0()` + `occlude()`（防 0 高容器裁掉子元素 / 鼠标穿透）；但 `occlude()` 会**截断祖先 hitbox**（hit_test 从后往前 break）→ 行级 hover 失效、滚轮困死——消息气泡禁止加 occlude。
- hover 事件按绘制逆序派发：向下移动时上一行 leave 晚于下一行 enter → enter-only 置位。
- gpui 无逐元素 backdrop blur → 遮罩用 bg 色 80% 透明近似柔雾。

## 文本与绘制

- **StyledText runs 的 font_size/font_weight 无效**（shape_text 只认容器继承链）→ 字号/加粗挂容器 div（markdown sized_text/weight 参数即此）。
- 字节/字符边界：gpui highlight/切片 range 按字节且必须落字符边界（粘贴 panic 0xc0000409 两连的根源）。
- **注入语言高亮（`<style>`→css / `<script>`→javascript）是每可见行成本**：
  `InputElement::highlight_lines` **逐可见行**调 `SyntaxHighlighter::styles()`，
  而 vendor 原 `handle_injection` 每次调用都 `parser.parse` 整块注入内容 + 在
  整棵注入树上跑全量 query（无缓存、无范围裁剪）→ 视口 N 行落在注入块内就是
  N 次整块 parse。实测 118KB 设计稿 HTML（32KB `<style>`）**每帧 ≈8s**：应用
  整体假死，连 pif-ui `snapshot` 都要等 6s+、`file.open` 一个**已开** tab
  （复用分支，不读盘）也要 9s——全是在等 UI 线程。
  已修（`vendor/gpui-component/src/highlighter/highlighter.rs`）：注入解析成品
  按 (语言, 注入节点字节范围) 缓存（`update()` 换文本时整表清空），查询用
  `set_byte_range` 裁到请求行在注入内容里的字节段。复测同文件 `app.info`
  8.1s → 0.18s，CSS/JS 高亮颜色不变（pif-ui `shot` A/B 目视）。
  **复核/回归只能走 app 侧 pif-ui 实测**：`cargo test --lib` 在 vendor/
  gpui-component 里现在编译不过（上游 `tests/fixtures/layout.json` 未 vendor +
  `input/text_wrapper.rs` 测试两处 SmallVec 类型推断），高亮单测跑不了。
  量法：隔离实例开文件后连打 `pif-ui exec app.info`，返回值 >1s 即此病复发。
- svg 未登记进 `assets!()` 会**静默画空白**（有双向一致性测试守卫）。
- **超大单行（minified js，94 万字符）每帧假死**：`visible_range` 是**逻辑行**
  粒度，旧实现把整行字节范围送进高亮查询与 shaping——`match_styles` 对
  94 万字符行产出 ~20 万条 capture，再进 `unique_styles` 的 O(区间×样式)
  二重扫（每帧 ~10^10 次比较，分钟级）；`layout_lines` 对该行 ~6000 段全量
  shape_line + 整段 String 物化（941KB/帧）。87K 的行只要落进 visible_range
  就已经是 ~10^8/帧 的卡。修法（2026-10-14，pi-flash-jtt）：prepaint 按行高
  几何算「视口字节窗」（上下各留 2 行余量供方向键/选区端点取 x），
  `highlight_lines` 逐行查询范围与窗求交；`layout_lines` 窗外段垫等 len 的
  空 `ShapedLine`（段数/字节 len/y 槽位全保留——光标/点击/选区的行内定位
  按 len 累加，x 取 0 只发生在屏外行）。**配套不变式（2026-10-15 补，缺它
  必炸）**：`highlight_lines` 返回的样式区间必须**从 visible 文本 0 起连续
  平铺**——消费端（layout_lines → runs_for_range → shape_text）按顺序吃
  run.len，不读 range.start；窗裁剪后窗前/窗后要补默认样式 filler 段，否则
  run 边界相对文本整体错位：ASCII 文件只是高亮悄悄错位不易察觉，含 CJK
  的文件在 direct_write shape_text 里被从多字节字符中间切开直接 panic
  （`byte index is not a char boundary`，0xc0000409）。另注意
  `gpui::combine_highlights` 是边界扫描线，输入有缺口输出就丢段——filler
  必须自己补，它不会兜底。配套：`closest_index_for_*`/
  `index_for_position` 的字节累加改用覆写后的 `len()`（垫段 text 为空）；
  `LineLayout::paint` 跳过空 text 段（省 ~6000 次 paint_layer/帧）。
  量法同注入修复：隔离实例开 echarts.min.js（1MB/45 行/末行 94 万字符），
  `pif-ui exec app.info` 稳态 45-70ms（修复前分钟级假死）。已知边界：在
  超长行内打字仍是整行 rewrap（wrapper 按逻辑行增量，~0.1-0.5s/键），
  打开/滚动/查看已顺。
- **`calculate_visible_range` 行空间混用（大单行滚到文件底 panic）**：
  visible_range 是逻辑行空间，但非等高路径初始值曾是 `text_wrapper.len()`
  （=软换行行数）——滚到文件底部 break 不触发时 end 跑到几千，与循环里
  赋值的逻辑行号混用。旧 `layout_lines` 靠 `slice_lines` 越界钳制掩住；
  改逐行迭代后直接 panic（`line should exists in text_wrapper`）。已钳到
  `lines.len()`，layout_lines 侧留防御性跳过。
- `icon_hover` 自定义元素在 list 虚拟条目里占位不绘制 → 用 `icon()`/`icon_current`。
- `WindowBounds::Maximized` 创建路径无效 → `zoom_window()`。

## 交互与实体

- **交互 handler（on_mouse_down/on_key_down/listener）漏 `cx.notify()` = 状态变 UI 不动。**
- 双重租约：事件派发期间同步 update/drop 正被派发的实体 → `cx.defer`；自动化 keys 同理（INPUT_KEYS 走 window.defer）。
- `window.update`/`chat.update` 不 flatten（catch_unwind 四层嵌套）；vendored `WeakEntity::update` 签名与 Zed 上游不同，别照抄；渲染期读取的实体会注册 window invalidator（InputState notify 即重渲染）。
- **vendored gpui 0.2.2 的 `Entity` 子元素每帧无条件重渲染**（view.rs `impl Element for Entity` 直接 `view.render()`，没有脏检查复用）；唯一复用路径是 `AnyView::cached(style)`（未 notify 即回收上一帧 layout+paint，`window.refresh()` 会击穿）。后果：composer 每键 `Chat` notify = **全窗口重建**（消息区 markdown 元素 + dock + chrome 全连坐）。消息列表是 `list()` 闭包不是实体，吃不到 cached——要隔离只有把轮次包实体 + cached（大手术）。在此前提下，Chat::render 每帧路径严禁 O(会话规模) 的分配：命令表/模型目录/全文拷贝都要门控或借用化（composer 打字卡顿 2026-10-12 排查的落点：slash_commands 深克隆、new_session_default 全目录扫描、set_value 镜像克隆）。

## keymap 与快捷键

- 键串里的 `cmd-` 是 **platform 修饰键**（解析成 `modifiers.platform`，Windows 显示语义=Win），不是 ctrl 的别名；PF 不发 macOS，**禁注册 `cmd-*` 绑定**：`Kbd::binding_for_action` 反查走 `highest_precedence_binding_for_action`（= 注册序最后一条），gpui-component `Kbd::format` 把 platform 渲成字面 `"Win"` → tooltip 出现「Win+P」假键位（实际生效的仍是 ctrl-p）。全仓 keymap 注册点唯一：`main.rs` 初始化。

## 焦点

- 焦点分两层：`window.focus(&h)` 直接写 `Window.focus`（`is_focused()` 读的就是它，`focus()` 同值提前 return），而**帧焦点路径**（`rendered_frame.focus`）只由这一帧 prepaint 过的 `set_focus_handle` 写出。给一个这一帧**不在元素树里**的句柄聚焦（组件的 fallback 句柄、滚出视口的虚拟列表行里的输入框）= 路径为空：旧持有者收 blur、新持有者收不到 focus，键事件按 root 兜底，而 `Window.focus` 还留着那个悬空 id（下次渲染又「复活」）。改焦点一律先问「这个元素这帧渲染了吗」。
- `Chat::render` 的焦点级联每帧仲裁「谁持焦」（优先级：对话框/弹层 > 设置 > 文件编辑器 > 终端 > 行内编辑 > composer）。要在**存续期**持焦的行内输入框必须进这条级联；只挂一次性聚焦、靠 blur 收场的写法会被级联末级的 composer 回收逐帧打掉（文件树行内改名「焦点捕捉不到」的根因）。真夺焦（对话框/终端/编辑器/设置）走级联靠前分支，blur 收场照常。
- gpui-component `InputState` 内件首帧才建（`TextInput::ensure_state`）→ 首发聚焦走 `TextInput::focus_soon`（want_focus 在输入框自己的 render 里兑现，内件建好即同帧生效，Zed `focus_filename_editor` 请求同位）；此后兜底用 `has_inner()` + `focus_handle_in()`（内件未建时不要碰 fallback 句柄）。

## gpui-component

- Input `set_value` 全文走 utf16 编辑管线（4.6MB ≈48ms）→ 大文本走 vendor state.rs 的 reset 路径（换 Rope + wrapper.reset）。
- **md 软换行 ⇄ WYSIWYG 解耦（2026-10-14 用户定夺）**：md 源码编辑器折行恒开（`editor/kind.rs` EditorConfig，stock 路径回行）；折叠与折行**结构互斥**（`element.rs` 折叠分支 `debug_assert!("折叠态必须关软换行")`——折叠改写每行字节长，text_wrapper 换行段按 doc 文本算），WYSIWYG 装饰挂上期间的关折行**只**收口在 `editor/markdown/doc.rs sync_live_decorations`（挂装饰原子关、摘装饰原子恢复；早退 `has_decorations()==live` 蕴含折行已同步）。别在别处单独动 md 编辑器的 soft_wrap，也别给 MdLiveProvider 之外的装饰路径默认开折行。

## 版本要点（0.2.2）

- `cx.spawn(async move |weak, &mut AsyncApp|)`；prompt_for_paths 返回 oneshot Receiver；HighlightStyle 无 builder（字段赋值）。
- 杂项：prepaint/paint 第 5 参是 prepaint state 非 hitbox；Pixels 字段私有（f32::from/除法）；move 闭包捕获 `&WeakEntity` 参数 E0521（闭包外 clone）；`unbounded()` 返回值不能别解构。
- **vendor 本地改动（升级必须重套）**：`elements/div.rs` `TOOLTIP_SHOW_DELAY` 500→350ms（用户定夺 2026-10-13，350ms 弹出体感更跟手；源码处有注释标记）。
- **vendor 本地改动（升级必须重套）**：`input/state.rs` `set_selected_range` 显式清 `selection_reversed`（不变式：该 setter 光标恒在选区尾）+ 新增 `set_selected_range_reversed`（光标锚选区**起点**，滚动保起点可见）——app `TextInput::select_all_on_focus` 全选改走它（改名框头部可见，2026-10-13 用户定夺）；`cursor()`/滚动早有 reversed 分支，此补丁只补公开设置口。
- **vendor 本地改动（升级必须重套）**：`input/state.rs` `set_soft_wrap` 抽出无 window 变体 `set_soft_wrap_cx`（上游签名里 window 参数本就被忽略）——app 渲染帧同步点（sync_live_decorations）手里只有 `Context<Self>`；`set_soft_wrap` 原签名保留并委托。
- **vendor 本地改动（升级必须重套）**：`input/element.rs` 视口字节窗（`viewport_byte_window` + `EXTRA_SHAPE_ROWS`：`highlight_lines` 逐行查询与窗求交 + 窗前/窗后默认样式 filler 保「从 0 连续平铺」不变式、`layout_lines` 窗外软换行段垫等 len 空 ShapedLine、`calculate_visible_range` 非等高路径钳逻辑行数）+ `input/text_wrapper.rs`（`LineLayout::paint` 跳空 text 段；`closest_index_for_x`/`closest_index_for_position`/`index_for_position` 字节累加改用覆写后 `len()`）——超大单行（minified js）每帧假死修复，2026-10-14，pi-flash-jtt；平铺 filler 补丁 2026-10-15（CJK 文件 char boundary panic）。
- **vendor 本地改动（升级必须重套）**：`highlighter/highlighter.rs`（`capture_names`/`InjectionQuery.capture_names` 预建捕获名表 intern，`match_styles`/`handle_injection` 零分配；`unique_styles` 改边界扫描线 O(N log N) 并 `#[doc(hidden)] pub` 导出——语义由 app 侧 `crates/app/tests/vendor_highlight_diff.rs` 差分钉死）+ `input/element.rs`（`snap_line_height` 行高设备像素吸附，request_layout/prepaint/paint 三处统一）+ `scroll/scrollbar.rs`（paint 的 offset-change 不再立即 `cx.notify`，就地调度渐隐定时器——PF 主题 Always 常显下零视觉影响）——滚屏手感对齐 Zed 三项，2026-10-16。
- **图片格式全集（editor 打开路由，2026-10-16）**：`ImageKind::try_binary`（zed `create_gpui_image` 同款 match）认 Png/Jpeg/Gif/Webp/Bmp/Tiff/Ico/Pnm 八种，magic bytes 裁决（`image::guess_format`），avif/tga/dds/hdr/exr/qoi/farbfeld 过扩展门但解不出 → None 落灰屏。gpui `img()` 解码内部自己 re-sniff magic（`elements/img.rs` 里 `image::guess_format`，Gif/WebP 动画走专路），`Image::from_bytes` 声明的格式只是 hint，真图后缀配错字节也能渲染出真格式。
- **vendor 本地改动（升级必须重套）**：`platform.rs` `ImageFormat` 补 `Ico`/`Pnm` variant（zed HEAD 同款，Tiff 0.2.2 自带）+ `mime_type` 两臂 + `from_mime_type` 改 zed HEAD 遍历式（iter + `from_mime_type_alias` jpg/tif 别名）+ `to_image_data` 两臂；`platform/windows/clipboard.rs` `From<ImageFormat> for image::ImageFormat` 两臂；`platform/linux/x11/clipboard.rs` atom 表补 `ICO__MIME`/`PNM__MIME`（zed HEAD 同名）+ set_image match 两臂 + get_any 读数组 7→9；`platform/mac/platform.rs` `From<ImageFormat> for UTType` 补占位臂 + `try_clipboard_image` 对 Ico/Pnm 提前返回 None（UTType 无内建等价，zed HEAD 已重构掉该转换无官方写法）。wayland 走 `ImageFormat::iter()` 自动适配。
