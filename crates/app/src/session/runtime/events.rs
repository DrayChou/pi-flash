//! 运行时事件分发：RPC 事件泵消费、on_event 各事件臂、轮结算与
//! phase/通知小件。pump（consume_runtime_events）epoch 守卫在此。

use gpui::Context;
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, Usage, content_blocks, parse_tree,
};

use crate::i18n::tr;
use crate::session::messages::{Msg, Role, UsageLine, merge_tool_result, result_payload};

use super::*;

impl SessionRuntime {

    /// 轮末提示音（「其他」页开关）。`stream_started` 兼作「本轮在飞」的闩：
    /// AgentSettled 与 AgentEnd 是同一轮的两个终止事件、都会到达，所以在这里
    /// take 掉它就恰好响一次；abort_stream() 会清掉闩，用户中止的一轮静音。
    fn settle_turn(&mut self) {
        if self.stream_started.take().is_some() && self.sound_on {
            crate::services::workspace::play_notify_sound();
        }
    }

    /// 重放 [`Self::system_msgs`] 刷新 `sys`（transcript 无 system 消息时保留
    /// 旧值，维持「尚未加载」语义）；变了就发 Changed，让开着的信息面板当场
    /// 点亮。
    fn replay_system(&mut self, cx: &mut Context<Self>) {
        if let Some(sys) = pi_link::transcript::transcript_system(&self.system_msgs) {
            let changed = match &self.sys {
                Some(prev) => prev.prompt != sys.prompt || prev.tools != sys.tools,
                None => true,
            };
            self.sys = Some(sys);
            if changed {
                cx.emit(SessionEvent::Changed);
            }
        }
    }

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        // MessageStart(user) 置位翻页锚点（见函数尾 page_turn）
        let mut user_arrived = false;
        match event {
            Event::Response { command, success, error, data, .. } => {
                if command == "get_state" && success {
                    if let Some(data) = &data {
                        let st = SessionState::parse(data);
                        // pi 换了绑定的会话文件（draft 首条 prompt 落盘 /
                        // fork / clone 换分支）→ shell 身份必须跟着换
                        let rebound = st.session_file.clone().map(PathBuf::from);
                        self.state = Some(st);
                        if let Some(f) = rebound {
                            self.follow_session_file(f, cx);
                        }
                    }
                    if self.pending_rename {
                        self.pending_rename = false;
                        let prefill = self
                            .state
                            .as_ref()
                            .and_then(|s| s.session_name.clone())
                            .or_else(|| {
                                self.messages
                                    .iter()
                                    .find(|m| matches!(m.role, Role::User))
                                    .map(|m| m.plain_text())
                            })
                            .map(|v| v.chars().take(50).collect::<String>())
                            .unwrap_or_default();
                        cx.emit(SessionEvent::RenameReady(prefill));
                    }
                } else if command == "get_session_stats" && success {
                    if let Some(data) = &data {
                        self.stats = Some(SessionStats::parse(data));
                        if let Some(f) = data["sessionFile"].as_str().map(PathBuf::from) {
                            // same follow-the-rebind path as get_state: a draft
                            // persisted by its first prompt, or a fork/clone
                            // that moved pi onto the branched file
                            self.follow_session_file(f, cx);
                        }
                    }
                } else if command == "set_session_name" && success {
                    // pi flushed the name to the session file — reload the
                    // sidebar labels (pi-web onRenamed → loadSessions), with
                    // one delayed pass to cover flush lag
                    cx.emit(SessionEvent::ListDirty);
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |_c, cx| {
                            cx.emit(SessionEvent::ListDirty);
                            cx.notify();
                        });
                    })
                    .detach();
                } else if command == "get_commands" && success {
                    if let Some(data) = &data {
                        let commands = SlashCommand::parse_list(data);
                        // 转发给 Chat：并进全局清单 + 回写自有缓存（010-启动.md §3）
                        cx.emit(SessionEvent::Commands(commands.clone()));
                        self.commands = commands;
                    }
                } else if command == "get_available_models" && success {
                    if let Some(data) = &data {
                        // project-level shared catalog: forward, don't store
                        cx.emit(SessionEvent::Models(pi_link::protocol::parse_model_list(data)));
                    }
                } else if command == "set_model" && success {
                    // pi 1.0 swaps the model asynchronously — a get_state sent
                    // right after the command still reports the PREVIOUS model
                    // (measured), which made the pill lag one click behind.
                    // The set_model response carries the swapped model: apply
                    // it immediately, then re-sync state (thinking level now
                    // reflects the new model, e.g. "off" for non-reasoning).
                    if let Some(data) = &data {
                        if let Some(st) = self.state.as_mut() {
                            st.model = pi_link::protocol::parse_model_info(data);
                        }
                    }
                    self.refresh_state();
                } else if command == "get_entries" && success {
                    // fork 锚点的唯一可靠来源：扁平 entries + parentId 回溯。
                    // get_tree 在长会话上会被 pi 自己拒绝（见 GetEntries 注释），
                    // 033 导航面板因此降级为空，但「新分支」必须一直可用。
                    if let Some(data) = &data {
                        let (entries, leaf) = pi_link::protocol::parse_entries(data);
                        self.active_user_entry_ids =
                            pi_link::protocol::active_user_entry_ids(&entries, leaf.as_deref());
                        self.apply_entry_ids();
                    }
                } else if command == "get_tree" && success {
                    if let Some(data) = &data {
                        let (tree, leaf) = parse_tree(data);
                        // 树只是 033 导航面板的数据源；锚点以 get_entries 为准
                        // （同一条链、同一顺序），这里同步一份作为短会话的兜底
                        self.active_user_entry_ids =
                            super::fork::collect_path_user_ids(&tree, leaf.as_deref());
                        self.apply_entry_ids();
                        self.branch_tree = Some((tree, leaf));
                    }
                } else if command == "fork" || command == "clone" {
                    // clone = 整段复制（rpc clone → fork at leaf），与 fork
                    // 的落地动作完全一样
                    self.on_fork_response(success, error.as_deref(), cx);
                } else if command == "get_messages" && success {
                    if let Some(data) = &data {
                        let rpc_msgs = data["messages"].as_array().cloned().unwrap_or_default();
                        // 系统提示词 + 工具声明：pi 0.86+ 把它们写进 transcript
                        // 的 system 消息（sections / toolsAdded），replay 是唯一读法
                        // （pi-ai transcript.js；pi-web 的 System/Tools 面板同源）。
                        // 快照是权威源：system 消息整表替换后重放（live 流式追加
                        // 的增量以快照为准对齐）。在叶子链修复之前算——那份回退只
                        // 喂显示用的消息。
                        self.system_msgs = rpc_msgs
                            .iter()
                            .map(|e| e.get("message").unwrap_or(e))
                            .filter(|m| m["role"].as_str() == Some("system"))
                            .cloned()
                            .collect();
                        self.replay_system(cx);
                        // leaf-chain repair: pi anchors its restored leaf at the
                        // last entry of ANY type; a mis-parented custom entry
                        // (plan-mode-state 实测) strands whole turns off the
                        // chain and get_messages comes back short. Rebuild from
                        // the file anchored at the last message entry instead.
                        // 秒开深度优化（pi-flash-2o92）：全文重读是 O(会话大小)
                        // 的主线程冻结源（20MB 上限下 ~300ms+）——改后台线程
                        // 执行，期间维持现有显示（tail 预渲染），落地后磁盘链
                        // 赢则换入、落败则执行权威投影（同守卫下无陈旧回退）。
                        self.last_rpc_msg_count = Some(rpc_msgs.len());
                        if rpc_msgs.len() < self.disk_msg_count && self.file.is_some() {
                            let rpc_len = rpc_msgs.len();
                            self.spawn_leaf_repair(rpc_len, Some(rpc_msgs), cx);
                            return;
                        }
                        self.project_rpc_messages(rpc_msgs, cx);
                    }
                    self.status = status_line(true, "resumed");
                } else if command == "compact" {
                    // 手动压缩（圆环弹窗按钮）：response 在摘要 LLM 完成后
                    // 才回（带 summary/usage）——清防连点标志并重拉
                    // stats/messages（compaction 卡片 + 上下文环跟着变）
                    self.compacting = false;
                    if success {
                        self.refresh_state();
                        if let Some(s) = self.agent.session.as_ref() {
                            let _ = s.send(&Command::GetMessages);
                        }
                    } else {
                        self.status =
                            format!("compact failed: {}", error.unwrap_or_default());
                    }
                } else if command == "bash" {
                    // `!` shell 命令最终结果（流式增量已就地追加；这里回填
                    // 退出码/截断/取消并点亮完成态）。失败响应（会话忙被 pi
                    // 拒等）同样要收尾乐观卡片，否则「执行中」永不消失。
                    let result = if success {
                        data.map(|d| pi_link::protocol::BashResult::parse(&d))
                            .unwrap_or_default()
                    } else {
                        pi_link::protocol::BashResult {
                            output: error.clone().unwrap_or_default(),
                            exit_code: Some(1),
                            ..Default::default()
                        }
                    };
                    self.finalize_bash(result, cx);
                } else if success {
                    self.status = format!("{command} ok");
                } else if command == "prompt" {
                    self.status =
                        format!("prompt failed: {}", error.unwrap_or_default());
                    // 发送失败：等待行不能一直转下去；锚点也无从跟随
                    self.phase_waiting = false;
                    self.pending_echoes.clear();
                    self.pager.release();
                    // 发送失败 = 本轮未跑成，红点
                    self.turn_error = true;
                    // preflight 拒绝（无鉴权/无模型）= 什么都没落盘，撤伪标签
                    if !self.try_bind_reported_file(cx) {
                        cx.emit(SessionEvent::PendingTab(None));
                    }
                } else if command == "follow_up" || command == "steer" {
                    // 排队/插话被 pi 拒（扩展命令不允许排队、输入被
                    // handler 消费掉、无可用模型…）：`PiSession::send` 只写 stdin
                    // 不等应答，拒信是**异步**回来的，所以不能在发送帧上
                    // 就判死。乐观气泡**留着**（用户还看得见自己写了什么，
                    // 不算静默丢失），这里只负责出声原因。
                    self.status = format!(
                        "{}: {}",
                        tr("status.queued"),
                        error.unwrap_or_else(|| "—".into())
                    );
                } else {
                    self.status =
                        format!("{command} failed: {}", error.unwrap_or_default());
                }
            }
            Event::MessageStart { role, blocks, timestamp, is_error, tool_call_id, custom_type: _, custom_display: _, details, raw_system } => {
                // is_error 只出现在 toolResult 上（协议口径：非 toolResult 恒
                // false），而工具报错是 agent 工作过程的一部分——它经常一边
                // 试错一边把事情做完（grep 未命中、命令非零退出…），**不是**
                // 这一轮的「外部错误/崩溃」。红点只认轮末终值：助手消息
                // stopReason=="error"（见 MessageEnd）与 prompt 发送失败。
                match role.as_str() {
                    "user" => {
                        // 轮次边界：结局旗标在用户消息处归零，由随后的每条
                        // assistant 消息按 stopReason **覆盖**（见 MessageEnd）。
                        // 不在 AgentStart/AgentEnd 清——自动重试同样会发
                        // AgentStart（会把上一跳的错误结局抹掉），而
                        // AgentEnd(willRetry) 是 pi 的乐观预判：先报 true，再看
                        // `_prepareRetry` 决定是否真的重试；退避期间被中止（用户
                        // 停止/新消息）或重试未跑起来时，清掉的错误旗再也没机会
                        // 置回来——「错误轮次报青蓝」的根因。
                        self.turn_error = false;
                        self.turn_interrupted = false;
                        // upgrade the optimistic send bubble in place instead
                        // of pushing a duplicate echo (pi-web
                        // optimisticUserMessageKey parity); image blocks and
                        // entry ids ride in with the echo
                        let echo_text = blocks
                            .iter()
                            .map(|b| match b {
                                Block::Text { text, .. } => text.as_str(),
                                _ => "",
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        // 去重口径（2026-10-16）：只看 pending_echoes 里的同文
                        // 条目，命中就**按位置**就地升级那条乐观气泡，不再要求
                        // 它是 messages.last()——排队发送的回显前面必然堆着本轮
                        // 的 assistant 消息，last() 口径下必然重复成两条气泡。
                        // 不命中则照常 push（pi 侧自行产生的 user 消息）。
                        // 回显到达 ≠ agent 已开始应答，等待行（spark + 正在思考…
                        // + shimmer）必须活到第一个 assistant 事件
                        if let Some(ix) = self.pending_echoes.iter().position(|p| *p == echo_text) {
                            self.pending_echoes.remove(ix);
                            // 保障幅度：死信条目（拒绝误判 / 会话切换等）永不回显时只会占位，
                            // 不能无限堆。轮终不清（见上），这里补上限。
                            while self.pending_echoes.len() > 64 {
                                self.pending_echoes.remove(0);
                            }
                            if let Some(ix) = self
                                .messages
                                .iter()
                                .rposition(|m| m.role == Role::User && m.plain_text() == echo_text)
                            {
                                let m = &mut self.messages[ix];
                                m.blocks = blocks;
                            }
                        } else {
                            self.messages.push(Msg {
                                role: Role::User,
                                bash: None,
                                blocks,
                                usage: None,
                                entry_id: None,
                                ts: timestamp,
                                end_ts: None,
                                stop_reason: None,
                                error_message: None,
                                custom_type: None,
                                custom_display: true,
                                details: None,
                                model: None,
                            });
                        }
                        user_arrived = true;
                    }
                    "assistant" => {
                        self.phase_waiting = false;
                        // 不清 pending_echoes：排队发送的回显在**下一轮 turn 边界**
                        // 才到，中间会先流过本轮的一堆 assistant 事件；这里
                        // 清了就是“排队气泡回显时又被当成新消息推了一条”
                        // （2026-10-16）。收口放到轮终（AgentEnd/AgentSettled/发送失败/
                        // 整表重读）。
                        self.streaming_content = false;
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            bash: None,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "toolResult" => {
                        let (text, images) = result_payload(&blocks);
                        if let Some(m) = self.messages.last_mut() {
                            merge_tool_result(
                                m,
                                tool_call_id.as_deref(),
                                is_error,
                                &text,
                                images,
                                details.clone(),
                                timestamp,
                            );
                        }
                    }
                    "custom" => {
                        let _ = &details;
                        self.messages.push(Msg {
                            role: Role::Custom,
                            bash: None,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "system" => {
                        // pi 每个 run 都会先追加一条 role:"system"（content /
                        // sections / toolsAdded 补丁），RPC message_start 全量
                        // 携带——收进 transcript 重放缓存，面板实时点亮
                        // （新会话第一轮就有，不必等下一次 get_messages）。
                        // message_end 不重复处理（start 已带全量）。
                        if let Some(raw) = raw_system {
                            self.system_msgs.push(raw);
                            self.replay_system(cx);
                        }
                    }
                    _ => {}
                }
                let _ = timestamp;
            }
            Event::MessageUpdate(assistant_event) => {
                self.phase_waiting = false;
                self.streaming_content = true;
                // 060 微信回推：把流式事件原样冒给订阅方，由它自己决定何时
                // flush（ZCode assistantReplyBuffer 语义，§6 坑1 边界不按 chunk）
                cx.emit(SessionEvent::Assistant(assistant_event.clone()));
                match assistant_event {
                AssistantEvent::TextDelta { content_index, delta } => {
                    if let Block::Text { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Text { content_index, text: String::new() },
                    ) {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::TextEnd { content_index, content } => {
                    if let Block::Text { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Text { content_index, text: String::new() },
                    ) {
                        *text = content;
                    }
                }
                AssistantEvent::ThinkingStart { content_index } => {
                    self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    );
                }
                AssistantEvent::ThinkingDelta { content_index, delta } => {
                    if let Block::Thinking { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    ) {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::ThinkingEnd { content_index, content } => {
                    if let Block::Thinking { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    ) {
                        *text = content;
                        // pi-web parity: collapse thinking once it completes
                        let msg_ix = self.messages.len().saturating_sub(1);
                        self.collapsed.insert((msg_ix, content_index), false);
                    }
                }
                AssistantEvent::ToolCallStart { content_index, id, tool_name } => {
                    self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id,
                            name: tool_name,
                            args: String::new(),
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: true,
                            result_arrived: false,
                        },
                    );
                }
                AssistantEvent::ToolCallDelta { content_index, delta } => {
                    if let Block::ToolCall { args, .. } = self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id: String::new(),
                            name: String::new(),
                            args: String::new(),
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: true,
                            result_arrived: false,
                        },
                    ) {
                        args.push_str(&delta);
                    }
                }
                AssistantEvent::ToolCallEnd { content_index, tool_call } => {
                    let name = tool_call["toolName"]
                        .as_str()
                        .or_else(|| tool_call["name"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = tool_call["arguments"]
                        .as_object()
                        .filter(|o| !o.is_empty())
                        .map(|o| serde_json::Value::Object(o.clone()).to_string())
                        .unwrap_or_default();
                    let name_c = name;
                    let args_c = args;
                    if let Block::ToolCall { name, args, .. } = self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id: String::new(),
                            name: name_c.clone(),
                            args: args_c.clone(),
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: false,
                            result_arrived: false,
                        },
                    ) {
                        *name = name_c;
                        *args = args_c;
                    }
                }
                AssistantEvent::Other(_) => {}
                }
            }
            Event::MessageEnd { role, blocks, usage, timestamp, stop_reason, error_message, model, .. } => {
                if role == "assistant" {
                    if let Some(m) = self.messages.last_mut() {
                        if m.role == Role::Assistant {
                            m.blocks = blocks;
                            m.usage = usage.map(|u| UsageLine {
                                input: u.input,
                                output: u.output,
                                cache_read: u.cache_read,
                                cache_write: u.cache_write,
                                cost: u.cost,
                            });
                            m.ts = timestamp.or(m.ts);
                            m.end_ts = Some(crate::services::format::now_ms());
                            m.stop_reason = stop_reason;
                            m.error_message = error_message;
                            m.model = model;
                            self.streaming_content = false;
                            // 通知点三色 = **最后一条 assistant 消息的 stopReason**
                            // （与转写区 final_error/final_truncated 同一口径）：
                            // error → 红（外部错误/崩溃）；length/aborted → 黄
                            // （内部中断）；其余 = 正常完成青蓝。这里**覆盖**（不是
                            // 只置位）：pi 自动重试会产出多条 assistant 消息，只有
                            // 最后一条是这轮的结局（重试成功也要能从红回到正常）。
                            self.turn_error = m.stop_reason.as_deref() == Some("error");
                            self.turn_interrupted =
                                matches!(m.stop_reason.as_deref(), Some("length" | "aborted"));
                        }
                    }
                } else if role == "user" {
                    // 新会话首条消息落盘（pi session-manager `_persist`：会话有
                    // user/assistant 消息那一刻才建文件，见 `_hasConversation`）。
                    // 此刻文件刚出现在磁盘上，但 shell 侧的会话清单此前只在轮末
                    // （AgentEnd 的 ListDirty）重扫——新会话标签要等整轮回复跑完
                    // 才出现。首条用户消息一到就刷一次侧栏（pi-web
                    // 「running 会话未列入 → loadSessions」parity），再补一次
                    // 300ms 后的刷新兜住写盘 / 指纹扫描的滞后。
                    if self.messages.iter().filter(|m| m.role == Role::User).count() == 1 {
                        cx.emit(SessionEvent::ListDirty);
                        cx.notify();
                        // 绑定对账：此刻文件已在盘上（pi 先 appendMessage 落
                        // 盘再发事件，存在性守卫此刻放行）→ FileBound 迁移
                        // pool key。不等 AgentSettled 的 refresh_state：否则
                        // 「真标签已上侧栏、pool key 还是 draft-N」的窗口里
                        // 点标签会 spawn 出 duplicate runtime。
                        self.try_bind_reported_file(cx);
                        cx.spawn(async move |this, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(300))
                                .await;
                            let _ = this.update(cx, |_c, cx| {
                                cx.emit(SessionEvent::ListDirty);
                                cx.notify();
                            });
                        })
                        .detach();
                    }
                }
            }
            Event::AgentStart => {
                // 不动 turn_error/turn_interrupted：自动重试也会走这里，结局由
                // assistant 消息（MessageEnd）覆盖、由用户消息（MessageStart）
                // 归零（见那两处注释）。
                self.agent_running = true;
                self.status = "running".into();
                if self.stream_started.is_none() {
                    self.stream_started = Some(std::time::Instant::now());
                }
                // 跑动状态的订阅方（psp 状态槽的未读绿点）只挂在 Changed 上，
                // 而 Changed 只在进程退出 / 压缩 / 换工具预设时才发——轮次起止
                // 必须自己冒泡，否则后台会话跑起来也点不亮未读。
                cx.emit(SessionEvent::Changed);
            }
            Event::AgentSettled => {
                self.agent_running = false;
                self.phase_waiting = false;
                self.streaming_content = false;
                // 一轮结束**不**退役锚点：pi-web 只是 promptAnchorActive=false
                // 让垫片收敛，容器 scrollTop 保持——短回复留在屏顶、长回复由
                // 胶水跟随尾部。锚点在此退役会把内容拽回屏底（消息从屏顶跳走，
                // 等于立刻撤销「发言钉顶」）。锚点留待用户滚轮 / 发送失败 /
                // 快照重建 / 会话切换退役。
                self.status = status_line(true, "idle");
                self.settle_turn();
                self.refresh_state();
                // 轮末对账：文件落盘了就绑定换真身（首条消息处通常已绑，这里
                // 幂等）；确定没落盘（prompt 被扩展接管等，pi 没写过任何
                // user/assistant 消息）就撤伪标签——标签不能挂一个永远不会
                // 变真的会话。
                if !self.try_bind_reported_file(cx) {
                    cx.emit(SessionEvent::PendingTab(None));
                }
                // 轮次终点专用事件（turn_error 终值随行）：订阅方按「窗口
                // 没焦点」规则给点——走 Changed 的话 running 已复位，永远
                // 轮不到记点
                cx.emit(SessionEvent::TurnSettled);
            }
            Event::AgentEnd { will_retry } => {
                // willRetry 只说明 pi「打算」重试（乐观预判）——结局旗标不在这里
                // 清：重试真跑起来后由它的 assistant 消息覆盖，没跑起来就保留
                // 上一跳的错误结局（见 AgentStart / MessageEnd 注释）。
                if will_retry {
                    // 重试退避：轮**没**结束（pi 退避后重发 agent_start 接着跑），
                    // agent_running 必须保持 true——composer 停止按钮/输入框提示/
                    // 等待行都读它，就地复位会在退避的 2-8s 里把「停止」翻回
                    // 「发送」、界面假装空闲（pi 语义：isStreaming=_isAgentRunActive
                    // 持续到 settled，isIdle 明列 retry 不算 idle；pi-web 全程
                    // 显示运行中）。轮末收尾（响铃/落盘基线/列表/锚点）一并推迟
                    // 到真轮末；退避倒计时文案由 auto_retry_start 负责。用户此时
                    // 点停止仍走 abort_stream：pi abortRetry 取消退避，乐观复位
                    // 语义不变。
                    self.phase_waiting = false;
                    self.streaming_content = false;
                    cx.emit(SessionEvent::Changed);
                } else {
                    self.agent_running = false;
                    self.phase_waiting = false;
                    self.streaming_content = false;
                    // 同上：轮末不退役锚点（短回复继续钉在屏顶）
                    self.settle_turn();
                    // our own writer advanced the file — re-baseline so the
                    // external-append tick doesn't re-read our own turn
                    self.sync_disk_baseline();
                    // the session file exists now — make the new session show up
                    // in the sidebar (pi-web refreshKey-on-agent_end parity)
                    cx.emit(SessionEvent::ListDirty);
                    // refresh fork anchors so newly-sent user messages gain entry ids
                    self.refresh_anchors();
                    // agent may have written files: refresh git status
                }
            }
            Event::AutoRetryStart { attempt, max_attempts, delay_ms, error_message: _ } => {
                // 状态行（快照/微信端消费）报倒计时与次数；错误原文可能很长，
                // 不进一行状态。桌面端此时按钮保持停止态、等待行照常转。
                let secs = (delay_ms / 1000).max(1);
                self.status = crate::i18n::tf(
                    "status.auto_retry",
                    &[
                        ("secs", secs.to_string()),
                        ("attempt", attempt.to_string()),
                        ("max", max_attempts.to_string()),
                    ],
                );
                cx.emit(SessionEvent::Changed);
            }
            Event::AutoRetryEnd { success, attempt: _, final_error: _ } => {
                // success=true：重试尝试已通过，轮继续（随后的 AgentStart 会把
                // 状态行拨回 running）；false：耗尽/被中止，真轮末的
                // agent_end{willRetry:false}+agent_settled 马上到，收尾归它们。
                if success {
                    self.status = "running".into();
                }
            }
            Event::ExtensionUi(req) => cx.emit(SessionEvent::ExtUi(req)),
            // `!` 执行的流式输出增量：就地追加进乐观卡片（pi-web 不消费这个
            // 事件、靠会话重载——这里直接流式渲染，严格更好）
            Event::BashExecutionUpdate { delta, .. } => {
                if let Some(ix) = self.pending_bash {
                    if let Some(m) = self.messages.get_mut(ix) {
                        if let Some(b) = m.bash.as_mut() {
                            b.output.push_str(&delta);
                        }
                    }
                    self.notify_list(cx);
                }
            }
            Event::Unparsed(_) => {}
        }
        // 用户消息到达（发送回显升级 / steer 推入）→ 翻页：该消息钉视口
        // 顶，历史滚出屏（回显升级同锚点重入不重钉，见 page_turn）
        if user_arrived {
            let ix = self.messages.len() - 1;
            self.pager
                .page_turn(ix, self.messages.len(), self.phase_row_visible());
        }
        self.notify_list(cx);
    }

    /// pi-web ChatWindow agentRunning && !hasStreamingContent parity: the
    /// waiting/running row shows while no content is streaming — before the
    /// first assistant event (waiting) and during tool execution (running).
    pub(crate) fn phase_row_visible(&self) -> bool {
        (self.phase_waiting || self.agent_running) && !self.streaming_content
    }

    /// pi-web phaseLabel parity: running tool names derive from the last
    /// assistant message's unanswered tool calls; none pending = waiting.
    pub(crate) fn phase_label(&self) -> String {
        let tools: Vec<String> = self
            .messages
            .last()
            .filter(|m| m.role == Role::Assistant)
            .map(|m| {
                m.blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolCall { name, result, .. }
                            if !name.is_empty() && result.is_empty() =>
                        {
                            Some(name.to_string())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        match tools.len() {
            0 => tr("status.waiting_model").to_string(),
            1 => crate::i18n::tf("status.running_tool", &[("name", tools[0].clone())]),
            n if n <= 3 => crate::i18n::tf("status.running_tools", &[("names", tools.join(", "))]),
            n => crate::i18n::tf(
                "status.running_tools_with_extra",
                &[
                    ("names", tools[..2].join(", ")),
                    ("count", (n - 2).to_string()),
                ],
            ),
        }
    }

    /// 列表同步：结构手术（splice/reset）与翻页垫片结算全在 ChatList
    /// （chat_list.rs，pi-web useAgentSession 滚屏层的移植）。
    pub(crate) fn notify_list(&mut self, cx: &mut Context<Self>) {
        self.pager.sync(self.messages.len(), self.phase_row_visible());
        // 消息变化（含流式 delta 路径）→ 导航摘要置脏，渲染帧惰性重建
        self.nav_dirty = true;
        cx.notify();
    }

    /// 清除复制反馈（032 恢复）：点亮 ~1.5s 后把 copy_flash 复位并重绘
    /// （pi-web copied-reset parity）。
    pub(crate) fn spawn_flash_clear(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |r, cx| {
                r.copy_flash = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// get_messages 的权威投影（整表替换——fixes doubled rows after the tail
    /// pre-render）：ingest 全表 + fork 锚点映射 + 尾部时间戳回填 + 定位/
    /// 钉顶/重绘。同步路径与后台 leaf 修复的落败分支共用（秒开深度优化
    /// pi-flash-2o92 把修复改后台后，落败分支在这里补投影）。
    pub(crate) fn project_rpc_messages(
        &mut self,
        rpc_msgs: Vec<serde_json::Value>,
        cx: &mut Context<Self>,
    ) {
        self.messages.clear();
        self.pending_echoes.clear();
        for msg in &rpc_msgs {
            let blocks = content_blocks(&msg["content"]);
            let usage = Usage::parse(&msg["usage"]);
            self.ingest_message(msg, blocks, usage, None, cx);
        }
        // map user messages to active-path entry ids (fork anchors)
        let mut ids = self.active_user_entry_ids.iter();
        for m in self.messages.iter_mut() {
            if m.role == Role::User {
                m.entry_id = ids.next().cloned();
            }
        }
        // get_messages payloads carry generation-START stamps only;
        // completion stamps (回复用时) come from the session file's entry
        // write-times
        self.merge_tail_stamps();
        self.apply_pending_locate(cx);
        // 同一会话的重读保住钉顶（换工具预设/压缩后重拉都走这里）
        self.restore_anchor_after_reload();
        self.notify_list(cx);
    }

    // ---- 033 会话导航面板 ----

}
