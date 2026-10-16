//! psp 项目行拖拽排序（020，Zed project_panel drag_and_drop 同款三件套）：
//! `on_drag` 发起（载荷 = 组 ws_key，幽灵 = folder + 名字小签 =「收起为
//! 标签」）、行侧 `on_drag_move` 半行判定插入位、容器 `on_drop` 提交重排
//! 并持久化用户排序。
//!
//! 状态生命周期：gpui 的拖拽在列表外松手 = 静默取消，不派发任何事件——
//! 状态里的 `at` 时刻被每次 drag_move 续期，看门狗按 staleness 兜底清理
//! （拖出列表区的清除由容器 on_drag_move 的 contains=false 实时做，看门狗
//! 只管「出窗后松手」这种再无事件的死法）。

use std::time::{Duration, Instant};

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme;
use crate::services::workspace::same_ws_key;
use crate::ui::icon;

/// 拖拽中的项目行收起为标签后，看门狗判定「再无 drag_move 续期」的阈值。
/// 任何一次 mouse move（窗口内）都会续期，正常拖拽远快于该节奏。
const DRAG_STALE: Duration = Duration::from_millis(700);

/// 看门狗轮询间隔（< DRAG_STALE，清理延迟最多一个周期）。
const WATCHDOG_TICK: Duration = Duration::from_millis(400);

/// 项目行拖拽载荷（`on_drag` 的 value；drop 监听按 TypeId 匹配，与文件树
/// 的 TreeDrag 互不串扰）。key = 组 ws_key。
#[derive(Debug, Clone)]
pub(crate) struct ProjDragTab {
    pub(crate) key: String,
}

/// 拖拽进行中的 Chat 态快照：key 对齐当前载荷，`at` = 最近一次 drag_move
/// 时刻（看门狗 staleness 判据；渲染读它判定「收起为标签」与插入线）。
#[derive(Debug, Clone)]
pub(crate) struct ProjDragState {
    pub(crate) key: String,
    pub(crate) at: Instant,
}

/// 组的排序键（用户排序表 / 拖拽载荷统一用它，与 collapsed_keys 同源）。
pub(crate) fn group_key(g: &crate::ProjectGroup) -> String {
    same_ws_key(&g.path.to_string_lossy())
}

/// 拖拽 move 的状态同步漏斗（容器与行侧共用）：在列表区内续期/重建状态，
/// 拖出列表区立即清态（此刻用户已把项目拖离合法落区，指示线该消失）。
/// 重建点 = 状态缺失（上轮取消后再入）或键不符（新一轮拖拽）。
pub(crate) fn sync_drag(chat: &mut Chat, key: String, inside: bool, cx: &mut gpui::Context<Chat>) {
    if inside {
        if chat.psp.proj_drag.as_ref().is_some_and(|d| d.key == key) {
            chat.psp.proj_drag.as_mut().unwrap().at = Instant::now();
        } else {
            // 重建点：状态缺失（上轮取消后再入，看门狗重新起一个）或键
            // 不符（新一轮拖拽，旧看门狗仍在轮询会自然接手/退出）
            let fresh = chat.psp.proj_drag.is_none();
            chat.psp.proj_drag = Some(ProjDragState { key, at: Instant::now() });
            chat.psp.proj_drop_ix = None;
            if fresh {
                spawn_watchdog(cx);
            }
        }
        // drop_ix 由行侧 handler 另算（需要行几何），这里只管存在性
    } else if chat.psp.proj_drag.is_some() || chat.psp.proj_drop_ix.is_some() {
        chat.psp.proj_drag = None;
        chat.psp.proj_drop_ix = None;
        cx.notify();
    }
}

/// 拖拽落位重排：把 `key` 的组移到插入位 `target`（拖拽前的下标空间，
/// 0..=len）。返回是否真的移动——拖回原地（target == cur）与相邻
/// （target == cur+1，即自己下缘）都是 no-op。
pub(crate) fn apply_drop(
    projects: &mut Vec<crate::ProjectGroup>,
    key: &str,
    target: usize,
) -> bool {
    let Some(cur) = projects.iter().position(|g| group_key(g) == key) else {
        return false;
    };
    if target == cur || target == cur + 1 {
        return false;
    }
    let g = projects.remove(cur);
    let t = if target > cur { target - 1 } else { target };
    projects.insert(t.min(projects.len()), g);
    true
}

