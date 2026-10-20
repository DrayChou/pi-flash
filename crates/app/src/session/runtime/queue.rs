//! 090 排队护栏：PF 本地队列的入队/管理/flush。
//!
//! 背景（「排队插队」根因，2026-10-20 定批）：pi 的 followUp 队列 drain
//! 点是 agent-loop 里「模型第一次停下不调工具」的 would-stop 瞬间——在
//! run 内、agent_end 之前；且 abort/出错退出不清队，滞留消息会冲进下一
//! 个不相关任务。ZCode 桌面端同源引擎同病，它的护栏在 UI 层：排队条目
//! 本地持有（卡片）、不进气泡流、当前轮彻底结束才按序续发。
//!
//! 本文件的对策：queue 模式（默认，设置-其他「运行中发送方式」）下发送
//! 一律扣进 [`SessionRuntime::queued_inputs`]，flush 钉死 `AgentSettled`
//! （唯一可靠的「彻底结束」信号；`AgentEnd{will_retry:false}` 之后 pi 还
//! 可能 post-run continue，在那里 flush 会把消息插进续跑段）。flush 发
//! `Prompt{streaming_behavior: None}`——pi 忙时会**响亮拒绝**而不是静默
//! 入队（response 臂放回队首等下一次 settled），插队从此不可能发生。

use gpui::Context;
use pi_link::protocol::Command;

use crate::i18n::{tf, tr};
use crate::AttachedImage;

use super::*;

/// 附件 → wire images（与 send.rs 各发送口同形）。
pub(super) fn images_to_wire(images: &[AttachedImage]) -> Vec<serde_json::Value> {
    images
        .iter()
        .map(|img| {
            serde_json::json!({
                "type": "image", "data": img.data_b64, "mimeType": img.mime
            })
        })
        .collect()
}

/// 纯函数：队列重排（拖拽落位）。`target` = 落点前空间（0..=len），越界
/// 钳尾（psp apply_drop 同款，陈旧落点安全）；拖回原地（target == from /
/// from+1）是 no-op 返回 false。
pub(crate) fn move_entry(v: &mut Vec<QueuedInput>, from: usize, target: usize) -> bool {
    if from >= v.len() || target == from || target == from + 1 {
        return false;
    }
    let item = v.remove(from);
    let t = if target > from { target - 1 } else { target };
    v.insert(t.min(v.len()), item);
    true
}

impl SessionRuntime {
    /// 090：当前发送方式是否为排队（默认档，设置-其他「运行中发送方式」；
    /// "steer" = 显式插话档）。
    pub(crate) fn queue_mode(&self) -> bool {
        crate::services::workspace::send_mode_pref() != "steer"
    }

    /// 排队总数（本地 + pi 侧镜像）：状态行「已排队 N 条」口径。pi 侧镜像
    /// 正常恒空（composer 不再喂 pi 队列），只剩 pif-ui 自动化的条目。
    pub(crate) fn queue_total(&self) -> usize {
        self.queued_inputs.len() + self.queued_steering.len() + self.queued_followups.len()
    }

    fn set_queued_status(&mut self) {
        self.status = tf("status.queued_n", &[("count", self.queue_total().to_string())]);
    }

