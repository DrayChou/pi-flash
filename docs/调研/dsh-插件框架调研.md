# deepseek-harness (dsh) 插件框架调研

> 调研日期:2026-10-12。对象:`D:\github\---harness-tools---\deepseek-harness`(下称 dsh,
> pnpm monorepo,TS)。其插件框架 = vendored **cordis** v4(`vendor/cordis`,
> `@deepseek-ai/cordis 4.0.5-alpha.1`,Koishi 作者的依赖注入/插件框架)之上自建的
> 装载/管理/能力面基础设施。
> 结论服务于 [090-插件框架](../模块设计/090-插件框架.md) 修订(§4);所有 file:line
> 均相对 dsh 仓库根,可复查。

## 0. 结论速览

一句话:dsh 用 cordis 做**进程内全信任**的 TS 插件体系,边界靠"服务接缝"而非权限
声明;值得 PF 抄的是它插件之上的**基础设施模式**(配置闭环、可逆生命周期、安装
安全、扩展点命名),不是运行时本身。

| | dsh (cordis) | PF 090 |
|---|---|---|
| 信任模型 | 进程内 TS,**全信任**,接缝式边界 | wasm 沙箱,**零信任**,manifest 声明式 capability |
| 插件形态 | Service 对象(函数/类/apply) | wasm32-wasip1 core module + JSON ABI |
| UI | 插件自带组件进 slot | 声明式 ElementSpec,宿主闭合组件库渲染 |
| 分发 | npm/pnpm 生态 + registry | Gitee monorepo 索引 + SHA-256 |
| 依赖图 | inject/provide + epoch 等待 | v1 无插件间依赖 |

## 1. cordis 框架核心机制

### 1.1 插件形态:inject / provide / apply

类型定义在 `vendor/cordis/src/registry.ts`。插件三种入口形态(registry.ts:92-133):

```ts
export type Plugin<T = any> =
  | Plugin.Function<T>      // (ctx, config) => any
  | Plugin.Constructor<T>   // new (ctx, config)
  | Plugin.Object<T>        // { apply(ctx, config) }
```

三者共享元数据 `Base<T>`(registry.ts:100-111):`name` / `Config`(standard-schema
校验器)/ `inject`(依赖的服务名)/ `provide`(提供的服务名)/ `intercept`。

- **inject**:声明"必须存在才能启动"的服务。类插件可用装饰器 `@Inject(name)` 写
  静态表(registry.ts:37-60,支持原型链合并);`ctx.inject(deps, callback)` 只是
  语法糖 = `ctx.plugin({ inject, apply: callback })`(:300-302)。
- **provide**:`Service` 基类(`service.ts:42-59`)在构造函数里自动把 self 注册为
  `ctx.<name>` 服务——构造即注册,随所属 fiber 卸载自动注销。关键符号:
  `[Service.init]`(async 生成器初始化,可 yield 清理函数)、`[Service.check]`
  (可用性谓词,决定依赖方是否等待)、`[Service.invoke]`(实例可调用)。
- **resolve**(registry.ts:222-228):三形态统一归约成可执行 callback,作为去重
  身份键。

### 1.2 Context:原型链派生 + isolate 作用域

`vendor/cordis/src/context.ts`。

- root context(`new Context()`,context.ts:71-84):创建自身 Proxy,安装 5 个内置
  服务:`fiber` / `reflect` / `registry` / `events` / `logger`。
- **派生 = 原型链**:`extend(meta)`(:99-107)用 `Object.create` 生成子 context,
  子原型继承父的全部服务,`meta` 作自有属性遮蔽,父不受影响——不是拷贝。
- **isolate**(:121-125):为某服务名建独立作用域标签(覆盖表沿原型链继承),同名
  label 的 isolate 共享作用域。不同 isolate 允许同名服务多实现并存(多窗口/
  多会话场景)。
- **`ctx.<key>` 解析**(`reflect.ts:135-171` Proxy get):沿 fiber 父链向上找当前
  作用域内的实现;**未 inject 就直接读 `ctx.foo` 直接抛错**(144 行,`cannot get
  property "foo" without inject`)——这是 cordis 的核心纪律。去重:同一服务名在同
  一 isolate 作用域只能注册一次(reflect.ts:289-291)。

