# subagent 开发计划(扩展侧,045 配套)

> 状态:**待开工**。依据 [045-subagent管理](045-subagent管理.md)(设计)+
> [调研/subagent三家对比](../调研/subagent三家对比.md)(研究归档)。
> **优先级铁定:L1 复刻 dsh-subagent 调用机制先行;L2 pi 增强(codemode/nested-call)
> 后置叠加。** 机制与 codemode 正交——manager/activation 是纯编排类,不依赖 codemode,
> 不会推倒重来。
> 交付物形态:**pi package 扩展**(`pi-subagents`),不碰宿主 Rust 代码。

## 0. 复刻基准事实(dsh 0.2.1-rc.2)

| dsh 包 | 行数 | 复刻取舍 |
|---|---|---|
| `subagent/src/manager.ts` | 1493 | **核心复刻对象**:启动/准入/容量/父子所有权/结算/释放 |
| `subagent/src/index.ts` | 655 | provider 注册表 + 服务入口(剥 cordis 注册壳) |
| `subagent/src/types.ts` | 356 | 契约类型,直接平移 |
| `subagent/src/descriptor.ts` | 269 | 版本化 `subagent/descriptor` 会话事件 |
| `subagent/src/catalog.ts` + `projection.ts` | 360 | 子级目录投影 |
| `subagent/src/activation.ts` | 186 | 执行期记录/容量槽 |
| `subagent/src/child-agent.ts` | 291 | 子级组装 + 深度策略 |
| `subagent/src/continuation-messages.ts` | 168 | 相邻 agent 消息/结算通知 |
| `subagent/src/structured.ts` | 155 | 结构化结果暂存与闸门 |
| `subagent-spawn-in-process` / `fork-in-process` | 59 / 83 | 两个后端,极薄 |
| `tool-subagent/src/index.ts` | 505 | 委派工具(工具面) |

**合计 ~5143(服务)+1056(工具)+142(后端)**。裁掉 cordis 注册、model-selection/
preset-selection 分支(我们走 profile,见 045 §8),预计落到 3000–3500 行 TS。

## 1. M1:L1 机制复刻(spawn + caller 投递跑通)

**目标**:`agent(prompt)` 在 pi CLI 里可用,子会话是完整 pi session,结果即工具结果。

> **进度(2026-10-18)**:1.1/1.2/1.3/1.4/1.5/1.6/1.10 已落地并通过测试
> (`extensions/pi-subagents`,64 测试全绿)。1.7 fork 后端以同一 `LocalProvider`
> (`inheritsParentContext: true`)提供,首次体验证待 CLI 冒烟。1.8 工具面已接
> (caller/parent 双投递+稳定分码)。1.9 深度护卡当前只有运行期一层
> (`childDepths` 映射),构造期 loader 不装配待补。

| # | 任务 | 产出 | 验收 |
|---|---|---|---|
| 1.1 | 类型层平移 `types.ts`(子代理请求/结果/provider 契约,去 dsh 特有字段) | `src/types.ts` | tsc 通过 |
| 1.2 | descriptor(版本化自定义 entry:profile/parentToolCallId/depth/resourceSnapshot) | `src/descriptor.ts` | 写读回环测试 |
| 1.3 | Activation 生命周期 + 容量槽(在线名额、耗尽即拒 `CAPACITY_REACHED`) | `src/activation.ts` | 容量测试:8 个占满→第 9 个拒且不排队 |
| 1.4 | SubagentManager(启动/准入/父子所有权/结算/释放、兑现即发布) | `src/manager.ts` | **对着 dsh README 行为约定写测试**:启动后失败要保持队列、结算等后代、释放防迟到工作 |
| 1.5 | provider 注册表 + 能力声明协商(`continuable`/`depthLimit`/`modelOptions`/`worktree`,不满足即拒 `BACKEND_UNSUPPORTED`) | `src/registry.ts` | 缺能力后端注册即拒 |
| 1.6 | spawn 后端(全新上下文子会话,`createAgentSessionFromServices` + `parentSession`) | `src/backends/spawn.ts` | 子会话独立 JSONL、header 带父 |
| 1.7 | fork 后端(fork 已完成轮次内容) | `src/backends/fork.ts` | 子会话首条消息含父转录 |
| 1.8 | 委派工具(单工具、`namespace: subagents`、caller 投递=await 即结果、稳定分码) | `src/tool.ts` | CLI 内 spawn 一个子代理完成任务并回结果 |
| 1.9 | 深度护栏(构造期 loader 不装配 + 运行期 child-marker 不注册) | — | 子会话内 `agent` 工具不可见 |
| 1.10 | 工具描述动态措辞(按 profile 继承配置,fresh/fork 两种句) | — | 快照断言描述文本 |

