# pi 扩展生态对接调研

> 调研日期:2026-10-17。对象:vendored pi 1.3.0 的扩展 UI 能力面
> (`vendor/pi/node_modules/@earendil-works/pi-coding-agent`,下称 **$CA**;
> `@earendil-works/pi-tui`,下称 **$TUI**)。
> 背景:战略转变——**PF 不自建扩展生态**(PF 只是 shell,生态应对接 pi),
> 本文回答三问:①CLI 下 pi 扩展能做哪些界面定制;②这些定制是否统一开放
> 接口;③PF 能否据开放接口自动转化为图形化设置/图形化输出。
> 所有 file:line 相对 $CA/$TUI 仓库根,可复查。结论落点:
> [090-插件框架](../模块设计/090-插件框架.md) 降级远期储备(§7)。

## 0. 结论速览

一句话:**PF 的扩展生态边界 = pi RPC 协议面(数据级通道)**。pi 的 `ctx.ui`
9 种调用在 RPC 模式下序列化为 `extension_ui_request`/`extension_ui_response`
JSON 子协议——这是 pi 官方唯一认可的跨前端 UI 契约;PF(040)已 100% 消化,
剩余是消费精度问题。代码级 TUI 定制(自定义组件/壳层替换/转录渲染器)是
pi-tui 进程内对象注入,RPC 下全部 no-op、无协议消息——**PF 拿不到是上游
设计而非 PF 缺陷**。

三个关键事实:

1. **分界线在"数据 vs 代码"**:同一套 `ctx.ui` API,TUI 下渲染原生组件、
   RPC 下序列化为扁平 JSON;而 `custom()`/`setFooter`/`register*Renderer`
   的返回值是代码对象,天然不可跨进程传输。
2. **pi 没有扩展配置 schema 机制**——"自动转化图形化设置"没有原料
   (settings.schema.json 是固定键,扩展读自定义键自解释)。
3. **主题不经扩展注册**——是 schema 化 JSON 资源文件
   (`~/.pi/agent/themes/*.json` 等,文件系统发现),PF 可直接对接。

| UI 能力线 | 开放形式 | RPC 可达 | PF 现状 |
|---|---|---|---|
| 会话交互 UI(ctx.ui 9 种) | 统一 JSON 子协议 | ✅ | 已 100% 消化(040) |
| 转录渲染(register*Renderer 4 种) | pi-tui Component 代码 | ❌ no-op | 数据在事件流,渲染自绘 |
| 壳层替换(custom/footer/header/editor) | pi-tui Component 代码 | ❌ no-op | 不可达 |
| 主题 | JSON 资源文件(theme.schema.json) | 文件系统 | 未对接(七主题自管) |
| 扩展设置 | 无 schema 机制 | — | settings-扩展页纯开关 |

---

## 1. Q1:CLI 下 pi 扩展能做哪些界面定制

扩展 UI 能力分三条线,定制力度依次变大。扩展自行判定运行模式:
`ctx.mode`(`"tui" | "rpc" | "json" | "print"`)+ `ctx.hasUI`(types.d.ts:212-219)。

### 1.1 会话交互 UI(`ctx.ui`,9 种——双模式统一)

接口定义 `ExtensionUIContext`(types.d.ts:72-196);TUI 接线
interactive-mode.js:2060-2100;RPC 接线 rpc-mode.js:83-224。

| 能力 | API | CLI 渲染 | RPC 模式 |
|---|---|---|---|
| 选择对话框 | `select(title, options, opts?)` :74 | SelectList 弹窗(可倒计时/AbortSignal) | ✅ method=`select`,阻塞等应答 |
| 确认对话框 | `confirm(title, message, opts?)` :76 | 确认弹窗 | ✅ method=`confirm`,阻塞 |
| 文本输入对话框 | `input(title, placeholder?, opts?)` :78 | 输入弹窗 | ✅ method=`input`,阻塞 |
| 多行编辑器对话框 | `editor(title, prefill?)` :139 | 全屏编辑弹窗 | ✅ method=`editor`,阻塞 |
| 通知 | `notify(message, type?)` :80 | toast(info/warning/error) | ✅ method=`notify`,fire-and-forget |
| 状态栏文本 | `setStatus(key, text?)` :84 | footer 状态区条目 | ✅ method=`setStatus` |
| 文本 Widget | `setWidget(key, lines[], {placement})` :101 | 编辑器上/下方常驻块(aboveEditor/belowEditor :47-52) | ⚠️ 仅 `string[]` 有消息 method=`setWidget`(rpc-types.d.ts:446-451);组件工厂重载(:102-104)被静默丢弃(rpc-mode.js:123-136) |
| 终端窗口标题 | `setTitle(title)` :119 | 终端 tab 标题 | ✅ method=`setTitle` |
| 预填输入框 | `setEditorText(text)` :135、`pasteToEditor` :133 | 写入主输入框 | ✅ method=`set_editor_text`;`getEditorText()` RPC 恒返回 `""`(:169-173) |