### 1.3 Fiber 生命周期:effect 原语 + epoch 依赖指纹

`vendor/cordis/src/fiber.ts`(754 行,核心)。

- **状态机**(:147-154):`PENDING(等依赖) → LOADING → ACTIVE → FAILED / UNLOADING
  → DISPOSED`。每个插件实例一个 fiber。
- **effect 是唯一注册原语**(:415-561):`fiber.effect(execute, label)` 立即执行
  body,把返回的 disposer 压入列表;fiber 卸载或手动 dispose 时**逆序执行**。
  一切可逆 API 都是它的语法糖:`ctx.on()`(events.ts:254-260)、`ctx.provide`
  (reflect.ts:277)、`ctx.plugin`(**子 fiber 的 dispose 注册为父 fiber 的
  effect**,fiber.ts:265-297 ⇒ 卸载级联免费获得)。
- **依赖等待 = epoch 字符串比较**(:611-623):依赖满足状态压缩成
  `"uid1:uid2"`(各依赖提供方 fiber 的 uid 指纹),与上次比较,从 INACTIVE 变具体值
  ⇒ `_reload()`,变 INACTIVE ⇒ `_unload()`;**依赖方跟随提供方的生命周期**(提供方
  重载 ⇒ uid 变 ⇒ 依赖方重载)。服务注册/注销时 `notify()`(reflect.ts:314-336)
  遍历所有 fiber 重算。不需要复杂调度器。
- **卸载顺序**:disposer 列表逆序并行执行(:675-696);provide 的注销 disposer 会
  await 所有受影响 fiber 卸载完(reflect.ts:297-303)保证顺序性。

### 1.4 事件系统:五种分发模式

`vendor/cordis/src/events.ts`。分发模式是事件公开契约的一部分:

| 模式 | 等待? | 语义 | 实现位置 |
|---|---|---|---|
| `emit` | 否 | 按注册序观察,忽略返回值 | :194-196 |
| `parallel` | 是 | `Promise.allSettled` 并发,错误聚合 AggregateError | :183-187 |
| `serial` | 是 | 按序 await,遇 bail 停 | :204-209 |
| `bail` | 否 | 按序同步,遇 bail 值停(注册拦截) | :217-222 |
| `waterfall` | 否 | **洋葱中间件**:最后一个参数是内层 `next`;每个监听器包装剩余链,不调 next 即否决/短路 | :234-243 |

waterfall 实现(内建行为作为最内层):

```ts
waterfall(...args: any[]) {
  const cbs = this.dispatch('waterfall', args)
  const inner = args.pop()          // 内建行为作为最内层
  const next = () => { const cb = cbs.shift() ?? inner; return cb(...args) }
  args.push(next)
  return next()
}
```

类型:方法签名经 `declare module` 给 `Context` 混入(events.ts:34-109,运行时靠
`mixin('events', [...])`);事件名经 `interface Events` 声明合并(:329-352),插件
写 `declare module { interface Events { 'hmr/change'(url: string): void } }` 后
`ctx.on` 的参数表自动联动。框架自身扩展点(配置插值 `internal/config`、监听器
注册 `internal/listener`、更新 `internal/update`)全用这套事件实现——吃自己狗粮。

### 1.5 加载:loader / include

`vendor/loader/src` + `vendor/include/src`。

- 抽象:`EntryTree`(tree.ts:7)→ `EntryGroup`(group.ts:6,`EntryOptions[]` 列表)
  → `Entry`(entry.ts:43,单插件节点)。JS 来源支持 `cordis:` 内建 / Node 内部模块
  / 动态 `import()`(tree.ts:112-129);导入后 `unwrapExports` 处理 ESM/CJS 互操作
  再 `registry.plugin()`(entry.ts:223-238)。
- **YAML include**(`include/src/index.ts`):顶层是 entry 数组;`!!js` 是自定义
  YAML tag(:9-23),解析成表达式节点,**延迟**到该插件 fiber 激活前求值——
  config 插值与 `disabled` 判断都走它(entry.ts:76-95;`new Function('ctx', 'with
  (ctx) { return eval(expr) }')`,loader/config/utils.ts:5-9)。
