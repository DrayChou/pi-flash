//! 发送路径：composer 发送/引导/排队、`!` bash 三件套、远程发送、
//! 模型/思考/压缩/工具预设。

use gpui::Context;
use pi_link::protocol::{
    Block, Command,
};

use crate::session::messages::{BashInfo, Msg, Role};

use super::*;

impl SessionRuntime {
    /// RPC 门面：session/runtime/ 域外的文件一律经此发送，不直摸
    /// `agent.session`——进程在场与否、wire 细节都收在 runtime 内部。
    /// 返回 false = 无进程（草稿/已断），调用方按需降级。
    pub(crate) fn rpc(&self, cmd: &Command) -> bool {
        match &self.agent.session {
            Some(s) => s.send(cmd).is_ok(),
            None => false,
        }
    }

    /// steer（中断当前运行并立即注入）——**仅 pif-ui 自动化调用**（2026-10-16
    /// 用户定批：UI 发送一律排队，composer 不再走这条路）。乐观上屏与翻页和
    /// prompt/排队发送一致——发送帧气泡钉视口顶，不等回显。
    /// 走 prompt + streamingBehavior:"steer"（pi-web onPromptWithStreamingBehavior
    /// parity）：排队与否由 pi 原子判定；裸 steer 落在空闲 agent 的 steering
    /// 队列里同样会滞留到下一轮中途才插话。
    pub(crate) fn steer_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image", "data": img.data_b64, "mimeType": img.mime
                })
            })
            .collect();
        match self
            .agent
            .session
            .as_ref()
            .map(|session| {
                session.send(&Command::Prompt {
                    message: text.clone(),
                    images,
                    streaming_behavior: Some("steer".into()),
                })
            })
        {
            Some(Ok(_)) => {
                self.input.clear();
                self.pending_images.clear();
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
                } else {
                    cx.notify();
                }
            }
            Some(Err(e)) => self.status = e,
            None => {}
        }
    }

    /// 乐观上屏 + 发送帧翻页（prompt/排队共用，pi-web optimistic user
    /// message parity）：气泡立即显示并钉视口顶，RPC 回显经 pending_echo
    /// 去重就地升级。
    fn optimistic_send(&mut self, text: String, cx: &mut Context<Self>) {
        // 伪标签：还没绑定会话文件的 draft，乐观发送的同一帧就在侧栏挂标
        // 签（真标签要等 pi 落盘 + 磁盘扫描，进程冷启 1-3s）。prompt/排队/
        // 微信远程三个入口共用本漏斗；已绑定会话被 file 守卫挡掉。
        if self.file.is_none() {
            let preview = text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or_default()
                .to_string();
            cx.emit(SessionEvent::PendingTab(Some(preview)));
        }
        self.pending_echoes.push(text.clone());
        self.messages.push(Msg {
            role: Role::User,
            bash: None,
            blocks: vec![Block::Text { content_index: 0, text }],
            usage: None,
            entry_id: None,
            ts: Some(crate::services::format::now_ms()),
            end_ts: None,
            stop_reason: None,
            error_message: None,
            custom_type: None,
            custom_display: true,
            details: None,
            model: None,
        });
        self.phase_waiting = true;
        // 发送帧翻页：新用户消息立即钉视口顶、历史滚出屏，不等回显不等
        // bounds（page_turn 内部完成 sync 落账）
        let ix = self.messages.len() - 1;
        let msgs = self.messages.len();
        let phase = self.phase_row_visible();
        self.pager.page_turn(ix, msgs, phase);
        cx.notify();
    }

    /// 排队消息（**路收窄**：仅 pif-ui 自动化；composer 走 send_input）。命令
    /// 与 send_input 同一付：prompt + streamingBehavior:"followUp"——排队与否
    /// 由 pi 原子判定（见 protocol.rs Prompt 注释的滞留队列竞态）。
    ///
    /// 三条硬要求（上一版踩过的坑）：
    /// 1. **先上屏再发**——pi 的 `followUp` 只入队 + 发 queue_update，等到
    ///    下一轮 turn 边界才 emit message_start(user)；不上屏就是“打字被
    ///    吞了”的观感；
    /// 2. **发成功才清输入**——上一版 `let _ = session.send(..)` 丢弃返回 +
    ///    无条件 `input.clear()`，pi 侧拒绝（`/ext-cmd` 扩展命令不允许排队、
    ///    输入被扩展 handler 消费、进程不在）时内容静默蒸发；
    /// 3. **失败保留草稿**并写 status（同 send_input 的 Err 口径）。
    pub(crate) fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return;
        };
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image", "data": img.data_b64, "mimeType": img.mime
                })
            })
            .collect();
        match session.send(&Command::Prompt {
            message: text.clone(),
            images,
            streaming_behavior: Some("followUp".into()),
        }) {
            Ok(_) => {
                if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = tr("status.queued").into();
                // 技能命令不做乐观回显（pi-web parity）：RPC 回显的是展开信封
                // 文本，乐观插入裸 "/skill:xxx" 会多出一条重复气泡
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
                }
            }
            Err(e) => {
                // 排队被拒：草稿与附件原样留着，用户改完能重发
                self.status = e;
            }
        }
        cx.notify();
    }

    /// 停止（rpc abort）。
    pub(crate) fn abort_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Abort);
        }
        self.stream_started = None;
        // 中止：等待行立即收起（不等 agent_settled）
        self.phase_waiting = false;
        // **不清 pending_echoes**：pi 的 abort 只掐当前 run，不清 followUp/
        // steering 队列——已排队未回显的消息在下一轮跑完时照常冲出来，回
        // 显到时还要靠 pending_echoes 去重；这里清了就会多出一条重复气泡。
        // 死信兜底由回显臂的 64 上限承担。
        // 中止即「不在跑」：状态槽转圈 / composer 停止按钮都读 agent_running，
        // 若 pi 的终止事件迟到或压根不来（进程被掐），不就地复位就会永久显示
        // 运行中——同一条「立即收起」语义
        self.agent_running = false;
        if let Some(s) = self.state.as_mut() {
            s.is_streaming = false;
        }
        self.notify_list(cx);
        cx.notify();
    }

    /// 「回到最新」按钮（pi-web scrollToBottom）：reveal 列表末条（锚点期
    /// 含垫片——reveal 垫片底 = 回到钉顶位，与 pi-web scrollToBottom 落在
    /// 垫片底同款）。落底后 Bottom 对齐自动归 None 恢复跟随。
    pub(crate) fn scroll_to_bottom(&mut self) {
        self.pager.jump_to_bottom();
    }

    /// 整表重读（`get_messages` / 会话文件重读）之后恢复锚点，**不要**无条件
    /// `release`：工具预设换绑就是一次 `GetMessages`，退役锚点会让「等待模型
    /// 响应」等短内容连同刚上翻的历史一起掉回屏底（用户实测）。锚点仍指着一条
    /// 用户消息就原样保留；否则按「最后一条用户消息」重锚；确实找不到才退役。
    pub(super) fn restore_anchor_after_reload(&mut self) {
        if !self.pager.anchor_active() {
            return;
        }
        let msgs = self.messages.len();
        let phase = self.phase_row_visible();
        let valid = self
            .pager
            .anchor_ix()
            .filter(|ix| self.messages.get(*ix).is_some_and(|m| m.role == Role::User));
        if let Some(ix) = valid {
            self.pager.reanchor(ix, msgs, phase);
        } else if let Some(ix) = self.messages.iter().rposition(|m| m.role == Role::User) {
            self.pager.reanchor(ix, msgs, phase);
        } else {
            self.pager.release();
        }
    }

    /// `!` / `!!` shell 命令发送（031 对齐 pi-web executeBash）：立即执行、
    /// 不进模型；乐观卡片流式回显输出，response 回填终态。发出后
    /// `bash_running` = true（调用方据此清空输入）。
    fn send_bash(&mut self, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim_start();
        let excluded = trimmed.starts_with("!!");
        let command = trimmed[if excluded { 2 } else { 1 }..].trim();
        if command.is_empty() {
            self.status = tr("composer.shell_empty").to_string();
            cx.notify();
            return;
        }
        // pi 侧拒绝条件（rpc-mode bash：busy 即 error）——提前拦，输入保留
        if self.bash_running || self.agent_running || self.compacting {
            self.status = tr("composer.shell_busy").to_string();
            cx.notify();
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return;
        };
        let cmd = Command::Bash {
            command: command.to_string(),
            exclude_from_context: excluded,
        };
        match session.send(&cmd) {
            Ok(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .ok();
                self.pending_bash = Some(self.messages.len());
                self.messages.push(Msg {
                    role: Role::Bash,
                    bash: Some(BashInfo {
                        command: command.to_string(),
                        excluded,
                        running: true,
                        ..Default::default()
                    }),
                    blocks: Vec::new(),
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
                self.bash_running = true;
                self.status = if excluded {
                    "shell（输出仅本地）".into()
                } else {
                    "shell（输出发给模型）".into()
                };
                self.notify_list(cx);
            }
            Err(e) => self.status = e,
        }
    }

    /// bash response 回填终态（pi BashResult）。乐观卡片按 pending_bash 定位，
    /// 索引失效（快照重建/换档重读）时补一张独立完成卡。
    pub(super) fn finalize_bash(&mut self, result: pi_link::protocol::BashResult, cx: &mut Context<Self>) {
        self.bash_running = false;
        let end_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .ok();
        let applied = self
            .pending_bash
            .take()
            .and_then(|ix| self.messages.get_mut(ix))
            .filter(|m| m.role == Role::Bash)
            .map(|m| {
                if let Some(b) = m.bash.as_mut() {
                    b.output = result.output.clone();
                    b.exit_code = result.exit_code;
                    b.cancelled = result.cancelled;
                    b.truncated = result.truncated;
                    b.full_output_path = result.full_output_path.clone();
                    b.running = false;
                }
                m.end_ts = end_ts;
            })
            .is_some();
        if !applied {
            // 卡片丢了：补一张完整卡（命令名无从找回时留空——正常链路
            // pending_bash 不会失效）
            self.messages.push(Msg {
                role: Role::Bash,
                bash: Some(BashInfo {
                    output: result.output,
                    exit_code: result.exit_code,
                    cancelled: result.cancelled,
                    truncated: result.truncated,
                    full_output_path: result.full_output_path,
                    ..Default::default()
                }),
                blocks: Vec::new(),
                usage: None,
                entry_id: None,
                ts: None,
                end_ts,
                stop_reason: None,
                error_message: None,
                custom_type: None,
                custom_display: true,
                details: None,
                model: None,
            });
        }
        self.status = if result.cancelled {
            "shell 中止".into()
        } else {
            match result.exit_code {
                Some(0) => "shell 完成".into(),
                Some(c) => format!("shell 退出码 {c}"),
                None => "shell 完成".into(),
            }
        };
        self.notify_list(cx);
    }

    /// Esc / 停止按钮的 bash 分支（rpc abort_bash；pi 会把已启动的命令
    /// 取消掉，response 带 cancelled=true 收尾卡片）
    pub(crate) fn abort_bash(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::AbortBash);
        }
        cx.notify();
    }

    pub(crate) fn send_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        // 空文本+图片可发送（pi-web handleSend：!msg && !images 才拦）
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        // 040：picker 运行中确认的清单，下一轮（空闲发送）开始前生效——
        // 换包集必须重绑进程（RPC 无运行时切换），`--session` 复接不丢消息。
        // 043：MCP 运行中改过 mcp.json 同走这条重绑通路（新进程重读清单）。
        if !self.agent_running {
            let pending_ext = self.pending_ext_sources.take();
            if pending_ext.is_some() || self.pending_mcp_reload {
                self.pending_mcp_reload = false;
                if let Some(list) = pending_ext {
                    self.ext_sources = list;
                    if let Some(p) = pi_link::session_ext::store_path() {
                        let _ = pi_link::session_ext::write_for(&p, &self.key, &self.ext_sources);
                    }
                }
                if !self.spawn_attached(cx) {
                    return;
                }
                if let Some(s) = &self.agent.session {
                    let _ = s.send(&Command::GetMessages);
                }
                self.refresh_state();
            }
        }
        if !self.ensure_spawned(cx) {
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return;
        };
        // `!` / `!!` shell 命令（031）：不经 prompt/steer，走 rpc bash。
        // 带图片时 ! 只当普通文本（pi-web bashMode 需无附件）。
        if self.pending_images.is_empty() && text.starts_with('!') {
            self.send_bash(&text, cx);
            if self.bash_running {
                // 与普通消息同口径：进 ↑ 历史、清输入
                if self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
            }
            cx.notify();
            return;
        }
        // pi-web parity（rpc-manager.ts:800）：统一 prompt + streamingBehavior，
        // 排队与否由 pi 以自身 isStreaming 原子判定——PF 本地猜的 agent_running
        // 与 pi 真实状态之间有竞态（裸 follow_up 在 pi 侧入队前还要 await 扩展
        // handler），猜「在跑」而 pi 已收尾时消息滞留在空闲队列里、下一轮跑完
        // 才被冲出来 = 「排队内容重复发送」bug。behavior 的失败方向是安全的：
        // pi 空闲就直接开跑。本地猜测只用于状态行文案。
        let streaming = self.agent_running || self.state.as_ref().is_some_and(|s| s.is_streaming);
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image",
                    "data": img.data_b64,
                    "mimeType": img.mime
                })
            })
            .collect();
        let cmd = Command::Prompt {
            message: text.clone(),
            images,
            streaming_behavior: Some("followUp".into()),
        };
        match session.send(&cmd) {
            Ok(_) => {
                // 纯图片发送不进历史、不做乐观回显（空文本气泡无内容可显，
                // 等 RPC 回显带图的完整用户消息）
                if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = if streaming {
                    tr("status.queued").into()
                } else {
                    "running".into()
                };
                // 技能命令不做乐观回显（pi-web parity）：RPC 回显的是展开
                // 信封文本（渲染层折叠成紧凑命令），乐观插入裸 "/skill:xxx"
                // 会多出一条重复气泡。prompt 与排队一致——发送帧即上屏
                // 翻页，不等回显
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
                }
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }


    /// 远程（微信）来的一条文本 —— 与 [`send_input`] **同口径**，但不碰
    /// `self.input` / `pending_images`（那两个是 composer 专属的）。
    ///
    /// 关键：草稿**还没有 pi 进程**时必须先 `spawn()` + `attach_pump`，
    /// 否则 `agent.session` 是 `None`、命令直接丢掉 —— 真机表现就是
    /// 「发消息没有任何应答，重启后才有人回」。
    ///
    /// 返回是否真的发出去了（失败时写 `status`，不静默）。
    pub(crate) fn send_remote_text(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        if self.compacting {
            return false;
        }
        if !self.ensure_spawned(cx) {
            return false;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return false;
        };
        // 运行中 = 排队，否则 prompt（与 composer 完全一致；同走 prompt +
        // streamingBehavior，由 pi 原子判定，理由见 send_input）
        let streaming =
            self.agent_running || self.state.as_ref().is_some_and(|s| s.is_streaming);
        let cmd = Command::Prompt {
            message: text.clone(),
            images: Vec::new(),
            streaming_behavior: Some("followUp".into()),
        };
        let sent = session.send(&cmd).is_ok();
        if sent {
            if self.history.last().map(|h| h != &text).unwrap_or(true) {
                self.history.push(text.clone());
            }
            self.history_ix = None;
            self.status = if streaming {
                tr("status.queued").into()
            } else {
                "running".into()
            };
            // 让桌面端也看到这条消息（微信发的同样属于这轮对话）
            if !text.starts_with("/skill:") {
                self.optimistic_send(text, cx);
            }
        } else {
            self.status = tr("status.send_failed").into();
        }
        cx.notify();
        sent
    }

    /// 远程来的非文本命令（`/停止`、ExtUi 回填）：**不 spawn** ——
    /// 没有进程就明确返回失败，由调用方出声。
    pub(crate) fn send_remote_command(&mut self, cmd: &Command, cx: &mut Context<Self>) -> bool {
        if self.compacting {
            return false;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return false;
        };
        let ok = session.send(cmd).is_ok();
        if !ok {
            self.status = tr("status.send_failed").into();
        }
        cx.notify();
        ok
    }

    /// LLM session title (pi-web lib/session-title.ts parity via a one-off
    /// `pi --no-session --print` run; the in-process SDK call pi-web uses is
    /// not reachable over RPC).
    pub(crate) fn set_thinking_level(&mut self, level: &str, cx: &mut Context<Self>) {
        if level == "auto" {
            self.thinking_override = None;
            cx.notify();
            return;
        }
        self.thinking_override = Some(level.to_string());
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetThinkingLevel { level: level.to_string() });
        }
        cx.notify();
    }

    pub(crate) fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetModel { provider, model: id });
        } else {
            // draft with no process: remember the pick, spawn carries it as
            // --model (pi-web newSessionModel parity)
            self.pending_model = Some((provider, id));
        }
        // no immediate refresh_state here: pi 1.0 completes the swap
        // asynchronously, so an eager get_state reports the PREVIOUS model —
        // the label updates from the set_model response instead (see the
        // set_model response arm)
        cx.notify();
    }

    /// 压缩按钮可用性：agent 跑动中不可（compact 会中止当前 turn）、
    /// 压缩中防连点、未连接不可。除此之外不设门槛——用不用由用户决定。
    pub(crate) fn can_compact(&self) -> bool {
        !self.compacting && !self.agent_running && self.agent.session.is_some()
    }

    /// 圆环弹窗底部手动压缩：rpc compact。`compacting` 期间输入面板锁死
    /// （placeholder 提示 + 变更丢弃）；完成后 pi 回 compact response，
    /// 由其分支清标志并刷新 stats/messages。
    pub(crate) fn compact(&mut self, cx: &mut Context<Self>) {
        if !self.can_compact() {
            return;
        }
        match self.agent.session.as_ref().map(|s| s.send(&Command::Compact)) {
            Some(Ok(_)) => {
                self.compacting = true;
                self.status = tr("status.compacting").into();
            }
            Some(Err(e)) => self.status = e,
            None => self.status = tr("status.disconnected").into(),
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    pub(crate) fn tool_preset_key(&self) -> &str {
        &self.tools_preset
    }

    pub(crate) fn tool_preset_label(&self) -> String {
        let key = self.tool_preset_key();
        if key.is_empty() {
            return "configured".into();
        }
        if key == "custom" {
            // 档名 full+；扩展数量由右边的「扩展(n)」按钮承载，不重复
            return crate::i18n::tr("extensions.full").to_string();
        }
        key.to_string()
    }

}
