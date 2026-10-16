//! 运行时定位与磁盘对账：消息定位/导航摘要 + 尾窗基线同步与外部
//! 追加检测（023 外部改动）。

use gpui::Context;
use pi_link::protocol::{
    Block, Command,
};

use crate::session::messages::{Role, msgs_from_tail};
use pi_link::sessions::read_leaf_messages;

use super::*;

impl SessionRuntime {

    /// 绑定对账：pi 报告过的 sessionFile（state 里）若已落盘就跟随绑定
    /// （FileBound → Chat 迁移 pool key + 摘伪标签换真身）。返回 false =
    /// 报过但确定没落盘（调用方据此撤伪标签）。state 还空着时发一条
    /// GetState 兜底（响应带 sessionFile 走 follow_session_file 同一守卫）。
    pub(super) fn try_bind_reported_file(&mut self, cx: &mut Context<Self>) -> bool {
        if self.file.is_some() {
            return true;
        }
        match self
            .state
            .as_ref()
            .and_then(|s| s.session_file.clone().map(PathBuf::from))
        {
            Some(f) => {
                if !f.is_file() {
                    return false;
                }
                self.follow_session_file(f, cx);
                true
            }
            None => {
                if let Some(session) = &self.agent.session {
                    let _ = session.send(&Command::GetState);
                }
                true
            }
        }
    }

    /// Remember the session file's current size (our own writer's progress):
    /// the external-append tick only re-reads when someone ELSE grew it.
    pub(crate) fn sync_disk_baseline(&mut self) {
        if let Some(f) = &self.file {
            if let Ok(meta) = std::fs::metadata(f) {
                self.disk_file_len = meta.len();
            }
        }
    }

    /// External-append watch (pi-web session-revision parity): another
    /// writer (pi-web on the same session file) appends behind our pi
    /// process's back, and pi's in-memory chain never notices. While idle,
    /// a size change triggers a disk leaf-chain re-read.
    pub(crate) fn check_external_append(&mut self, cx: &mut Context<Self>) {
        if self.agent_running || !self.pending_echoes.is_empty() {
            return;
        }
        let Some(f) = self.file.clone() else { return };
        let Ok(meta) = std::fs::metadata(&f) else { return };
        let len = meta.len();
        if len == self.disk_file_len || len == 0 {
            return;
        }
        let msgs = msgs_from_tail(read_leaf_messages(&f, LEAF_REPAIR_MAX));
        self.disk_file_len = len;
        if msgs.len() != self.messages.len() {
            // 整表重读：锚点索引可能失效，重读后按最后一条用户消息重锚
            self.messages = msgs;
            // fork anchors best-effort (same as the open-time repair path)
            let mut ids = self.active_user_entry_ids.iter();
            for m in self.messages.iter_mut() {
                if m.role == Role::User {
                    m.entry_id = ids.next().cloned();
                }
            }
            self.apply_pending_locate(cx);
            self.restore_anchor_after_reload();
            self.notify_list(cx);
            self.status = status_line(true, "synced from disk");
        }
    }

    /// Backfill message stamps from the session-file tail: snapshots never
    /// carry completion times, and entry write-times are the only end-stamp
    /// source for history (aligned by suffix — tail conversion mirrors the
    /// same ingest semantics).
    pub(super) fn merge_tail_stamps(&mut self) {
        let Some(f) = self.file.clone() else {
            return;
        };
        let entries = pi_link::sessions::read_tail_messages(&f, 256 * 1024, 400);
        if entries.is_empty() {
            return;
        }
        let tail = msgs_from_tail(entries);
        if tail.len() > self.messages.len() {
            return; // projection shorter than file (compaction) — skip unsafe align
        }
        let start = self.messages.len() - tail.len();
        for (i, tm) in tail.into_iter().enumerate() {
            let m = &mut self.messages[start + i];
            if m.role == tm.role {
                if m.ts.is_none() {
                    m.ts = tm.ts;
                }
                if m.end_ts.is_none() {
                    m.end_ts = tm.end_ts;
                }
            }
        }
    }