- **配置 diff**:`EntryGroup.update(config)`(group.ts:48-65)按 id 对比:新增/存在
  ⇒ create,消失 ⇒ remove;仅 config 变 ⇒ volatile 短路或重启(entry.ts:118-157)。
- **自卸载回写**:loader 监听 `internal/plugin`(loader/index.ts:129-166),插件被
  外部 dispose 时自动把 entry `disabled: true` 并 `tree.write()` 持久化。

### 1.6 HMR:换码不丢状态

`vendor/hmr/src/index.ts`。chokidar 监听 → 沿模块依赖图分类 accepted/declined
(:196-249)→ 备份 ESM loadCache / CJS require.cache → 清缓存重导入,失败回滚
(:312-340)。**状态不丢的关键**(:353-360):

```ts
const reload = (plugin: any, runtime: Plugin.Runtime) => {
  for (const oldFiber of runtime.fibers) {
    const fiber = oldFiber.parent.registry.plugin(plugin, oldFiber._config, ...)
    fiber.entry = oldFiber.entry              // 新插件继承旧 entry 与原始 config
    if (fiber.entry) fiber.entry.fiber = fiber
  }
}
```

即:用旧 fiber 的原始 config 在旧父 context 上挂新代码;旧插件统一 dispose——由于
服务按 inject 重新绑定,其他插件的状态/监听/树全部无感。

### 1.7 配置:schemastery + 变更响应链

`vendor/schemastery/src/index.ts`:schema 是可调用对象,链式 meta(default/required
/role/hidden/comment…),经 `~standard` 属性适配 standard-schema 协议(:290-307);
`simplify`(:421-458)写回时剔除等于默认值的字段,配置文件保持精简。插件声明
`static Config: z<Config>`,fiber 激活前 `resolveConfig` 校验,失败抛带路径的
`ValidationError`。

**变更响应链**(config 更新的固定管线):

1. `fiber.update(config)`(fiber.ts:736-753):schema 校验 → `internal/update`
   waterfall → 默认 `next()` 里 restart(dispose 后以新 config 重载);
2. **插件可否决/改写**:不调 `next()` 即否决重启(include 对纯 patches 变更自己
   持久化热应用,include/src/index.ts:190-201);
3. **volatile 免重启**:schema 标 `.volatile()` 的字段输出稳定引用
   (cosmokit/src/volatile.ts),`Entry.update` 检测"仅 volatile 变化"走
   `_commitVolatile()`(entry.ts:159-197)原地提交并发 `loader/volatile-update`,
   不重启 fiber。

## 2. 装载与管理基础设施(cordis 之上)

### 2.1 插件包格式:`dsh.*` manifest 命名空间

`packages/util/package-manifest/src/types.ts`:

- L8-27:标准 npm 字段 + `icon`(≤256KiB)+ `engines` + `dsh`;
- L30-39 `DshManifest`:`dsh.manifestVersion` / `dsh.bundle` / `dsh.profile` /
  `dsh.client`——一个包可同时扮演多角色;
- L57-66:`engines.dsh` 是 SemVer range,**兼容性门槛**;
- L69-78:bundle = `{ patch: string | string[] }`(指向包内 cordis.patch.yml);
  profile 目录自己的 package.json 用 `{ bundles?: string[] }`;
- L81-94 `DshClientManifest`:`{ platform, inject?, immediately?, external? }`
  (inject 是信息性包依赖,非 cordis 服务注入)。

入口约定:Host 侧 = 包 `main`(默认导出 cordis 插件/Service);浏览器侧 = `./client`
导出(约定导出 `apply(ctx)` + `inject` 数组)。消费方 `packages/client/modules/src/
index.ts:836-847` 扫描 Loader 活动条目,声明 `dsh.client` 必须有 `./client` bundle。

### 2.2 plugin-manager:安装/启停与持久化

`packages/boot/plugin-manager/src/index.ts`(`ctx.pluginManager`,L170-189,
`inject = ['loader','profileContext']`):