/// 容器 on_drop 的提交路径：清拖拽态；有合法落点就重排并把结果记为用户
/// 排序持久化（020：拖完下次打开依旧）。无落点（原地松手）= 纯取消。
pub(crate) fn commit_drop(chat: &mut Chat, target: Option<usize>, cx: &mut gpui::Context<Chat>) {
    let key = chat.psp.proj_drag.as_ref().map(|d| d.key.clone());
    chat.psp.proj_drag = None;
    chat.psp.proj_drop_ix = None;
    if let (Some(key), Some(target)) = (key, target)
        && apply_drop(&mut chat.psp.projects, &key, target)
    {
        chat.psp.proj_order = Some(chat.psp.projects.iter().map(group_key).collect());
        chat.persist_ui();
    }
    cx.notify();
}

/// 拖拽看门狗：gpui 对「窗口外松手」不派发任何事件（active_drag 静默清
/// 零），拖拽态只能靠 `at` staleness 兜底——周期检查，无续期即清态收场
/// （收起的组复原、插入线消失）。有续期则继续轮询到拖拽结束为止。
fn spawn_watchdog(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(WATCHDOG_TICK).await;
            let alive = this
                .update(cx, |chat, cx| match &chat.psp.proj_drag {
                    None => false,
                    Some(d) => {
                        if d.at.elapsed() > DRAG_STALE {
                            chat.psp.proj_drag = None;
                            chat.psp.proj_drop_ix = None;
                            cx.notify();
                            false
                        } else {
                            true
                        }
                    }
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        }
    })
    .detach();
}

/// 拖拽幽灵（gpui 把它画在光标处）：folder + 项目名小签——「项目拖拽时
/// 收起为标签」的可见形态（TreeDragGhost 同款骨架）。
pub(crate) struct ProjDragGhost {
    pub(crate) name: String,
}

impl gpui::Render for ProjDragGhost {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = theme::theme();
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(4.))
            .max_w(px(260.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.nav))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text))
            .child(icon("folder", 14., t.text_dim))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(self.name.clone())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::apply_drop;
    use crate::ProjectGroup;
    use std::path::PathBuf;

    fn group(name: &str) -> ProjectGroup {
        ProjectGroup {
            name: name.to_string(),
            path: PathBuf::from(format!(r"D:\{name}")),
            sessions: Vec::new(),
        }
    }

    fn names(projects: &[ProjectGroup]) -> Vec<String> {
        projects.iter().map(|g| g.name.clone()).collect()
    }

    #[test]
    fn move_down_and_up() {
        // a 下移到 c 之前（target=2）→ [b,a,c]；c 上移到顶（target=0）→ [c,a,b]
        let mut p = vec![group("a"), group("b"), group("c")];
        assert!(apply_drop(&mut p, r"d:\a", 2));
        assert_eq!(names(&p), ["b", "a", "c"]);
        assert!(apply_drop(&mut p, r"d:\c", 0));
        assert_eq!(names(&p), ["c", "b", "a"]);
    }

    #[test]
    fn self_and_neighbor_targets_are_no_ops() {
        // 拖回自己（上缘 cur / 下缘 cur+1）都不动
        let mut p = vec![group("a"), group("b"), group("c")];
        assert!(!apply_drop(&mut p, r"d:\b", 1));
        assert!(!apply_drop(&mut p, r"d:\b", 2));
        assert_eq!(names(&p), ["a", "b", "c"]);
    }

    #[test]
    fn stale_and_unknown_targets_are_safe() {
        // 陈旧落点越界钳尾；载荷键不存在 = False
        let mut p = vec![group("a"), group("b")];
        assert!(apply_drop(&mut p, r"d:\a", 99));
        assert_eq!(names(&p), ["b", "a"]);
        assert!(!apply_drop(&mut p, r"d:\missing", 0));
        assert_eq!(names(&p), ["b", "a"]);
    }
}
