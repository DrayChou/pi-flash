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
- **排队发送恒走 `prompt` + `streamingBehavior`，禁用裸 `follow_up`/`steer`**（2026-10-17 真机 bug：排队内容几分钟后「重复发送」）。根因：RPC `follow_up` → `session.followUp()` → `_queueUserInput` **不校验 isStreaming**，且入队前还要 `await _runInputHandlers`（扩展 handler 有 latency）——客户端猜「在跑」而 pi 恰好收尾时，消息入到**空闲 agent 的 followUpQueue**，无人消费；下一轮 `prompt` 跑完时 agent-loop 在「would stop」处 drain 队列，陈旧消息被当新轮冲出来（实证：滞留消息的落盘时间 = 下一轮 assistant 终条同一秒）。`prompt(text, {streamingBehavior:"followUp"|"steer"})` 由 pi 以自身 isStreaming **原子**判定：在跑→入队，空闲→立即开跑（失败方向安全）。pi-web parity：rpc-manager.ts 恒带 `streamingBehavior`，从不发裸 `follow_up`。配套：abort/prompt 被拒**不清 pending_echoes**（pi 的 abort 只揖当前 run 不清队列，滞留消息回显时还要靠它去重，清了就出重复气泡）。

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

## 进程死亡与轮终语义（EOF 三件套，2026-10-19）

- pi 进程死亡不发任何「轮终」事件——只有 stdout EOF。app 侧 pump 尾部（`runtime/mod.rs` 的 `consume_runtime_events` 通道关闭分支）统一收尾：轮中死亡 ⇒ `turn_error=true` + `fail_dangling_tool_calls()`（悬空工具卡就地落红）+ `settle_turn()`（响铃）+ `TurnSettled`（标签栏红点）；空闲死亡只复位不冒泡。此前这里只留一条静默的 "pi exited"——标签栏三色点只认 TurnSettled，用户看到的就是「跑一半悄无声息停了」。
- pi-link 合成 [`Event::ProcessExited`]：exit-waiter 线程 300ms 轮询 `try_wait` reap 退出码（std 无法跨线程 wait，child 住 `Arc<Mutex<Option<Child>>>`，Drop take 走后 waiter 见 None 即退）。stderr 恒 piped + 16KB 尾缓冲（`PiSession::stderr_tail`，`STDERR_TAIL_CAP`）——node OOM / unhandled rejection / 扩展 fatal 只出现在 stderr，死亡时 eprintln 留痕。
- 排查「跑一半停了」：先看 wire log 尾部有无 `=== stdout EOF`——有 = 进程死（本节语义收尾，状态行带退出码）；无 EOF 也无 agent_end = 流挂起（另一类问题）。

## 会话事件解析口径（1.3.0 实测，升级复核点）

`AgentSessionEvent`（`dist/core/agent-session.d.ts`）全量对齐：除轮终三件套（`agent_end`/`agent_settled`/`auto_retry_*`）外，`compaction_start/end`（end 带 `errorMessage`/`willRetry`）、`summarization_retry_scheduled/attempt_start/finished`（与 auto_retry 同构的退避窗口，轮未结束）、`extension_error`（rpc-mode 对扩展错误的输出）、`tool_execution_*`/`queue_update`/`entry_appended`/`session_info_changed`/`thinking_level_changed`（归 `Event::WireEvent`，app 暂不消费）均已 typed。真正未知 type 落 `Event::Unparsed`：按 type 去重 eprintln 留痕，不再完全静默。升级 pi 时 grep 新版 agent-session.d.ts 的 `AgentSessionEvent` 核对增量。

## 调试

- `PI_FLASH_RPC_LOG` 开线缆日志。