- `protectedModules`(L68-78):管理链自身模块不可被禁用(`management-required`);
- Remote 方法:`listPlugins`(:257)/`listBundles`(:281)/`inspect`(:373,`pnpm view`
  探测)/`setPluginEnabled`(:457)/`setBundleEnabled`(:475)/`installBundle`
  (:496-610)/`cancelInstall`/`waitForInstall`(:617-637)/`removeBundle`(:644)。

**持久化全在 `$DSH_HOME/profiles/<name>/`**:

| 文件 | 内容 |
|---|---|
| `package.json` | 依赖 + `dsh.profile.bundles` 顺序表(bundle 启停写这里,src/index.ts:763-783) |
| `cordis.patch.yml` | 用户 override 行:`{id, disabled}`(单插件启停)+ id 定向 config override;`src/patch.ts:14-43` 用 `yaml.parseDocument` 保注释,findLast 匹配同 id 行改写,没有则 append |
| `compatibility.json` | 版本豁免:`"pkg@exact-version": ["exact-dsh-version"]`,授权需 `acceptRisk`(app-boot/src/profile-compatibility.ts:10-141) |
| `pnpm-workspace.yaml` | `nodeLinker: hoisted`(所有插件共享同一 cordis 实例)+ pnpm build-script 审批 |

并发安全:所有变更走 `withFileLock(profile 的 package.json)`(:829);变更检测对 4
个文件做指纹(:850-858)。

**安装流程**(:496-610):git 连通预检 → registry 计划逐个 `pnpm add` → 快照
`package.json`+`pnpm-lock.yaml`,**失败/取消即还原**(:81,:745-761)→ 校验
`dsh.bundle` 声明与 `engines` 兼容 → selectBundle → HMR `reconcileProfilePatches`
热应用(无 HMR 则报 `restart-required`)。进度走事件 `plugin-manager/install-log`
/ `install-state` / `changed`;掉线后 `waitForInstall` 找回结果。

### 2.3 boot 启动序列:一切皆 patch 层

`packages/boot/app-boot/src/`:

- profile 布局(profile.ts:36-40,:169-195):`profiles/<name>/`;模板
  `acp/web/headless/sdk/...` 各自带 bundle 列表;`OPTIONAL_BUNDLES` 出厂自带默认关;
- **层序**(profile-context.ts:63-75):bundle 层(模板序)→ profile
  `cordis.patch.yml` → home 全局 patch → CLI `--patch` overlays → 遥测开关;
- **双锚解析**(profile.ts:713-724):bundle 目录先从 dsh 安装包解析、再从 profile
  `node_modules` 解析——in-box bundle 永远来自运行中的安装(供应链一致);
- `loadProfileDirectory`(:739-773):按 `dsh.profile.bundles` 顺序逐个读
  `dsh.bundle.patch` → 兼容预检 → 加载;坏 bundle 进 `skippedBundles` 不炸启动;
- `boot()`(index.ts:975-1041):`new Context()` → `ctx.plugin(Loader)` → prepare
  → 根 Include 以固定 id 挂载(**根 `cordis.yml` 恒为空数组,组合全部由 patch 层
  合成**,每次启动重写防固化,apps/cli/src/profile-boot.ts:81-88)→ `loader.await()`
  → `auditStartupEntries` 校验必需条目表。

### 2.4 管理面:一套服务四种消费

同一 `PluginManager` Remote 服务(Typert 协议,`@Remote` 方法装饰器 + 生成 codec)
暴露给:(a) Web Plugins 页(读写,事件流进度,`packages/client/ui-plugin-manager`);
(b) Settings 插件清单(只读,`packages/host/plugin-inventory`:输出
`{entryId, enabled, fiberPhase, meta}` 每次直读 Loader 无缓存);(c) agent 的
`plugin_manager` 工具(Creator preset 启用,需 danger-full-access 或逐次审批);
(d) CLI `dsh plugin`。

### 2.5 配置持久化:三层值 + 乐观锁