**M1 出口**:三个真实场景手跑——单代理委派、并行批(模型同轮多调用)、
子代理失败(分码正确)。

## 2. M2:L2 pi 增强(codemode + 原生记账)

**目标**:不动 M1 机制,叠 pi 能力面。

| # | 任务 | 验收 |
|---|---|---|
| 2.1 | `exposure: "direct"` + `outputSchema`/`structuredContent` 正式化 | 模型直调 + 脚本调双通 |
| 2.2 | codemode fan-out(`ctx.executeTool` 并行 `Promise.allSettled`) | 脚本 3 路 fan-out,只有聚合结论进模型 |
| 2.3 | nested-call usage 上卷验证(不自建账本) | 父工具结果含子代理 usage;compaction 清单含嵌套 |
| 2.4 | `prepareLoadout` 适配(`codemode.mode: "only"` 时隐藏声明) | 两模式下工具声明正确 |
| 2.5 | 子会话 `extensionFactories` 显式装配 codemode/tool_search/MCP(0.99 补 pi-web 的课) | 子会话内脚本可跑 |

**M2 出口**:同一任务用「模型并行 2 路」与「脚本 fan-out 3 路」两种写法各跑通。

## 3. M3:parent 投递(v2 后台)+ 宿主契约

| # | 任务 | 验收 |
|---|---|---|
| 3.1 | `delivery: 'parent'`:立即返回 childId、完成走 `ctx.ui.notify` + entry | CLI 后台委派不阻塞父 turn |
| 3.2 | 中断三层语义(本地停轮次留队列 / 外部永久停 / 已结束空操作) | 三态各一测 |
| 3.3 | catalog 投影 + `list_agents` 类控制工具(复用同一 schema) | 列表与实际一致 |
| 3.4 | 孤儿恢复(`agent_settled.aborted` 区分取消/崩溃 + resourceSnapshot resume) | 杀进程重启后标 interrupted 可续 |
| 3.5 | setWidget 载荷格式定稿(与 044 契约表对齐) | 宿主侧联调 |

**M3 出口**:PF 宿主接上后台通知与聚合面板(044 v2)。

## 4. 依赖与并行

- **M1 全部独立于宿主**——纯 TS,只依赖 vendored pi 1.1.0,可在 CLI 内完成验收。
- **M2 只依赖 M1.8/M1.9**(工具面存在)。
- **M3.5 是唯一需要 044 配合的点**;其余 M3 也可在 CLI 验证。
- 可并行的:1.1/1.2/1.3 先行,1.4 与它们并行(接口按 types.ts 定);1.6/1.7 与 1.4 并行。

## 5. 风险

| 风险 | 应对 |
|---|---|
| manager.ts 1493 行的隐性不变式(dsh 靠契约而非注释撑住,需逐节对照 README) | 每个行为约定落一条测试,不靠"看起来对" |
| 剥 cordis 壳时漏掉生命周期(注册/注销、HMR 等价物) | 先圈出 `ctx.on/ctx.provide/effect` 全部调用点再动手 |
| pi 扩展在 RPC 宿主下的行为差异(会话构造、事件流) | M1 先在 CLI 验收,RPC 差异留给 044 联调 |
| Windows worktree | 按 `BACKEND_UNSUPPORTED` 拒绝(045 §10 待研究 4) |
| 扩展分发 | pi package 走 npm;PF 是否内置分发由 044 后续定 |

## 6. 不做

- ❌ 不碰 `crates/app` 宿主代码(M1–M3 全在扩展侧)
- ❌ 不做 team/roster/DAG(远期,045 §8 之外的协作面)
- ❌ 不做多后端外部 provider(ACP/CC/Codex,远期)
- ❌ 不自建 token 账本、不自建通知去重、不自建 toolCallId 映射(pi 原生)