    /// Reveal a search hit (013): scroll the message list to the matched row —
    /// by payload timestamp, falling back to the first message containing the
    /// needle. Returns false when messages aren't loaded yet (caller parks a
    /// pending_locate for the reconcile to apply).
    pub fn locate_message(&mut self, ts: Option<i64>, needle: &str, _cx: &mut Context<Self>) -> bool {
        if self.messages.is_empty() {
            self.pending_locate = Some((ts, needle.to_string()));
            return false;
        }
        let ix = self.hit_index(ts, needle);
        if let Some(ix) = ix {
            self.pager.reveal(ix);
        }
        ix.is_some()
    }

    fn hit_index(&self, ts: Option<i64>, needle: &str) -> Option<usize> {
        if let Some(ts) = ts {
            if let Some(ix) = self.messages.iter().rposition(|m| m.ts == Some(ts)) {
                return Some(ix);
            }
        }
        let needle_lc = needle.to_lowercase();
        if needle_lc.is_empty() {
            return None;
        }
        self.messages
            .iter()
            .position(|m| m.plain_text().to_lowercase().contains(&needle_lc))
    }

    /// Apply a parked 013 jump once messages exist (reconcile path).
    pub(super) fn apply_pending_locate(&mut self, _cx: &mut Context<Self>) {
        if let Some((ts, needle)) = self.pending_locate.take() {
            if let Some(ix) = self.hit_index(ts, &needle) {
                self.pager.reveal(ix);
            }
        }
    }

    /// 导航数据（每轮摘要 + 总结栏统计，Rc 共享）。渲染层每帧调用：命中
    /// 缓存零成本，notify_list 置脏后下一帧重建一次。
    pub(crate) fn nav_summary(&mut self) -> std::rc::Rc<NavData> {
        if self.nav_dirty {
            self.nav_dirty = false;
            let built = self.build_nav_summary();
            self.nav_cache = Some(std::rc::Rc::new(built));
        }
        self.nav_cache.clone().unwrap_or_else(|| {
            std::rc::Rc::new(NavData {
                turns: Vec::new(),
                users: 0,
                assistants: 0,
                tool_calls: 0,
            })
        })
    }

    fn build_nav_summary(&self) -> NavData {
        // 总结栏统计（自洽口径）：用户/助手按消息条数，工具调用按
        // assistant 消息内的 ToolCall 块数（结果是否合并到达不影响）；
        // custom（压缩摘要/extension 卡）不计
        let mut users = 0usize;
        let mut assistants = 0usize;
        let mut tool_calls = 0usize;
        for m in &self.messages {
            match m.role {
                Role::User => users += 1,
                Role::Assistant => {
                    assistants += 1;
                    tool_calls += m
                        .blocks
                        .iter()
                        .filter(|b| matches!(b, Block::ToolCall { .. }))
                        .count();
                }
                Role::Custom => {}
                // `!` shell 卡不是 assistant 消息，不计统计（034 自洽口径）
                Role::Bash => {}
            }
        }
        // turns = 用户消息锚点（ChatMinimap.tsx parity：每轮=用户消息 +
        // 其后的全部 assistant 回复）；摘要 033：用户前 50 字 + agent
        // 首条有正文回复首行前 50 字（定位器不是会话副本——agent 说
        // 多少话也只摘要一行）
        let turns: Vec<usize> = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == Role::User)
            .map(|(i, _)| i)
            .collect();
        let turns = turns
            .iter()
            .enumerate()
            .map(|(ti, &ux)| {
                let user_prefix = self
                    .messages
                    .get(ux)
                    .map(|m| m.plain_text_prefix(50))
                    .unwrap_or_default();
                let end = turns.get(ti + 1).copied().unwrap_or(self.messages.len());
                let agent = (ux + 1..end).find_map(|i| {
                    let m = self.messages.get(i)?;
                    if m.role != Role::Assistant {
                        return None;
                    }
                    let head = m.plain_text_prefix(80);
                    let line: String = head
                        .trim_start()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(50)
                        .collect();
                    (!line.is_empty()).then_some((i, line))
                });
                TurnSummary {
                    user_ix: ux,
                    user_prefix,
                    agent,
                }
            })
            .collect();
        NavData {
            turns,
            users,
            assistants,
            tool_calls,
        }
    }

}
