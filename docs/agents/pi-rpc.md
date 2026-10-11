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

## 方法存在性（1.0.0 / 1.1.0 实测，vendor 升级时复核）

- 无 `navigate_tree` / `systemPrompt` / `get_tools`（`get_tools` 是 pi-web 进程内 SDK 专属）。
- `get_commands` 只回扩展 + skill 命令，**无内置**——升级时复核 `catalog.rs`。
- RPC 模式不支持 custom 扩展 UI；`extension_ui_response` 的 id 必须是请求 id（pi 在 raw-line 层按 id 关联）。

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
