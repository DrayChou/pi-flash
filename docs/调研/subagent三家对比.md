# subagent 三家对比(pi-web / ZCode / dsh)

> 调研日期:2026-10-18。动机:用户裁定 pi-web 版太简陋、ZCode 版不可取,要求
> 把 dsh 加入对比、三家拉平。**本文只归档研究,不写设计**——[044](../模块设计/044-subagent宿主设计.md)/
> [045](../模块设计/045-subagent管理.md) 保持原样;文末修订建议是否采纳由设计侧另行决定。
> pi-web / ZCode 材料承自 045 已有沉淀;dsh 材料为本次新增(0.2.1-rc.2,
> 浅克隆 `D:\github\deepseek-harness`,file:line 可复查)。

## 0. 结论速览

三家是三种**结果哲学**,不可互相评判:

| | 结果哲学 | 一句话定性 |
|---|---|---|
| pi-web | 结果即取回(`get_subagent_result` 二次取) | **简陋的根因**:与 pi nested-call 打架,2/3 代码在补偿"结果不是工具结果"这个模型——方向错,打磨无意义 |
| ZCode | 结果即工具结果(阻塞 await) | **乱在形态**:能力大致等价,但复杂度以"十余环节 + 注释级不变式"存在(runner.ts 2100 行、product-projection.ts 5500 行),不可维护 |
| dsh | 结果即 activation(工具立即返回 `childId`,工作归 activation 所有,完成走通知) | **强在维度**:把 subagent 当分布式系统做——activation 所有权、投递模式、能力协商、稳定失败码、容量不排队;复杂度全长在类型与契约上 |

代码量:pi-web ~2000 行(2/3 补偿性);ZCode ~7600 行;dsh 正式包
`packages/subagent/*/src` 共 10112 行(另 experimental/agent-team 2179 行)。

## 1. 组件定位(先分清两层)

dsh 的 subagent 是**正式组件**(`packages/subagent/`,9 包:seam、spawn/fork
两个进程内后端、acp/codex/claude-code/dsh-sdk 四个进程外后端、委派工具、
控制工具);agent-team 是**官方自带但实验性**(`packages/experimental/agent-team`,
包名 `@deepseek-ai/dsh-experimental-agent-team`,自述"在实验性 Team 组合中
运行具名 teammate";同组配套 agent-team-profile、client-ui-agent-team、
tool-agent-team)。rc.1 刚做"直投 Inbox"破坏性重构,接口未稳。

## 2. 逐维对比

