# 消息区文本划选：换用 gpui-component TextView 评估

日期：2026-10-15。需求：agent 输出目前只能整条复制（复制按钮），希望像普通文本一样鼠标划选部分内容。
结论前置：**可行，建议换用，预估 3–5 天**（对比自研选区 1–3 周）。

## 1. 社区方案盘点

| 方案 | 状态 | 对 PF 可用性 |
|---|---|---|
| gpui-component `TextView`（discussion #1147 的官方答案，PR #1169） | 已合入 | **vendor 0.2.0 副本已包含**，与 vendored gpui 0.2.2 同栈，零升级 |
| `gpui-base` crate（窗口级划选：跨元素、双击选词、三击选行、自动滚动、触摸） | 活跃（0.7.3，2026-10-12） | **不可用**：绑死 `gpui-pre 0.3.9`（Longbridge 的 gpui 分支），与 vendored zed gpui 0.2.2 不兼容 |
| Zed Agent Panel | — | 业界参照：也是"每条消息复制按钮 + 消息内划选"，无跨消息划选（issue #58319） |

选区范围：一个 TextView = 一条消息。跨消息划选做不了（那是 gpui-base 的能力），但"复制这条回复里的一段"正是需求场景。

## 2. TextView 机制（vendor 0.2.0 实读）

- 构造：`TextView::markdown(id, src, window, cx).selectable()`；内部 `window.use_keyed_state("{id}/state")` 持久化解析结果。
- 解析：`markdown` crate v1 `to_mdast(ParseOptions::gfm())`（GFM 表格/任务列表/删除线），**首次同步解析**（首帧即有内容），后续文本变更走通道 → 后台线程 **200ms 防抖**重解析 → notify 父视图重渲。流式期间重解析不在渲染主线程上，比现状（流式每帧同步重渲 markdown、跳过代码高亮 v56-3 c15）更优。
- 代码块：`SyntaxHighlighter`（tree-sitter，`tree-sitter-languages` feature PF 已开启）；配色读 `cx.theme().highlight_theme`。
- 选区：
  - 命中是**逐行感知**的，不是矩形框：`point_in_text_selection`（text/inline.rs:368）首行取起点右侧、末行取终点左侧、中间行全选，带单测。
  - 状态存 TextViewState（窗口坐标两点），`Inline` paint 时逐字符 `position_for_index` 算选区、`paint_selection` 画三段 quad（首行/中段/末行）。
  - 文本重解析到达、TextView 尺寸变化 → 选区清除（浏览器同语义）。
  - 有选区时链接点击被抑制（inline.rs:344 `if !is_selection`），不会误开链接。
- 复制：`ctrl-c`/`cmd-c` 键绑定在 `TextView` key context（text_view.rs:32）。
- 主题：全部读 gpui-component `Theme` global——PF 已有 `sync_gpui_tokens`（appearance/mod.rs:58）逐 token 映射，含 `selection`、`highlight_theme`、`foreground`。**主题差距≈0**。

## 3. 差距清单（换用前必须解决/知晓）

1. **链接回调缺失（需 vendor 补丁）**：TextView 链接点击硬编码 `cx.open_url`（inline.rs:359、node.rs:585）。PF 需要"本地文件链接走应用内编辑器"（现有 `markdown::set_link_target` 机制，turn.rs:257）。补丁：给 LinkMark/NodeContext 加可选回调或在 PF 侧包一层全局链接路由。小。
2. **复制拼接缺分隔符（需 vendor 补丁）**：`Node::selected_text`（node.rs:187）把各块选中文本直接 concat，**段与段之间无 `\n`**，跨段复制会粘连。补丁：块级边界插入换行。小，但用户可感知，必须修。
3. **Ctrl+C 可能不生效（需验证/补丁）**：键绑定挂在 TextView 的 focus context 上，但 TextView 从不主动 take focus（只有 `track_focus`）；焦点在 composer 时 Ctrl+C 走不到它。方案：PF 侧在 Chat 层挂 Ctrl+C 兜底（栈顶 TextView 有选区 → 复制），或补丁 mouse-down 时 focus。小。
4. **代码块缺语言标签和复制按钮（需 vendor 补丁或接受降级）**：TextView CodeBlock 只有高亮文本（node.rs:357），无 v56 的语言标签/复制按钮/内部滚动。mono 字体硬编码 `Menlo, Monaco, Consolas`，可用 `TextViewStyle.code_block`（StyleRefinement）覆写或补丁走 PF `MONO_FAMILY`。中。
5. **渲染管道改造**：`render_msg`（turn.rs:235）目前无 `window`/`cx` 参数，而 `TextView::markdown` 构造需要两者。需把 window/cx 下传进消息渲染链。机械改造，中。
6. **内存模型变化**：现状渲染无状态（每帧重建元素）；TextView 每条消息常驻一棵解析树（keyed state）。长会话内存上浮；会话切换时 keyed state 清理需验证（key 里建议带 session id）。
7. **性能观察点**：每个 `Inline` 每帧注册 1–2 个 window 级鼠标监听；有选区时拖拽帧每个可见 Inline 逐字符 `position_for_index`。百消息级会话预计可用，需 pif-ui 实测帧时间。上游维护者自己也说"Selection 实现仍需打磨"（2025-09）。
8. **双解析器并存**：聊天走 TextView（mdast），编辑器 md 预览仍走 PF 自研解析（editor/markdown/mod.rs）。渲染细节可能漂移（如用户消息 HTML 不渲染的 c19 规则只在自研渲染器里）。用户消息**本期不动**（气泡/技能折叠逻辑特殊）。
9. **图片**：TextView `ImageNode` 支持 url/宽高；PF 用户消息图片已独立缩略图行（v58），agent 消息内嵌图片 240 上限规则需在 TextView 下核对（本期 agent 消息少见图，可后置）。

## 4. 工作量分解

| 批次 | 内容 | 估计 |
|---|---|---|
| 一期 | render_msg 管道下传 window/cx；agent 消息换 `TextView::markdown().selectable()` + TextViewStyle 映射（字号/段落间距/code_block refinement/mono 字体）；补丁①链接回调 + ②selected_text 换行；Ctrl+C 兜底 | 2–3 天 |
| 二期 | CodeBlock 语言标签/复制按钮对齐 v56；流式帧时间与长会话内存 pif-ui 实测；keyed state 清理 | 1–2 天 |

## 5. 验证口径（pif-ui）

- 划选跨段 → Ctrl+C → 剪贴板内容含块间换行；
- 划选含链接文本 → 松开不触发链接；纯点击链接 → 走 PF 内部路由打开编辑器/浏览器；
- 流式输出中拖拽划选不崩、流结束后选区语义正确；
- 七主题下选区高亮色/代码块配色正确；
- 长会话（100+ 消息）帧时间回归对照。

## 6. 结论

自研选区的主要成本（文本模型、命中测试、高亮绘制、复制）TextView 已全部解决且已在本仓 vendor 内；剩余工作是适配与 3 个小补丁，走本仓成熟的重套补丁流程（docs/agents/gpui.md）。建议立项，按两批落地。