### 1.2 转录渲染定制(`pi.register*Renderer`——仅 TUI 可达)

| 能力 | API | CLI 渲染 | RPC 模式 |
|---|---|---|---|
| 工具卡片 | `ToolDefinition.renderCall/renderResult` :516-518;全局拦截任意工具 `pi.registerToolRenderer(resolver)` :1256(resolver :526) | 交互转录 + HTML 导出中的工具卡片 | ❌ 渲染器不可达;但工具执行照常产生 `tool_execution_*` JSON 事件,客户端自绘 |
| 自定义消息 | `pi.registerMessageRenderer(customType, renderer)` :1250(类型 :1169) | `pi.sendMessage({customType, display:true})` 发出的消息 | ❌ 渲染器不可达;**消息数据(customType+content)在事件流里可拿** |
| 持久条目 | `pi.registerEntryRenderer(customType, renderer)` :1254 | `pi.appendEntry()` 条目(不进模型上下文) | ❌ 同上:数据可达,渲染不可达 |
| Markdown 变换 | `pi.registerMarkdownTransformer(transformer)` :1252 | 转录前字符串变换 | ❌ 不可达(作用于 TUI 渲染路径) |

注:HTML 导出里 custom_message 退化为纯文本 `[customType]: content`
(export-html/template.js:1335-1338)——连 pi 自己的 HTML 导出都消化不了
代码级渲染器,只有工具渲染有 pre-rendered HTML 通道(:1050)。

### 1.3 壳层替换(仅 TUI,RPC 全 no-op)

| 能力 | API | RPC 行为(rpc-mode.js) |
|---|---|---|
| 自定义全屏组件/悬浮 overlay | `custom<T>(factory, {overlay?, overlayOptions?, onHandle?})` :121-131(官方示例能跑 DOOM:examples/extensions/doom-overlay/) | 返回 `undefined`(:152-155),无协议消息 |
| 替换主输入框 | `setEditorComponent(factory)` :175(EditorFactory :67,需继承导出的 `CustomEditor`;示例 modal-editor.ts = vim 模式) | no-op,`getEditorComponent()` 恒 `undefined` |
| 替换 footer | `setFooter(factory)` :111-113(工厂收 `ReadonlyFooterDataProvider`:git 分支+setStatus 条目) | no-op |
| 替换 header | `setHeader(factory)` :115-117 | no-op |
| working 指示定制 | `setWorkingMessage` :86、`setWorkingVisible` :88、`setWorkingIndicator({frames, intervalMs})` :97 | no-op |
| thinking 折叠标签 | `setHiddenThinkingLabel(label?)` :99 | no-op |
| 补全项叠加 | `addAutocompleteProvider(factory)` :141 | no-op |
| 终端原始按键拦截 | `onTerminalInput(handler)` :82 | 返回空 unsubscribe |
| 工具输出展开态 | `getToolsExpanded()`/`setToolsExpanded()` :193-195 | 返回 false / no-op |
| 主题读取/切换 | `theme` getter :179、`getAllThemes()` :181、`getTheme(name)` :186、`setTheme(name)` :188 | `getAllThemes()` 恒 `[]`、`setTheme` 恒 `{success:false}`(:207-216) |

### 1.4 非渲染但 UI 相关注册

- `pi.registerCommand(name, options)` :1231(`RegisteredCommand` :1171-1177,
  含 `getArgumentCompletions` 补全)→ `/` 菜单;**RPC 可经 `get_commands`
  拿列表并远程执行**(rpc-types.d.ts:135-139、393-397)。
- `pi.registerShortcut(shortcut, {handler})` :1233 → 全局快捷键(仅 TUI 有键盘)。
- `pi.registerFlag(name, {type, default})` :1238 → CLI 启动旗标。

slash 命令 handler 签名 `(args, ctx) => Promise<void>` **无返回值通道**——
命令"输出"全靠副作用:`ctx.ui.notify()`(纯文本)、`ctx.ui.custom()`(仅 TUI)、
或 `sendMessage({customType})` + 渲染器(仅 TUI)。

## 2. Q2:是否统一开放接口

**双轨制,分界线恰好落在"数据"与"代码"上:**

### 2.1 数据级通道(有统一协议——真正的"开放接口")

§1.1 的 9 种方法是统一的 `extension_ui_request`/`extension_ui_response`
子协议(rpc-types.d.ts:406-476),全部扁平 JSON(标题/字符串数组/枚举)。
同一套 `ctx.ui` 调用:TUI 下渲染原生组件、RPC 下序列化为协议消息。
这是 pi 官方认可的跨进程 UI 契约,并有规范文档(docs/rpc-extension-ui.md)
与参考客户端(examples/rpc-extension-ui.ts)。