- **写入器** `packages/boot/config-editor/src/index.ts`:`configuration()` 把每个
  活动条目拆成 `inherited`(bundle 层合成值)与 `override`(用户层最后一条);
  `edit()` 在 profile 锁内保注释地更新/追加该 id 的 override 行,再热应用;
- **表单投影** `packages/settings/settings/src/index.ts:302-340` `describe()`:
  从每条目的 schemastery Config 生成 `{ns, schema, revision, base, user, value}`
  (base/user/value 三层 + 密钥脱敏);`update/replace/mutate`(:347-375)带
  **revision 乐观锁**,最终走 configEditor;
- 浏览器侧 `packages/client/ui-settings/src/client/config-form.ts:230-340` 按
  entryId 缓存表单控制器,经 `remote.settings` 读写。

## 3. 能力面:插件能扩展什么

> 总矩阵(生成物,最权威):`docs/event-producer-consumer.md`(每行标 Mode/声明处/
> 分发者/监听者);接缝总表 `docs/capability-seams.md`。

### 3.1 核心事件矩阵

**Agent loop**(`packages/core/agent/src/runtime-types.ts:245-395`):

| 事件 | 模式 | 用途 |
|---|---|---|
| `agent/created` | serial | 初始化;throw 可否决创建 |
| `agent/pre-step` | **waterfall** | 核心拦截点:拒绝 step 或替换消息 |
| `agent/request` | **waterfall** | 替换冻结的 LlmCallConfig(换模型/参数) |
| `agent/request-error` | waterfall | 重试决策 |
| `agent/assistant-stream` | emit | 瞬态流式 chunk |
| `agent/turn-stopping` | serial | 收尾检查点;可 steer 继续 |
| `agent/status` / `disposed` / `error` / `inbox/*` | emit | 观察 |

**工具管线**(`packages/core/tools/src/index.ts:143-210`):

| 事件 | 模式 | 用途 |
|---|---|---|
| `tools/pre-execute` | **waterfall** | allow/deny/cancel/ask(`ask` 走 ctx.approval) |
| `tools/execute` | **waterfall** | around 包装(timeout/retry/metrics) |
| `tools/post-execute` | waterfall | accept(可替换)/block(纠正反馈)/附加上下文 |
| `tools/result` / `tools/change` | emit | 观察最终结果 / 工具集变化 |

**其他**:`llm/stream`(**waterfall**,包装/短路模型流,`packages/llm/llm/src/index.ts:75`);
`system-prompt/assemble`(waterfall,`packages/core/system-prompt/src/index.ts:31`);
`session/created|disposed|event|flush`(`packages/core/session/src/index.ts:43-83`,
`session/event` 是可回放事务日志);`fs/write-intent|edit-intent`(waterfall,变更
闸门);`approval/request`(waterfall);持久事件词汇 `SessionEventMap` 也可声明合并
扩展(插件可追加 `plugin:` 前缀审计事件)。

### 3.2 工具注册(ctx.tools)

