# gpui 0.2.2（vendored）/ gpui-component 陷阱

改动会碰到的文件：`crates/app/src/**`（渲染层集中在 `ui/`、`session/`、`function_panel/`）。

依赖形态：gpui 0.2.2 与 gpui-component 经 `[patch.crates-io]` 指 `vendor/gpui`、`vendor/gpui-component`（workspace `exclude`；debug profile 给两者保留行号表，可断点进内部）。随踩随补。

## 滚动

- `overflow_y_scroll`/`track_scroll` 只在 `Stateful<Div>`（先 `.id()`）；`visible_on_hover` 不存在 → `.group()` + `group_hover`。
- `list()` 虚拟列表条目**只认上下 padding**，左右无效 → 外层容器包 `.px()`；item 宽度语义不可靠（psp 弃用改全量 div）。
- `uniform_list` 契约 = 容器 fixed/max 高（Fill 需 relative+flex_1 外包 + absolute 定死四角，否则画全部内容）；**UniformListScrollHandle 必须跨帧复用**（每帧 `new()` = 滚动全丢）→ `ui::vlist` thread_local 句柄表按 id 复用。
- 滚轮派发给**所有**命中可滚动 hitbox 含祖先（嵌套 = 内外同时滚，非先内后外）。
- 滚屏铁律：Bottom 对齐下内容填不满视口 → 强制 logical=None 贴底（钉顶必须靠 spacer 垫片，常数化最稳）；`ListState::reset()` 清滚动位（notify 前存、后恢复）；`scroll_to*` 不触发 notify（程序化滚动自补通知 + window.refresh）；滚动回调内**严禁**触碰 ListState（BorrowMutError → 置标记渲染帧补账）。

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
- `icon_hover` 自定义元素在 list 虚拟条目里占位不绘制 → 用 `icon()`/`icon_current`。
- `WindowBounds::Maximized` 创建路径无效 → `zoom_window()`。

## 交互与实体

- **交互 handler（on_mouse_down/on_key_down/listener）漏 `cx.notify()` = 状态变 UI 不动。**
- 双重租约：事件派发期间同步 update/drop 正被派发的实体 → `cx.defer`；自动化 keys 同理（INPUT_KEYS 走 window.defer）。
- `window.update`/`chat.update` 不 flatten（catch_unwind 四层嵌套）；vendored `WeakEntity::update` 签名与 Zed 上游不同，别照抄；渲染期读取的实体会注册 window invalidator（InputState notify 即重渲染）。
- **vendored gpui 0.2.2 的 `Entity` 子元素每帧无条件重渲染**（view.rs `impl Element for Entity` 直接 `view.render()`，没有脏检查复用）；唯一复用路径是 `AnyView::cached(style)`（未 notify 即回收上一帧 layout+paint，`window.refresh()` 会击穿）。后果：composer 每键 `Chat` notify = **全窗口重建**（消息区 markdown 元素 + dock + chrome 全连坐）。消息列表是 `list()` 闭包不是实体，吃不到 cached——要隔离只有把轮次包实体 + cached（大手术）。在此前提下，Chat::render 每帧路径严禁 O(会话规模) 的分配：命令表/模型目录/全文拷贝都要门控或借用化（composer 打字卡顿 2026-10-12 排查的落点：slash_commands 深克隆、new_session_default 全目录扫描、set_value 镜像克隆）。

## 焦点

- 焦点分两层：`window.focus(&h)` 直接写 `Window.focus`（`is_focused()` 读的就是它，`focus()` 同值提前 return），而**帧焦点路径**（`rendered_frame.focus`）只由这一帧 prepaint 过的 `set_focus_handle` 写出。给一个这一帧**不在元素树里**的句柄聚焦（组件的 fallback 句柄、滚出视口的虚拟列表行里的输入框）= 路径为空：旧持有者收 blur、新持有者收不到 focus，键事件按 root 兜底，而 `Window.focus` 还留着那个悬空 id（下次渲染又「复活」）。改焦点一律先问「这个元素这帧渲染了吗」。
- `Chat::render` 的焦点级联每帧仲裁「谁持焦」（优先级：对话框/弹层 > 设置 > 文件编辑器 > 终端 > 行内编辑 > composer）。要在**存续期**持焦的行内输入框必须进这条级联；只挂一次性聚焦、靠 blur 收场的写法会被级联末级的 composer 回收逐帧打掉（文件树行内改名「焦点捕捉不到」的根因）。真夺焦（对话框/终端/编辑器/设置）走级联靠前分支，blur 收场照常。
- gpui-component `InputState` 内件首帧才建（`TextInput::ensure_state`）→ 首发聚焦走 `TextInput::focus_soon`（want_focus 在输入框自己的 render 里兑现，内件建好即同帧生效，Zed `focus_filename_editor` 请求同位）；此后兜底用 `has_inner()` + `focus_handle_in()`（内件未建时不要碰 fallback 句柄）。

## gpui-component

- Input `set_value` 全文走 utf16 编辑管线（4.6MB ≈48ms）→ 大文本走 vendor state.rs 的 reset 路径（换 Rope + wrapper.reset）。

## 版本要点（0.2.2）

- `cx.spawn(async move |weak, &mut AsyncApp|)`；prompt_for_paths 返回 oneshot Receiver；HighlightStyle 无 builder（字段赋值）。
- 杂项：prepaint/paint 第 5 参是 prepaint state 非 hitbox；Pixels 字段私有（f32::from/除法）；move 闭包捕获 `&WeakEntity` 参数 E0521（闭包外 clone）；`unbounded()` 返回值不能别解构。
- **vendor 本地改动（升级必须重套）**：`elements/div.rs` `TOOLTIP_SHOW_DELAY` 500→350ms（用户定夺 2026-10-13，350ms 弹出体感更跟手；源码处有注释标记）。