### 2.2 代码级通道(无协议、仅本地)

§1.2/§1.3 的返回类型都是 pi-tui 的 `Component`(tui.d.ts:68-80,
`render(width): string[]` + `handleInput/handleMouse`);扩展直接 import
pi-tui 的类(Box/Text/VStack/ScrollView/SelectList…)写渲染代码,Component
对象只在 pi 进程内的 TUI 树里活。RPC 协议里**没有任何"组件描述 JSON"或
UI 快照消息**;官方文档明确这些在 RPC 下 no-op(rpc-extension-ui.md:12-25)。

**结论:pi 没有(也不打算有)统一的"扩展 UI 描述协议"。对话框/widget 层是
结构化 JSON 协议;自定义组件/布局替换/转录渲染层是纯代码级 API(进程内
对象注入),跨进程不可传输。**

### 2.3 两个例外:主题与设置

- **主题**:不经扩展 API 注册,是 **JSON 资源文件**——`~/.pi/agent/themes/*.json`、
  项目 `.pi/themes/`、或 settings `themes` 数组,遵循
  schemas/theme.schema.json,由 loader 文件发现。扩展只能**消费** Theme
  对象(theme.d.ts:36-70:`style/fg/bg/colors/appearance`)或运行时切换
  (仅 TUI)。→ 对 PF 是好消息:文件系统直读即可对接,不需要协议。
- **设置**:**没有扩展声明式配置 schema 机制**。settings.schema.json 是
  固定键集合,/settings 界面只渲染内置项。扩展只能 `pi.getSettings()`
  (:1286)读合并配置自行解释,或 `registerFlag` 做启动参数——所谓扩展
  设置 = settings.json 里的自定义键,无校验、无 UI。→ 对 PF 是坏消息:
  "自动转化图形化设置"没有 schema 原料(§3.2)。

## 3. Q3:PF 能否自动转化为图形化设置/输出

### 3.1 图形化输出:能,而且已经发生了

PF 的 040 ext_ui 链路已把 §1.1 的 9 种消息 **100% 图形化**:

| 消息 | PF 渲染(crates/app/src/ext_ui.rs) |
|---|---|
| select/confirm/input/editor | 460px 模态对话框(render_ext_dialog :31-210;035 问卷卡优先拦截 :230-237) |
| notify | toast,4s 自动消失(:267-287) |
| setWidget | 输入栏上/下方边框文本块(:252-265;渲染 session/mod.rs ext_widget_rows) |
| setStatus | `aux.ext_status` 维护(**只存不渲染——缺口**) |
| setTitle | 故意忽略(:288-290,**缺口**) |
| set_editor_text | 覆写 composer(:291-294) |
| 微信端 | 9 种全部转文本(remote_control + wxprobe/extui.rs) |

这证明"对接 pi 生态自动获得图形化"的路**已经打通**,剩余是消费精度:
customType 消息(§1.2)有数据无渲染器——PF 可做「已知扩展特判映射表 +
markdown 兜底」;setWidget 是纯文本块——可交互化(§6 M4)。

### 3.2 图形化设置:没有自动转化的原料,走渐进策略

pi 不给 schema(§2.3),"全自动表单"不成立。可行路线(按侵入度递增):

1. **无 schema 的扩展** → settings.json 原始 JSON 编辑(限扩展自定义键,
   不碰内置键;PF 写 settings.json 已有先例:settings/plugins.rs 的
   packages 过滤)。
2. **带惯例 schema 的扩展**(扩展在自己 package.json 里声明 config
   schema——读取惯例而非 PF 注册表)→ 自动渲染表单(复用 settings/widgets
   公共件)。

这是渐进兼容立场:有 schema 给表单、没有给 JSON 编辑,任何扩展都不被
拒之门外,且 schema 放扩展自己包里、由扩展作者决定是否提供——**不是
PF 生态,是读取惯例**。

## 4. PF 现状盘点(简)

链路:发现(pi-link/extensions.rs,复刻 pi discoverExtensionsInDir)
→ 启用(spawn 配方 tools_recipe.rs + session-ext.json 会话清单)
→ RPC 事件(protocol.rs `ExtUiMethod` 9 种全集)→ 渲染/交互(ext_ui.rs)
→ 应答(`ExtensionUiResponse` 按请求 id 关联)。细节见
[040](../模块设计/040-Pi的扩展管理.md) 与
[041](../模块设计/041-扩展装载与工具声明.md),不重复。

已知缺口全集(含本文新视角):