`packages/core/tools/src/index.ts`:`ToolRuntime extends Service`;API:
`register(ToolDefinition)`(:1069,scoped 遮蔽全局)/ `restrict(filter)`(:1103)/
`guard(g)`(:1142,**单调守卫:只能 deny,不能 force-allow**——"listener ordering
cannot turn a denial back into permission")/ `presentAs(mode)`。

`ToolDefinition`(:224-300)= schema + 执行 + 展示钩子:`execute(args, exec)`、
**必填 `output`**(输出 schema 强制校验,违反 → INVALID_TOOL_OUTPUT)、
`presentCall/presentResult`(纯函数,可回放的 UI 卡片视图)、`timeoutMs`、
`isConcurrencySafe`。类型化助手 `defineTool<S,O>`(schema.ts:554)用 DSL 编译出
JSON Schema + TS 类型推断。

### 3.3 UI 扩展点:slot 命名契约

`packages/client/ui-slots/src/index.ts`(client 侧纯核心):

- 契约表全部声明合并:`SlotMap`(:26)/`SlotFactoryMap`/`LocaleNamespaceMap`;
- `SlotKind = 'single' | 'list' | 'keyed' | 'chain'`(:101);`SlotScope = 'root' |
  'session-maybe' | 'session'`(:104);
- `SlotCore.register()`(:1157-1312):一次调用同时贡献组件 + **声明子 slots**
  (children 表是渲染授权,"声明即认领")+ store seat + inject hooks + locale
  命名空间;**shadowing 按 priority(lowest renders)**,chain slot 用 selector
  竞选;返回 disposer 级联注销子声明;
- slot key 清单(各包合并):`sidebar`/`main`(keyed)/`rightbar`、
  `conversation.composer`(chain)、`conversation.input.*`、`settings.section`、
  `settings.plugins.tab`、`tool.call.toolview`、`plugins.item` 等。

client 插件加载:包声明 `dsh.client` 导出 `./client` 浏览器 bundle,`ctx.clientModules`
增量扫描组合启动图注入 index 页。

### 3.4 命令 / 技能 / prompt

- 斜杠命令 `ctx.commands.register(CommandDefinition)`(`packages/interaction/
  commands/src/index.ts:285`;name 校验 `^[a-z][a-z0-9_-]*$`);**命令永不发给
  模型**(human 通道);执行写 `command/run|done` 审计事件;
- 技能 `ctx.skills.register / registerProvider`(`packages/skill/skill/src/index.ts`):
  `SkillInvocationPolicy` 分 `modelInvocable` / `userInvocable` 双通道;同名胜者按
  rank 决出;
- prompt `ctx.systemPrompt.section/context/tools/variable`(`packages/core/
  system-prompt/src/index.ts:474-557`):section 带 `order`(中央序位表 :131-165),
  `complete: true` 可整段接管该 scope 的 system prompt。

### 3.5 MCP / Remote / 跨进程

- `ctx.mcpResources.register(server, provider)`(`packages/mcp/mcp-resources/src/
  index.ts:80`):首个 provider 出现时自动注册共享工具 `list_mcp_resources` 等并挂
  prompt section;MCP 连接插件把连接的 resources 注册进来,工具直接 `ctx.tools.register()`;
- Remote 服务栈:`typert`(`packages/typert/protocol/src/index.ts`)——Host 侧
  `class X extends TypertRemoteService` + `@Remote` 方法装饰器(**只有标记的方法
  进 wire**),Client 侧 `ctx.remote.<namespace>` 普通对象;构建期从 ts.Program 生成
  codec,参数经生成 schema 验证。`svc_xxxController` 族(settingsController/
  sessionController/terminalController…)都是这个模式。

### 3.6 安全模型:无权限 manifest,接缝强制

**没有插件权限声明清单**——能力面 = 它 inject 的服务 + 监听的事件 + Config schema。
边界在服务接缝处强制:

- **审批接缝** `ctx.approval`:`tools/pre-execute` 的 `ask` 决策经 `approval/request`
  waterfall 由 UI 应答;**absent/不可应答 fail-closed 为 deny**;审计走
  `approval/asked|decided` 日志事件;
- **单调守卫**:`tools.guard()` 的 deny 不可被后续 listener 翻转成 allow;
- **沙箱接缝** `ctx.sandbox + ctx.sandboxPolicy`:`read-only`(默认 fail-safe)/
  `workspace-write` / `danger-full-access`;bash 与 fs 一致受限("cannot confine to
  different roots");
- **wire 边界**:Client 只能调 `@Remote` 方法,参数生成 schema 验证;settings/
  credentials 层强制密钥脱敏;`ctx.workspaceFiles` 限定 workspace root(lstat +
  containment + stat re-check);
- **动态插件**:`ctx.dynamicCordisRunner` 把运行时第三方 cordis 包 host 半部放进
  **vm 沙箱**执行(定义注册表 + 请求往返,`packages/extensions/cordis-host-runner`);
- **agent 自省**:`tool-cordis` 两个只读工具让 agent 查运行时 API(定义注册表 +
  inspect registry)——agent 可以自己发现扩展点。

## 4. 对 PF 090 的借鉴结论

**采纳六条**(已落入 090 修订,见该文档 2026-10-12 拍板记录):

1. **schema 驱动的插件配置闭环**(源自 §1.7/§2.5):`plugin.toml` 内嵌 `[config]`
   字段 schema → 宿主通用渲染设置表单 → 用户值落 `<id>/config.json` 只存非默认值
   → 生效管线固定为"校验 → volatile 变更原地热更 / 否则受控重启该插件实例,其他
   插件无感"。对应 090 的 enable/disable 本来就不重启应用,成本极低。
2. **effect 式可逆注册表**(§1.3):load() 每个注册项带句柄,PluginRegistry 登记
   (plugin_id, kind, label),禁用/卸载按注册逆序回收;注册表兼作诊断数据
   ("谁占用了什么")。
3. **升级旁路回滚**(§1.6 回滚思想):新 wasm 先旁路独立 Store/Instance 跑 load(),
   成功才替换;失败保留旧版本(旧字节在内存 = 回滚点)。
4. **版本豁免名单**(§2.2):双版本检查不过默认拒装,设置页可显式"接受风险启用"
   落 `compatibility.json`,可见可撤销;安装/更新后台任务化 + 进度可取消
   (install-state 事件思想)。
5. **slot 命名契约**(§3.3):未来 UI 开放面统一为 `<域>.<位置>` 命名 + kind
   (single/list/keyed/chain)+ priority 竞选;先登记词条再实现,避免每个新开放面
   各发明一套注册/覆盖/排序规则。
6. **agent 写插件**(§3.6 tool-cordis 自省):PF 本就是 agent 应用,把插件 SDK
   文档包成 pi skill,让 pi 帮用户生成插件项目——远期项。

**不照搬**(及理由):

- 进程内信任模型与接缝式安全——PF 零信任场景下 capability manifest + wasm 沙箱
  是更强边界;但吸收两条纪律:宿主服务参数一律 schema 校验;未来敏感服务的 deny
  必须单调(不允许后注册者翻转拒绝);
- inject/provide 插件间依赖图——v1 无此需求;若将来 `service:*` 开放给插件互相
  提供,epoch 指纹算法(§1.3)是最省事的实现;
- patch 层组合 / YAML loader / `!!js`——为 JS 组合树服务,PF 无 JS 插件;
- HMR 全套——wasm 载入毫秒级,重启实例代价低,旁路回滚已覆盖安全需求;
- typert Remote 栈——单进程桌面无跨进程 UI 需求。

## 5. 复查路径索引

| 主题 | 路径(相对 dsh 仓库根) |
|---|---|
| 插件/类型定义 | `vendor/cordis/src/registry.ts`、`service.ts` |
| Context/isolate | `vendor/cordis/src/context.ts`、`reflect.ts` |
| Fiber/effect/epoch | `vendor/cordis/src/fiber.ts` |
| 事件五模式 | `vendor/cordis/src/events.ts` |
| loader/include | `vendor/loader/src/`、`vendor/include/src/index.ts` |
| HMR | `vendor/hmr/src/index.ts` |
| schemastery | `vendor/schemastery/src/index.ts` |
| 包 manifest | `packages/util/package-manifest/src/types.ts` |
| plugin-manager | `packages/boot/plugin-manager/src/index.ts`、`src/patch.ts` |
| boot/profile/patch 层 | `packages/boot/app-boot/src/profile.ts`、`profile-context.ts`、`index.ts` |
| 配置编辑/表单 | `packages/boot/config-editor/src/index.ts`、`packages/settings/settings/src/index.ts` |
| UI slots | `packages/client/ui-slots/src/index.ts` |
| 工具管线 | `packages/core/tools/src/index.ts`、`docs/tool-execution-pipeline.md` |
| agent 事件 | `packages/core/agent/src/runtime-types.ts`、`docs/agent-lifecycle.md` |
| llm/stream | `packages/llm/llm/src/index.ts` |
| typert Remote | `packages/typert/protocol/src/index.ts`、`docs/api-gateway.md` |
| 事件总矩阵 | `docs/event-producer-consumer.md`、`docs/capability-seams.md` |
| cordis 入门文档 | `docs/cordis-primer.md`、`docs/cordis-tutorial/` |