    /// composer 发送入队（queue 模式的 send_input 漏斗）：入队 + 清输入/
    /// 附件 + 进 ↑ 历史。**不上屏不进 pending_echoes**——排队期以卡片呈现
    /// （不进气泡流，ZCode parity），flush 后 pi 的 message_start 回显才是
    /// 气泡的唯一来源，回显去重负担随之消失。
    pub(crate) fn enqueue_from_composer(&mut self, text: String, cx: &mut Context<Self>) {
        let images = std::mem::take(&mut self.pending_images);
        self.queued_inputs.push(QueuedInput {
            text: text.clone(),
            images,
            remote: false,
        });
        if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
            self.remember_input(text.clone());
        }
        self.history_ix = None;
        self.input.clear();
        self.set_queued_status();
        cx.notify();
    }

    /// 微信远程入队（与 composer 同一漏斗；不碰 self.input/pending_images）。
    pub(crate) fn enqueue_remote(&mut self, text: String, cx: &mut Context<Self>) {
        self.queued_inputs.push(QueuedInput {
            text: text.clone(),
            images: Vec::new(),
            remote: true,
        });
        if self.history.last().map(|h| h != &text).unwrap_or(true) {
            self.remember_input(text.clone());
        }
        self.history_ix = None;
        self.set_queued_status();
        cx.notify();
    }

    /// 卡片垃圾桶：移除该条（丢弃）。
    pub(crate) fn queue_remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.queued_inputs.len() {
            return;
        }
        self.queued_inputs.remove(ix);
        self.set_queued_status();
        cx.notify();
    }

    /// 卡片拖拽落位（queue_cards.rs on_drop 提交）。
    pub(crate) fn queue_move(&mut self, from: usize, target: usize, cx: &mut Context<Self>) {
        if move_entry(&mut self.queued_inputs, from, target) {
            cx.notify();
        }
    }

    /// 卡片「立即」（ZCode sendQueuedNow parity）：把该条从队列取出，以
    /// `prompt + streamingBehavior:"steer"` 插进当前轮（pi 原子判定——空闲
    /// 则直接开跑）。乐观上屏与 steer_input 同口径。
    pub(crate) fn queue_send_now(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.queued_inputs.len() || self.flush_in_flight.is_some() {
            return;
        }
        let entry = self.queued_inputs.remove(ix);
        let images = images_to_wire(&entry.images);
        match self.agent.session.as_ref().map(|s| {
            s.send(&Command::Prompt {
                message: entry.text.clone(),
                images,
                streaming_behavior: Some("steer".into()),
            })
        }) {
            Some(Ok(_)) => {
                self.status = tr("status.steered").into();
                // 技能命令不做乐观回显（pi-web parity，同 steer_input）
                if !entry.text.is_empty() && !entry.text.starts_with("/skill:") {
                    self.optimistic_send(entry.text, cx);
                } else {
                    cx.notify();
                }
            }
            Some(Err(e)) => {
                // 发送失败：条目放回原位，不静默丢
                self.queued_inputs.insert(ix.min(self.queued_inputs.len()), entry);
                self.status = e;
                cx.notify();
            }
            None => {
                // 无进程：放回原位（下次 settled / 下次发送兜底）
                self.queued_inputs.insert(ix.min(self.queued_inputs.len()), entry);
                self.status = tr("status.disconnected").into();
                cx.notify();
            }
        }
    }

    /// 卡片铅笔（ZCode 撤回编辑 parity，**不是原地改**）：composer 有草稿
    /// 则拒绝（先发送或清空）；否则该条退场，文本+附件回填草稿（置前——
    /// 撤回的比正在写的旧，与 RecallInput 同口径）。经 with_active_editor
    /// 调用，草稿回拉由它完成。
    pub(crate) fn queue_withdraw(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.queued_inputs.len() {
            return;
        }
        if !self.input.trim().is_empty() {
            self.status = tr("queue.withdraw_conflict").into();
            cx.notify();
            return;
        }
        let entry = self.queued_inputs.remove(ix);
        if entry.text.is_empty() {
            self.input = String::new();
        } else if self.input.is_empty() {
            self.input = entry.text.clone();
        } else {
            self.input = format!("{}\n\n{}", entry.text, self.input);
        }
        self.pending_images.splice(..0, entry.images);
        self.set_queued_status();
        // 草稿/附件回拉 composer 由 with_active_editor 统一完成
        cx.notify();
    }

    /// abort 配套（090 定批：中止时队列**全部退回编辑器**）：本地队列整体
    /// 退场，文本 \n\n 拼接、附件置于 pending_images 队首；文本同时并入
    /// runtime 草稿（后台会话唯一去向）。返回 (拼接文本, 附件) 供
    /// RecallInput 冒泡——活跃会话由 pool.rs 回填 composer。
    pub(crate) fn restore_queue_to_editor(&mut self) -> (String, Vec<AttachedImage>) {
        if self.queued_inputs.is_empty() {
            return (String::new(), Vec::new());
        }
        let mut texts: Vec<String> = Vec::new();
        let mut images: Vec<AttachedImage> = Vec::new();
        for e in self.queued_inputs.drain(..) {
            if !e.text.is_empty() {
                texts.push(e.text);
            }
            images.extend(e.images);
        }
        let joined = texts.join("\n\n");
        if !joined.is_empty() {
            self.input = if self.input.is_empty() {
                joined.clone()
            } else {
                format!("{}\n\n{}", joined, self.input)
            };
        }
        self.pending_images.splice(..0, images.clone());
        (joined, images)
    }

    /// flush 本地队列：仅在空闲（!agent_running）且非压缩时发队首。
    /// 调用点：AgentSettled 臂 / compact response / 入队瞬间（空闲发送立即
    /// 开跑）。发 `Prompt{streaming_behavior: None}`：pi 空闲直接开跑；
    /// 万一实际在跑（AgentEnd..Settled 续跑窗的猜测误差）pi 会**拒绝**而
    /// 非静默入队——response 臂放回队首等下一次 settled，护栏闭环。
    pub(crate) fn flush_local_queue(&mut self, cx: &mut Context<Self>) {
        if self.flush_in_flight.is_some()
            || self.compacting
            || self.agent_running
            || self.queued_inputs.is_empty()
        {
            return;
        }
        // 队首 → wire 命令（owned，先于 session 借用构建）
        let entry = &self.queued_inputs[0];
        let cmd = Command::Prompt {
            message: entry.text.clone(),
            images: images_to_wire(&entry.images),
            streaming_behavior: None,
        };
        // 040/043 承诺：消息被扣住期间 send_input 的空闲重绑分支不会跑，
        // flush 补上同一承诺（扩展清单/MCP 重绑）。失败（spawn 失败）留队。
        if !self.rebind_pending(cx) {
            return;
        }
        let Some(session) = &self.agent.session else {
            return; // 无进程：留着，spawn 后的下一次 settled / 发送兜底
        };
        match session.send(&cmd) {
            Ok(id) => {
                self.flush_in_flight = Some(id);
                self.status = "running".into();
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: &mut [QueuedInput]) -> Vec<String> {
        v.iter().map(|q| q.text.clone()).collect()
    }

    fn item(text: &str) -> QueuedInput {
        QueuedInput {
            text: text.into(),
            images: Vec::new(),
            remote: false,
        }
    }

    #[test]
    fn move_down_and_up() {
        let mut v = vec![item("a"), item("b"), item("c")];
        assert!(move_entry(&mut v, 0, 2)); // a → c 之前
        assert_eq!(n(&mut v), ["b", "a", "c"]);
        assert!(move_entry(&mut v, 2, 0)); // c → 顶
        assert_eq!(n(&mut v), ["c", "b", "a"]);
    }

    #[test]
    fn self_and_neighbor_are_no_ops() {
        let mut v = vec![item("a"), item("b"), item("c")];
        assert!(!move_entry(&mut v, 1, 1)); // 拖回原地（上缘）
        assert!(!move_entry(&mut v, 1, 2)); // 自己下缘
        assert_eq!(n(&mut v), ["a", "b", "c"]);
    }

    #[test]
    fn stale_targets_are_safe() {
        let mut v = vec![item("a"), item("b")];
        assert!(move_entry(&mut v, 0, 99), "越界钳尾");
        assert_eq!(n(&mut v), ["b", "a"]);
        assert!(!move_entry(&mut v, 5, 0), "from 越界 = false");
    }
}
