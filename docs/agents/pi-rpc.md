# pi RPC wire 陷阱

改动会碰到的文件（先读完本文再动）：
- `crates/pi-link/**`：协议层本体——`protocol.rs` 类型、`client.rs` 侧车进程、`json.rs` 解析、`transcript.rs` 重放、`catalog.rs` 启动目录、`vendor.rs` 定位
- `crates/app/src/agent_session.rs`、`crates/app/src/session/runtime.rs`、`crates/app/src/session/messages.rs`、`crates/app/src/ext_ui.rs`：app 侧主要消费方

随踩随补。vendor 升级（bump `vendor/pi/package.json` + VERSION）必跑 pi-link 符合性测试，并复核下方标了版本号的条目。

## wire 字段与形态

- 内容块类型 camelCase `toolCall`；流式 args 起始走 `partialJson`。
- 工具结果 `role:"toolResult"` 独立回灌，按 toolCallId 挂回工具卡。
- `set_model` 字段是 `modelId`，且**异步**：立刻 `get_state` 拿旧值，响应 data 里才有切换后的完整模型对象。
- client 不得硬编码 `--no-session`（会压掉 `--session`）。
- `get_messages` 快照携带 bashExecution——与乐观卡并轨会出双卡。
- **`agent_end{willRetry:true}` 不是轮末**（pi-flash-7ep）：可重试错误（429/5xx/网络断，默认 maxRetries=3、退避 2s/4s/8s 上限 60s）后 pi 在**同一轮内**退避并重发 `agent_start`。轮的真正终点是 `agent_end{willRetry:false}` + `agent_settled`；pi 语义 `isStreaming=_isAgentRunActive` 持续到 settled（`isIdle` 明列 retry 不算 idle）。伴随事件 `auto_retry_start{attempt,maxAttempts,delayMs,errorMessage}` / `auto_retry_end{success,attempt,finalError?}`（success=true 不带 finalError）。注意 `FreeUsageLimitError` 类账号限额在 pi 的 NON-RETRYABLE 名单里，不会重试（`@earendil-works/pi-ai` utils/retry.js）。1.3.0 起可重试名单扩大：gateway `Unable to forward upstream stream`、`The service is temporarily unavailable` 也会走重试。

## 方法存在性（1.0.0 / 1.1.0 / 1.2.0 / 1.3.0 实测，vendor 升级时复核）

- 无 `navigate_tree` / `systemPrompt` / `get_tools`（`get_tools` 是 pi-web 进程内 SDK 专属）。
- `get_commands` 只回扩展 + skill 命令，**无内置**——升级时复核 `catalog.rs`。
- RPC 模式不支持 custom 扩展 UI；`extension_ui_response` 的 id 必须是请求 id（pi 在 raw-line 层按 id 关联）。
- 1.2.0 起 `getAllTools` 输出多可选键 `namespace` / `reachesTools`（codemode/tool_search 声明用）；pi-link 无 `deny_unknown_fields`，直接容忍。

## 1.3.0 升级复核（2026-10-16，实测 + dist diff）

- **wire 三件套逐字节一致**：`dist/modes/rpc/{rpc-mode,rpc-types,jsonl,rpc-client}.js` 与 1.2.0 完全相同；`agent-session.js` 字符串集（事件名/工具名/键）零增删。protocol.rs 无需动。
- 内置工具名仍 8 个（bash/read/write/edit/find/grep/ls/powershell）；`get_commands` 仍只回扩展+skill。
- 行为变化（不破 wire，但 app 可见）：模型切换丢弃他模型的 thinking 块（旧模型思考不再以纯文本漏给新模型）；resume 保留会话激活的工具（`--tools +name/-name` 在其上叠加）；工具参数含非法 `\u` 转义不再丢字段；尾参 malformed JSON 不再静默半跑而是校验失败回给模型（用户会看到以前「能跑」的工具报错）。
- pi 侧新 settings 键 `sessionShareDestination`（auto/gist/radius，/share 用）——PF 设置页不消费，无需动。
- `kimi-coding` provider 改走 Chat Completions，`models.json` 里它的 baseUrl 覆盖**必须带 `/v1`**（PF 只读透传该文件，用户自己改）。

## transcript 重放

- 系统提示词与工具声明在 transcript 的 `role:"system"` 消息里（content 追加 + sections 补丁 + toolsAdded/Removed），重放对齐 pi-ai `utils/transcript.js`。
- serde_json 开 `preserve_order` 保 sections 顺序。
- turn_start 后的 system message 全量携带 transcript 补丁（app 靠它 live 重放）；1.1.0 起多 `toolsAdded` 键、`agent_settled` 多 `aborted` 字段。

## fork 与深树

- `get_tree` 深树双炸：客户端 serde_json 默认 128 层递归上限（→ `parse_value` disable_recursion_limit + reader 线程 16MB 栈）；pi 侧超长会话自己 `Maximum call stack`。
- fork 锚点正解 = `get_entries` 扁平列表走 parentId，不用 `get_tree`。

## spawn 开关

- `-ne` 只关扩展，个人 skills 照旧（全关要 `-ns`/`-np`/`--no-themes`）；`--tools` 是注册级硬 allowlist（连 getAllTools 都挡）；重复 `-ne` 无害。

## 调试

- `PI_FLASH_RPC_LOG` 开线缆日志。