| 维度 | pi-web | ZCode | dsh |
|---|---|---|---|
| 架构位置 | Next.js 宿主内(`globalThis.__piSessions` 扛热重载) | 宿主编排器,child 启动穿十余环节 | 独立包组三层:服务(seam)/后端/工具;**工具镜像 provider 生命周期**,无悬空 |
| 结果模型 | 二次取回 + consumed 标记 + 通知去重 | 阻塞 await | `startActivation()` 立即返回 `{childId,…}`;规范工具结果 `{kind:'activation', subagentId}`,**无执行 Promise 无最终输出** |
| 后端体系 | 无 | 无 | **具名 provider 注册表**(spawn/fork/acp/codex/claude-code/dsh-sdk);能力声明协商:`depthLimit`/`agentOptions`/`prepareContinuable`,不满足**显式拒绝而非静默降级**(`UNSUPPORTED_CAPABILITY`) |
| 生命周期 | 一次性 | 前后台混在环节里 | **本地/外部二分**:本地 spawn/fork 可续(`send_message`、冷恢复、驻留);外部(ACP/CC/Codex/SDK)一次执行拒续 |
| 投递模式 | — | — | `delivery:'parent'`(完成通知回父)⊥ `'caller'`(工作流等待,无通知) |
| 中断语义 | 粗 | 粗 | 三层:`interrupt_agent` 对外部=永久停;本地=停当前轮次、队列保持、后代继续;对已结束者=空操作容忍 |
| 容量/深度 | 无 | 有,埋在环节里 | **二者分离**:`maxActiveSubagents`(默认 8,在线+创建中名额,**耗尽即拒不排队**——防"等待后代的父又等待自己占用的名额"级联死锁;调低不杀已有)⊥ `maxDepth`(数值深度要求 provider 具备 `depthLimit` 能力;`'provider-managed'` 外包;达限时工具仍可见,启动时拒) |
| 上下文继承 | 截断 50k 字符 | 有 | 工具描述**随继承方式动态措辞**:fresh="does not see this conversation",fork="does not see the current in-flight turn"——模型无法幻觉上下文 |
| 失败语义 | ad hoc | 注释级不变式 | 稳定分码:`ACTIVATION_LIMIT_REACHED`/`DUPLICATE_CHILD`/`UNSUPPORTED_CAPABILITY`/`CONTINUATION_UNAVAILABLE`/`UNAUTHORIZED` |
| 子级个性化 | profile | profile | preset 体系:`agentPreset` 默认 + `allowedAgentPresets` 白名单(模型可选枚举);仅进程内 spawn 支持;冷恢复按 descriptor 还原 persona/工具限制 |
| 模型选择 | — | — | `modelSelectionSettings` + `list_subagent_models` 工具;路由策略记录进 Session 继承;provider 不支持则拒配 |
| schema 膨胀控制 | — | — | 父与可续子**继承同一 `send_message` 定义**,子级通信不新增专属工具 schema |
| 发现/观测 | 自建 map | 自建投影 | `subagentCatalog` projection;`listChildren` O(D) 事件序、外部条目为叶;不入子级日志 |
| 结算语义 | — | — | 本地活动+待处理输入+所拥有后代全部结束后关准入;**空闲停放子树驻留但不延迟宿主完成**;结构化结果暂存期间拒绝继续输入(防结果漂移),拒绝后重开 |

## 3. dsh 设计精华(与结果模型无关、普适可搬的 6 条)

1. **provider 能力声明协商**:后端能力不满足→稳定失败码显式拒绝,绝不静默降级
2. **容量⊥深度分离**:并发上限=在线名额、耗尽即拒不排队(理由:防级联等待死锁),与轮次/深度预算解耦
3. **中断三层语义**:外部永久停 / 本地停轮次留队列留后代 / 对已结束者空操作
4. **稳定失败分码表**:工具错误结果可预期、可编程
5. **工具描述动态措辞**:按上下文继承配置生成,防模型幻觉父子上下文
6. **schema 不膨胀**:控制类工具复用同一 schema 定义,子级不新增专属工具

**不可搬的 1 条**(与 pi 原语冲突,当时判断):activation+通知的结果模型。

> **后记(2026-10-18 决策)**:最终采用 dsh-subagent 框架结构。上条修正——
> activation 模型经 dsh 自带的 `delivery` 二分(`caller`=await 即工具结果 /
> `parent`=完成通知)并入框架,与 pi nested-call 不冲突:v1 取 caller,
> v2 后台取 parent。落地见 [045(修订版)](../模块设计/045-subagent管理.md) §1–§3。

## 4. 证据索引(dsh 侧,0.2.1-rc.2)

| 事实 | 出处 |
|---|---|
| seam 服务/注册表/结算/源码地图 | `packages/subagent/subagent/README.zh.md`(Activation 容量、本地与外部子级、失败与恢复、理解实现) |
| 工具配置/受管理委派/preset/模型选择/动态措辞 | `packages/subagent/tool-subagent/README.zh.md`(最小配置、受管理的委派、选择子级 preset、选择子级 LLM、结果确认) |
| send_message/interrupt_agent/list_agents | `packages/subagent/tool-subagent-control/README.zh.md` |
| 正式 vs 实验分层 | `packages/subagent/`(无实验标记)vs `packages/experimental/agent-team/README.zh.md` frontmatter |
| 代码量 | `wc -l packages/subagent/*/src/*.ts` = 10112;`wc -l packages/experimental/agent-team/src/*.ts` = 2179 |
| pi-web 拆除三冲突 / ZCode 教训 / 五定律 | [045](../模块设计/045-subagent管理.md) 对比研究节(本表不重复) |