1. `SetStatus` 只存不渲染(aux.ext_status 全仓库无消费点);
2. `SetTitle` 硬忽略;
3. `setWidget` 纯文本块,无交互、placement 两档;
4. customType 消息有数据无渲染策略(通用 markdown 兜底都没有);
5. RPC 无运行时换包——改清单须空闲重绑(send.rs 空闲发送前);
6. 发现口径简化:项目作用域(.pi/extensions、项目 settings)不复刻,
   settings extensions 数组跳过 `!`/`-`/glob/`builtin:` 条目;
7. custom 档 `-ne` 关掉个人扩展发现来源,未像 full 档显式 `-e` 加回(041);
8. 主题无库存来源(041 §2)——get_commands 拿不到主题,但文件系统可扫(§2.3);
9. 无 S2 会话级工具声明名单(041 推荐的下一步);
10. settings-扩展页纯开关,无 per-extension 配置(→ §6 M3);
11. 活跃会话同时只挂一个阻塞弹窗(aux.ext_dialog: Option),多余靠 ext_queue。

## 5. 战略定位

**对接 pi 生态的准确定义:PF 的扩展生态边界 = pi RPC 协议面。**

- 凡走数据级通道的扩展,PF **自动**获得图形化——这是架构保证,不靠
  逐扩展适配;
- 代码级 TUI 定制是 pi 生态内的"**TUI 专属分叉**":认真做跨前端兼容的
  扩展作者本就被迫用数据级通道(扩展自己 `ctx.hasUI` 判定降级)——PF
  的做法是**把这条通道消化得比任何客户端都好**(消费精度、交互品质、
  微信降级),兼容面自然最大化;
- PF 不追代码级分叉:不自建 wasm 插件框架去复刻 pi-tui 的能力(090 降级,
  §7),不对上游 no-op 的接口做模拟。

## 6. 分期路线图

| 期 | 内容 | 规模 | 价值 |
|---|---|---|---|
| **M1 补消费缺口** | ① SetStatus 渲染落位(aux.ext_status 已有数据,候选:输入栏控件行/状态栏);② SetTitle→PF 窗口标题;③ customType 消息兜底渲染(markdown fallback + 已知扩展特判表,数据结构留映射表扩展位);④ custom 档 `-e` 加回个人扩展(缺口 7) | 小 | 数据级通道消费 100% → 逼近无缺口 |
| **M2 主题对接** | 文件系统扫 pi 主题(全局 `~/.pi/agent/themes/` + 项目 `.pi/themes/` + settings.themes,theme.schema.json)→ 映射 PF gpui 色板;`--theme` spawn 杠杆联动。**注意映射损耗**:pi 主题是终端 TUI 色板(fg/bg/样式),PF 是 gpui 语义色槽位,只能做 best-effort 映射 | 中 | pi 生态的主题资产直接为 PF 所用;扩展 setTheme 语义对齐 |
| **M3 扩展设置图形化** | §3.2 渐进策略:schema 惯例→表单 / 无 schema→JSON 编辑;写回限扩展自定义键 | 中 | 补 settings-扩展页"纯开关"短板 |
| **M4 widget 交互化** | setWidget 文本块 → 支持简单交互(按钮/选择);依赖上游协议扩展或扩展端约定(§8) | 远 | widget 从"显示"升级为"操作入口" |

## 7. 090 重新定位(定夺)

**090 自有插件框架(wasm)降级为远期储备,暂不动工。**

- 090 是"PF 自建生态"路线的产物,与"对接 pi 生态"战略直接冲突;
- 但**不废弃**:pi 扩展只覆盖会话区(agent 能力),永远管不到 PF 桌面壳
  自有 UI(文件树/git 面板/编辑器/dock 视图)。**触发条件 = 未来确有
  扩展 PF 桌面壳自有 UI 的需求**——届时 090 的设计资产(wasm 沙箱/
  ElementSpec/市场机制)仍然适用;
- 文档处置:090 头部加远期储备批注 + 指向本文。

## 8. 上游影响评估(仅记录,不动手)

以下方向值得向 pi 上游提案(RPC 协议纯增量,惠及所有客户端):

1. **custom UI 的结构化子集**:声明式卡片描述(类似 090 的 ElementSpec
   思路)作为代码级 Component 的 RPC 降级路径——让 `custom()` 在 RPC 下
   有数据可拿;
2. **主题清单协议化**:get_commands 或专用方法暴露主题列表+路径
   (解 041 §2 缺口;M2 期间文件系统扫描可先行,但 settings.themes 内联
   主题文件系统扫不到);
3. **setWidget 交互元素**:协议里 widget lines 之外增加结构化动作声明
   (M4 的前置);
4. **扩展配置 schema 声明机制**:扩展声明 config schema → 所有客户端
   (不只 PF)受益,图形化设置从"惯例"变"标准"。

提案渠道:pi 仓库 issue,逐条单独提;本文只记录不做。
