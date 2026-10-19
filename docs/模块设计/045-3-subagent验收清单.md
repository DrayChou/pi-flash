# subagent M1 验收清单

> 配套 [045-1 开发计划](045-1-subagent开发计划.md)。对象:`extensions/pi-subagents`。
> 所有命令在仓库根执行;`PI` 指 vendored CLI:
> `node vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js`
> 下面简写为 `$PI`。**不需安装依赖**——Node 24 原生跑 TS,SDK 由扩展在运行时
> lazy import(实测可用,157 导出)。

## 0. 一条命令:机制层测试

```bash
cd extensions/pi-subagents && node --test "test/*.test.ts"
```

期望:`ℹ tests 64 / ℹ pass 64 / ℹ fail 0`。

覆盖:容量(拒绝不排队/降低上限不杀已在跑)、结算(清理后才交付结果/关准入/迟到
输入被拒)、注册表(能力协商/重复注册)、深度、描述符(版本/对账)、工具面
(caller/parent 双投递/分码)、profile(两级/fail-closed)、本地后端(答案/继续
输入/中断/失败)。

## 1. 单代理委派(真模型,最小冒烟)

```bash
$PI -e extensions/pi-subagents/src/index.ts --model deepseek/deepseek-flash --no-session \
  -p "Use the agent tool exactly once with prompt 'Reply with exactly: PONG-7'. Then tell me what the child replied."
```

期望:输出里出现子代理回复 `PONG-7`(主代理转述)。**这证明**:工具注册、
子会话创建、caller 投递、结果回流全链路通。

## 2. 并行两路

```bash
rm -rf tmp/subagent-smoke
$PI -e extensions/pi-subagents/src/index.ts --model deepseek/deepseek-flash \
  --session-dir tmp/subagent-smoke --mode json -p \
  "Call the agent tool twice in parallel: one child must reply 'AAA', the other 'BBB'. Then report both."
```

期望(JSON 事件流):
- 两条 `tool_execution_start` 的 `toolName=agent`
- 两条 `tool_execution_end`,`isError=false`,`structuredContent.childId` 分别为
  `sub-1`/`sub-2`,`text` 分别为 `AAA`/`BBB`
- `durationMs` 有值(说明并发而非串行)

## 3. 子会话落盘与父子链接

```bash
find tmp/subagent-smoke -name "*.jsonl" | sort
head -c 300 <子会话文件>
```

期望:
- 子会话 jsonl **与父会话同目录**(`tmp/subagent-smoke/`,而非用户
  `~/.pi/agent/sessions`)
- 子会话 header 含 `"parentSession":"<父会话相对路径>"`
- **不污染真实目录**:跑完后 `~/.pi/agent/sessions/` 下不应新增本次子会话文件

## 4. 描述符持久化

```bash
grep -c '"subagent/descriptor"' tmp/subagent-smoke/*.jsonl
grep -o '"customType":"subagent/descriptor","data":{[^}]*}' tmp/subagent-smoke/*.jsonl | head -1
```

期望:父会话里有描述符条目,`phase` 依次出现 `provisioning` → `active` → 终态,
`provider`/`depth`/`cwd` 正确。

## 5. 容量真实耗尽(可选,约 9 次调用)

```bash
$PI ... -e ... PI_SUBAGENTS_CAPACITY=2 ... -p "Call the agent tool three times in parallel ..."
```

期望:第三次返回 `CAPACITY_REACHED` 分码(不排队、不挂起)。

## 6. Profile

在 `~/.pi/agent/agents/probe.md` 建:

```markdown
---
description: probe
tools:
  - read
---
You are a probe. Answer in one word.
```

然后:

```bash
$PI -e extensions/pi-subagents/src/index.ts --model deepseek/deepseek-flash --no-session \
  -p "Use the agent tool once with profile 'probe' and prompt 'say OK'."
```

期望:子代理正常回答;若把 frontmatter 写成非法的 `tools: read`(非列表),
委派返回 `PROFILE_INVALID` 而不是静默降级。

## 7. 深度护栏

期望:子会话**不加载本扩展**(子会话由 `createAgentSession` 创建,默认资源发现
不含 `-e` 传入的路径),因此孙代无从委派;若强行加载,运行期深度表
(`childDepths`)会把 `depth+1` 传给 manager 并以 `DEPTH_LIMIT` 拒绝。

## 本轮真机冒烟已修的缺陷(留档)

| 现象 | 根因 | 修法 |
|---|---|---|
| 工具调用报 `Cannot read properties of undefined (reading 'sessionFile')` | pi 的 `execute` 签名是 `(toolCallId, params, signal, onUpdate, ctx)`,原实现把第 3 个参数当 ctx | 按正确位置取 `ctx` |
| 子会话写到用户 `~/.pi/agent/sessions`,且无父子链接 | 未传 `sessionManager`,也没设 `parentSession` | `SessionManager.create(cwd, 父sessionDir, { parentSession })` |
| 描述符从不落盘 | `ManagerOptions.onDescriptor` 声明了但没接 manager | 构造时传 `onDescriptor: (d) => persist(d)` |

## 已知边界(不在本验收内)

- UI:PF 桌面端尚未接入(044),当前只有 CLI 形态
- 后台投递(`background: true`)只回 childId,完成通知未接宿主(M3)
- codemode fan-out / usage 上卷展示(M2)、worktree、外部后端(ACP/CC/Codex)
