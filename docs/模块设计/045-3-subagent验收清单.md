# subagent M1 验收清单

> 配套 [045-1 开发计划](045-1-subagent开发计划.md)。对象:`extensions/pi-subagents`。
> **验收主路径就是跑 PF**:`cargo run -p app`,新会话里让模型委派一次,
> 界面上看到 `agent` 工具卡与子代理结果。下面 CLI 步骤是排查用的下位手段。

## A. PF 内验收(主路径,2026-10-19 已实测通过)

集成方式:PF 起 vendored pi 时追加 `-e extensions/pi-subagents/src/index.ts`
(`pi-link/src/subagents.rs` 定位、`client.rs` 挂载;`PI_FLASH_SUBAGENTS=0` 关闭,
`PI_FLASH_SUBAGENTS_EXT` 覆盖路径)。发版时 `scripts/release.sh` 把扩展一并拷到
`dist/pi-flash/extensions/`,查找顺序与开发布局一致。

```bash
cargo run -p app
```

新会话里发送:

```
用 agent 工具委派一次,prompt 是 'Reply with exactly: PF-SUBAGENT-OK',
然后把子代理的回复原样告诉我。
```

期望:
- 出现「工作详情 · N 条消息 · 1 次工具调用」,折叠内是 `agent` 工具卡
  (入参为那条 prompt)
- 最终回答含子代理返回的 `PF-SUBAGENT-OK`
- 子会话文件落在**父会话同目录**(`~/.pi/agent/sessions/<encoded-cwd>/`),
  header 带 `parentSession`;父会话 jsonl 有 `subagent/descriptor` 条目

实测记录(2026-10-19):PF 界面确认通过。同轮快照 `messages` 为
user → assistant(tool_calls=1)→ assistant("PF-SUBAGENT-OK");子会话文件与
描述符齐备。

## B. 机制层测试(改扩展后必跑)

```bash
cd extensions/pi-subagents && node --test "test/*.test.ts"
```

期望:`ℹ tests 64 / ℹ pass 64 / ℹ fail 0`。

覆盖:容量(拒绝不排队/降低上限不杀已在跑)、结算(清理后才交付结果/关准入/
迟到输入被拒)、注册表(能力协商/重复注册)、深度、描述符(版本/对账)、
工具面(caller/parent 双投递/分码)、profile(两级/fail-closed)、本地后端
(答案/继续输入/中断/失败)。

## C. CLI 级排查(可选)

`PI` 指 `node vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js`。
零安装:Node 24 原生跑 TS,SDK 由扩展运行时 lazy import(实测 157 导出)。

### C1 单代理冒烟

```bash
$PI -e extensions/pi-subagents/src/index.ts --model deepseek/deepseek-flash --no-session \
  -p "Use the agent tool exactly once with prompt 'Reply with exactly: PONG-7'. Then report it."
```

期望:输出出现 `PONG-7`。

### C2 并行两路

```bash
rm -rf tmp/subagent-smoke
$PI -e extensions/pi-subagents/src/index.ts --model deepseek/deepseek-flash \
  --session-dir tmp/subagent-smoke --mode json -p \
  "Call the agent tool twice in parallel: one child must reply 'AAA', the other 'BBB'. Then report both."
```

期望:两条 `agent` 的 `tool_execution_end`,`isError=false`,
`structuredContent.childId` 为 `sub-1`/`sub-2`,`text` 分别 `AAA`/`BBB`,
`durationMs` 有值(证明并发)。

### C3 落盘与父子链接

```bash
find tmp/subagent-smoke -name "*.jsonl" | sort
head -c 300 <子会话文件>
```

期望:子会话与父同目录;header 含 `"parentSession":"…"`;跑完后
`~/.pi/agent/sessions/` 不新增本次子会话文件。

### C4 描述符

```bash
grep -c '"subagent/descriptor"' tmp/subagent-smoke/*.jsonl
```

期望:父会话有描述符,`phase` 依次 `provisioning` → `active` → 终态。

### C5 容量耗尽(可选,约 3 次调用)

`PI_SUBAGENTS_CAPACITY=2` + 并行三次 → 第三次返回 `CAPACITY_REACHED`(不排队、
不挂起)。

### C6 Profile

`~/.pi/agent/agents/probe.md`:

```markdown
---
description: probe
tools:
  - read
---
You are a probe. Answer in one word.
```

用 `profile: 'probe'` 委派 → 正常回答;把 `tools` 写成非列表 → `PROFILE_INVALID`
(fail-closed,不静默降级)。

### C7 深度护栏

子会话由 `createAgentSession` 创建,默认资源发现不含 `-e` 路径 → 孙代没有
`agent` 工具;若强行加载,运行期 `childDepths` 会把 `depth+1` 交 manager 并以
`DEPTH_LIMIT` 拒绝。

## D. 本轮真机冒烟已修的缺陷(留档)

| 现象 | 根因 | 修法 |
|---|---|---|
| 工具调用报 `Cannot read properties of undefined (reading 'sessionFile')` | pi 的 `execute` 签名是 `(toolCallId, params, signal, onUpdate, ctx)`,原实现把第 3 个参数当 ctx | 按正确位置取 `ctx` |
| 子会话写到用户 `~/.pi/agent/sessions`,且无父子链接 | 未传 `sessionManager`,也没设 `parentSession` | `SessionManager.create(cwd, 父sessionDir, { parentSession })` |
| 描述符从不落盘 | `ManagerOptions.onDescriptor` 声明了但没接 manager | 构造时传 `onDescriptor: (d) => persist(d)` |

## E. 已知边界(不在本验收内)

- 工具卡为通用渲染(无 subagent 专属卡片、无子会话跳转)——属 044 宿主侧
- 后台投递(`background: true`)只回 childId,完成通知未接宿主(M3)
- codemode fan-out / usage 上卷展示(M2)、worktree、外部后端(ACP/CC/Codex)
