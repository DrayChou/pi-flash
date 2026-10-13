//! psp / dock (v54): 项目+会话一体列表（ChatGPT 式，设计核心=隐藏复杂度）。
//! dock = statusbar 三面板容器（psp / 文件树 / git），nav 底色，右缘 6px
//! 拖拽调宽（250–500，双击复位 282）。psp 的悬浮层（路径 tooltip / 会话
//! 详情卡 / 两级菜单 / 删除确认）由 `psp_overlays` 在根渲染挂载。

pub(crate) mod file_tree;
pub(crate) mod file_actions;
pub(crate) mod psp_actions;
pub(crate) mod git_panel;
pub(crate) mod psp_overlays;

use gpui::{Context, Entity, MouseButton, SharedString, div, prelude::*, px, rgb};

use std::path::PathBuf;

use crate::ui::TextInput;
use crate::Chat;
use crate::DockPanel;
use crate::i18n::tr;
use crate::services::workspace::{same_ws, same_ws_key};
use crate::ListMode;
use crate::SortMode;
use crate::appearance::theme::{Theme, theme as T};
use crate::ui::{WithTip, icon, icon_hover, spinner};

/// Flattened psp row model (one virtual list over all rows).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PspRow {
    Title,
    Project(usize),
    Session { p: usize, s: usize },
    /// 伪标签（draft 已发送、pi 未落盘的占位行；key = 池 key draft-N）
    Pending { key: String },
    /// 空项目展开时的「无会话」占位
    Empty,
    /// 「显示更多」行（key = 组 ws_key；Flat 模式用全局键）
    More { key: String },
}

/// 每页显示的会话标签数（学 zcode：「显示更多」每次续一页）——默认值来自
/// 设置-其他「会话显示数」（3-10）。
fn psp_page() -> usize {
    crate::services::workspace::session_display_count()
}

pub(crate) fn psp_rows(chat: &Chat) -> Vec<PspRow> {
    let mut rows = vec![PspRow::Title];
    // 伪标签归属：cwd 匹配项目组的插在组内会话行之前（它就是该项目最新的
    // 活动）；没有任何组可归（该项目还没有已落盘会话）的插在 Title 后。
    // 折叠的组里伪标签照常显示——它正是用户刚发出的那条消息的标签。
    let orphan_pending: Vec<String> = chat
        .psp.pending_tabs
        .iter()
        .filter(|e| !chat.psp.projects.iter().any(|g| same_ws(&g.path.to_string_lossy(), &e.cwd)))
        .map(|e| e.key.clone())
        .collect();
    match chat.psp.list_mode {
        ListMode::Grouped => {
            for (pi, g) in chat.psp.projects.iter().enumerate() {
                rows.push(PspRow::Project(pi));
                let key = same_ws_key(&g.path.to_string_lossy());
                for e in chat
                    .psp.pending_tabs
                    .iter()
                    .filter(|e| same_ws(&g.path.to_string_lossy(), &e.cwd))
                {
                    rows.push(PspRow::Pending { key: e.key.clone() });
                }
                if chat.psp.collapsed_keys.contains(&key) {
                    continue;
                }
                if g.sessions.is_empty() {
                    rows.push(PspRow::Empty);
                } else {
                    let shown = chat
                        .psp.psp_shown
                        .get(&key)
                        .copied()
                        .unwrap_or_else(psp_page)
                        .min(g.sessions.len());
                    for si in 0..shown {
                        rows.push(PspRow::Session { p: pi, s: si });
                    }
                    if g.sessions.len() > shown {
                        rows.push(PspRow::More { key });
                    }
                }
            }
        }
        ListMode::Flat => {
            // Flat：伪标签按插入序排列表最前（最新活动优先）
            for e in &chat.psp.pending_tabs {
                rows.push(PspRow::Pending { key: e.key.clone() });
            }
            let mut all: Vec<(usize, usize)> = Vec::new();
            for (pi, g) in chat.psp.projects.iter().enumerate() {
                for si in 0..g.sessions.len() {
                    all.push((pi, si));
                }
            }
            if chat.psp.sort_mode == SortMode::Time {
                all.sort_by(|a, b| {
                    let ma = chat.psp.projects[a.0].sessions[a.1].modified;
                    let mb = chat.psp.projects[b.0].sessions[b.1].modified;
                    mb.cmp(&ma)
                });
            }
            let key = "__flat__".to_string();
            let shown = chat.psp.psp_shown.get(&key).copied().unwrap_or_else(psp_page);
            let total = all.len();
            all.truncate(shown);
            all.into_iter().for_each(|(p, s)| rows.push(PspRow::Session { p, s }));
            if total > shown {
                rows.push(PspRow::More { key });
            }
        }
    }
    // 无组可归的伪标签（该项目组还没被 rebuild_projects 建出来）：插在
    // Title 之后置顶——用户刚发的消息必须是第一眼能看到的东西
    for (i, key) in orphan_pending.into_iter().enumerate() {
        rows.insert(1 + i, PspRow::Pending { key });
    }
    rows
}

impl Chat {

    /// Group the cross-project session scan into ≤N project groups (current
    /// project pinned first, then by newest session mtime desc).
    pub(crate) fn rebuild_projects(&mut self, all: Vec<pi_link::sessions::SessionInfo>) {
        let mut groups: Vec<crate::ProjectGroup> = Vec::new();
        for s in all {
            let cwd = PathBuf::from(s.cwd.clone());
            if let Some(g) = groups
                .iter_mut()
                .find(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy()))
            {
                g.sessions.push(s);
            } else {
                groups.push(crate::ProjectGroup {
                    name: cwd
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| cwd.to_string_lossy().to_string()),
                    path: cwd,
                    sessions: vec![s],
                });
            }
        }
        let cwd = self.ws.cwd.clone();
        if !groups.iter().any(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy())) {
            groups.push(crate::ProjectGroup {
                name: cwd
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| cwd.to_string_lossy().to_string()),
                path: cwd.clone(),
                sessions: Vec::new(),
            });
        }
        // current pinned first, the rest by newest session mtime desc
        groups.sort_by(|a, b| {
            let ka = same_ws(&a.path.to_string_lossy(), &cwd.to_string_lossy());
            let kb = same_ws(&b.path.to_string_lossy(), &cwd.to_string_lossy());
            match (ka, kb) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => newest(b).cmp(&newest(a)),
            }
        });
        // v60: 加载单位是会话（设置.默认加载会话数）——组由这批会话的
        // cwd 自然形成，不再按项目数截断
        self.psp.projects = groups;
        self.sync_current_sessions();
    }

    /// 启动自动折叠（其他页「自动折叠时间」档位 1/3/7 天，内部按小时算）：
    /// 项目内**所有**会话的最后活动时间都早于阈值 → 该项目组默认折叠。
    /// 只在启动装载项目集后调一次（`startup::spawn_session_list_load`）——
    /// 运行期 `refresh_sessions` 等重建不得再折叠，否则用户手动展开的组
    /// 会被下一轮重建打回去。空组（无会话）不折叠；当前项目永不折叠
    /// （钉顶的活动项目折叠等于看不见自己刚开的会话）。
    pub(crate) fn auto_collapse_stale_projects(&mut self) {
        let hours = crate::services::workspace::auto_collapse_hours();
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(hours * 3600);
        let cwd = self.ws.cwd.to_string_lossy().to_string();
        for g in &self.psp.projects {
            if g.sessions.is_empty() {
                continue;
            }
            if same_ws(&g.path.to_string_lossy(), &cwd) {
                continue;
            }
            if newest(g) < cutoff {
                self.psp.collapsed_keys
                    .insert(same_ws_key(&g.path.to_string_lossy()));
            }
        }
    }

    /// Keep `sessions` (current project) in sync with the matching group.
    pub(crate) fn sync_current_sessions(&mut self) {
        let cwd = self.ws.cwd.to_string_lossy().to_string();
        self.pool.sessions = self
            .psp.projects
            .iter()
            .find(|g| same_ws(&g.path.to_string_lossy(), &cwd))
            .map(|g| g.sessions.clone())
            .unwrap_or_default();
    }
}

fn newest(g: &crate::ProjectGroup) -> std::time::SystemTime {
    g.sessions
        .iter()
        .map(|s| s.modified)
        .max()
        .unwrap_or(std::time::UNIX_EPOCH)
}

// ---------------------------------------------------------------------------
// psp view
// ---------------------------------------------------------------------------

pub(crate) fn psp_view(
    chat: &mut Chat,
    _entity: Entity<Chat>,
    weak: gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    // title 行固定在 dock 顶部（操作行永远可见），只有列表滚动。
    // 之前 title 行放滚动容器内时，其按钮 hitbox 被 scroll mask 裁掉，
    // 点击全部失效——这也是设计的本意：title 行不是列表内容。
    let t = T();
    // 统一对齐线（2026-10-13 定稿）：topbar 收放钮贴左 5px（30px 钮盒、
    // 18px 图标居中）→ 图标视觉线 = 5+6 = 11px。列表侧 col 外层 px 10 是
    // 唯一水平留白源，行内 pl 1 是对齐补偿 = 10+1 = 11px，与钮图标同线；
    // title 行挂 pl 1（文字同线）。右线不动（行 pr 10 / title pr 10）。
    let mut col = div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .px(px(10.))
        .pt(px(6.))
        .child(title_row(chat, &weak, t));
    let rows: Vec<gpui::AnyElement> = psp_rows(chat)
        .into_iter()
        .filter(|row| !matches!(row, PspRow::Title))
        .map(|row| match row {
            PspRow::Project(pi) => project_row(chat, pi, &weak, t),
            PspRow::Session { p, s } => session_row_view(chat, p, s, &weak, t, window, cx),
            PspRow::Pending { key } => pending_row_view(chat, &key, &weak, t),
            PspRow::Empty => div()
                .id("psp-empty")
                .pl(px(45.))
                .py(px(5.5))
                // 与「显示更多」行同款：placeholder 色 + 面板字号−1（2026-10-10 定夺）
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(tr("psp.no_sessions"))
                .into_any_element(),
            PspRow::More { key } => {
                let weak_more = weak.clone();
                let key_more = key.clone();
                div()
                    .id(SharedString::from(format!("psp-more-{key}")))
                    .pl(px(45.))
                    .py(px(5.5))
                    // placeholder 色 + 面板字号−1（2026-10-10 定夺）
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_faint))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let key = key_more.clone();
                        let _ = weak_more.update(cx, |chat, cx| {
                            *chat.psp.psp_shown.entry(key).or_insert_with(psp_page) += psp_page();
                            cx.notify();
                        });
                    })
                    .child(tr("settings.show_more"))
                    .into_any_element()
            }
            PspRow::Title => div().into_any_element(),
        })
        .collect();
    // psp 会话列表滚动 + 右缘滚动条。Scrollbar 挂在与滚动容器平级的
    // relative 包裹上（贴 dock 右缘，不吃容器内 10px 留白）；ZED Regular
    // 视觉：只有 thumb 无 track（track 色已在 sync_gpui_tokens 透明化）。
    col = col.child(
        div()
            .id("psp-scroll-wrap")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .id("psp-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&chat.psp.psp_scroll)
                    // 鼠标进 panel → 滚动条立现；离开 → 3s 后淡出（自绘
                    // psp_scrollbar 读这两个状态自绘透明度）
                    .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                        this.psp.psp_sb_state.set_parent_hovered(*hovered);
                        cx.notify();
                    }))
                    // 滚动时立刻关掉全部浮层（详情卡/⋯菜单/项目tooltip/删除
                    // 确认）——它们锚定的是行位置，滚动后锚点漂移，钉在原地
                    // 只会错位（用户定稿：滚动 = 菜单立即消失）
                    .on_scroll_wheel(cx.listener(|this, _: &gpui::ScrollWheelEvent, _w, cx| {
                        if this.dlg.hover_card.is_some()
                            || this.dlg.psp_menu.is_some()
                            || this.dlg.proj_tip.is_some()
                            || this.dlg.confirm_prj_del.is_some()
                        {
                            this.dlg.hover_card = None;
                            this.dlg.psp_menu = None;
                            this.dlg.proj_tip = None;
                            this.dlg.confirm_prj_del = None;
                            cx.notify();
                        }
                    }))
                    // 水平留白由 col 外层统一（10px），此处不再叠加
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            // 自绘滚动条（ZED Regular 移植）：贴边 0px；无 track；panel 内
            // 常显、离开 3s+1s 淡出；thumb 悬停加宽；事件不穿透
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .bottom(px(0.))
                    .right(px(-10.))
                    .w(px(10.))
                    .child(crate::ui::psp_scrollbar::psp_scrollbar(
                        &chat.psp.psp_sb_state,
                        &chat.psp.psp_scroll,
                    )),
            ),
    );
    col.into_any_element()
}

/// title 行（操作行）：左文案（项目/会话），右 4 常显钮。
fn title_row(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    let label = match chat.psp.list_mode {
        ListMode::Flat => tr("psp.session_label"),
        _ => tr("scope.project"),
    };
    let w_open = weak.clone();
    let w_new = weak.clone();
    let w_search = weak.clone();
    let w_sort = weak.clone();
    div()
        .id("psp-title")
        .w_full()
        .h(px(30.))
        .flex()
        .items_center()
        .pl(px(1.))
        .pr(px(10.))

        .text_size(crate::appearance::ui_size(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_soft))
        .child(div().flex_1().child(SharedString::from(label)))
        .child(
            div()
                .ml_auto()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap(px(10.))

                // 打开项目（iconfont 实底）
                .child(
                    div()
                        .id("psp-open-project")
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .with_tip_action("psp.open_project", &crate::OpenProjectPicker)
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w_open.update(cx, |c, cx| c.open_project_picker(false, cx));
                        })
                        .child(icon_hover("icon-project", 18., t.text_dim)),
                )
                // 新建会话（19px 实底，ml 2）
                .child(
                    div()
                        .id("psp-new-session")
                        .ml(px(2.))
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .with_tip_action("psp.new_session", &crate::NewSession)
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w_new.update(cx, |c, cx| c.new_session(cx));
                        })
                        .child(icon_hover("icon-new-chat", 19., t.text_dim)),
                )
                // 会话查询（lucide search）
                .child(
                    div()
                        .id("psp-search")
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .with_tip_action("psp.search_sessions", &crate::OpenSessionSearch)
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w_search.update(cx, |c, cx| c.open_session_search(cx));
                        })
                        .child(icon_hover("search", 18., t.text_dim)),
                )
                // 排序（ellipsis）
                .child(
                    div()
                        .id("psp-sort-menu")
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .with_tip("psp.sort")
                        .on_mouse_down(MouseButton::Left, move |ev: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = w_sort.update(cx, |c, cx| {
                                // 存原始点击点；方向与间距由渲染期统一弹出
                                // 算法（ui::popup）决定，此处不再手工偏移
                                c.dlg.psp_menu = Some(crate::function_panel::PspMenu::Sort {
                                    sub: None,
                                    x: f32::from(ev.position.x),
                                    y: f32::from(ev.position.y),
                                });
                                cx.notify();
                            });
                        })
                        .child(icon_hover("ellipsis", 18., t.text_dim)),
                ),
        )
        .into_any_element()
}

/// 项目行：folder 开合图标 + 名字；点击 = 组收起/展开；hover 显 ⋯/＋。
fn project_row(
    chat: &Chat,
    pi: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(g) = chat.psp.projects.get(pi) else {
        return div().into_any_element();
    };
    let active = same_ws(&g.path.to_string_lossy(), &chat.ws.cwd.to_string_lossy());
    let collapsed = chat.psp.collapsed_keys.contains(&same_ws_key(&g.path.to_string_lossy()));
    let hovered = chat.psp.hovered_project == Some(pi)
        // ⋯ 菜单打开中：按钮保持显示（交互进行中不消失）
        || matches!(
            &chat.dlg.psp_menu,
            Some(crate::function_panel::PspMenu::Project { path: mp, .. })
            if crate::services::workspace::same_path(mp, &g.path)
        );
    let path = g.path.clone();
    let path_toggle = path.clone();
    let path_tip = path.clone();
    let path_menu = path.clone();
    let name: SharedString = g.name.clone().into();
    let w_toggle = weak.clone();
    let w_menu = weak.clone();
    let w_new = weak.clone();
    let w_tip_on = weak.clone();
    let w_tip_off = weak.clone();

    div()
        .id(SharedString::from(format!("prj-{pi}")))
        // §5.8 主列表档真行高 32（原 33=32+mb2 补偿，mb 写法废除）
        .h(px(crate::ui::tokens::row::MAIN))
        // 滚动容器 overflow 压缩行时 flex-shrink 会把行压到内容高
        // （min-height:auto），hover 按钮一现（28 高）行高就跳变、整列
        // 下移（「行浮动」）——列表行一律禁止收缩
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(1.))
        .pr(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        // v55：项目行悬停不给背景色（避免与会话行选中态混淆）
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = w_toggle.update(cx, |c, cx| {
                let key = crate::services::workspace::same_ws_key(&path_toggle.to_string_lossy());
                if !c.psp.collapsed_keys.remove(&key) {
                    c.psp.collapsed_keys.insert(key);
                }
                c.persist_ui();
                cx.notify();
            });
        })
        .on_hover(move |h, _, cx| {
            let _ = w_tip_on.update(cx, |c, cx| {
                let on = *h;
                if on {
                    if c.psp.hovered_project != Some(pi) {
                        c.psp.hovered_project = Some(pi);
                        cx.notify();
                    }
                } else {
                    if c.psp.hovered_project == Some(pi) {
                        c.psp.hovered_project = None;
                    }
                    if c.dlg.proj_tip.is_some() {
                        c.dlg.proj_tip = None;
                        cx.notify();
                    }
                }
            });
        })
        .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
            let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
            let _ = w_tip_off.update(cx, |c, cx| {
                let path = path_tip.clone();
                let path_str = path.to_string_lossy().to_string();
                if c.dlg.proj_tip.as_ref().is_none_or(|(p, _, _)| {
                    !same_ws(&p.to_string_lossy(), &path_str)
                }) {
                    c.dlg.proj_tip = Some((path, x, y));
                    cx.notify();
                } else if let Some(p) = c.dlg.proj_tip.as_mut() {
                    p.1 = x;
                    p.2 = y;
                }
            });
        })
        .child(icon(
            if collapsed { "folder" } else { "folder-open" },
            15.,
            t.text_dim,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                // 项目标签 = 面板设置值（字体大小设置.md §1）
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::MEDIUM
                })
                .text_color(if active { rgb(t.text) } else { rgb(t.text_dim) })
                .child(name),
        )
        .when(hovered, |d| {
            d.child(
                div()
                    .flex()
                    .gap(px(10.))
                    // ⋯ 项目菜单
                    .child(
                        div()
                            .id(SharedString::from(format!("prj-menu-{pi}")))
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_color(rgb(t.text_dim))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(t.text)))
                            .on_mouse_down(
                                MouseButton::Left,
                                move |ev: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    let _ = w_menu.update(cx, |c, cx| {
                                        // 同排序菜单：存原始点击点，定向交统一算法
                                        c.dlg.psp_menu = Some(crate::function_panel::PspMenu::Project {
                                            path: path_menu.clone(),
                                            x: f32::from(ev.position.x),
                                            y: f32::from(ev.position.y),
                                        });
                                        cx.notify();
                                    });
                                },
                            )
                            .child(icon_hover("ellipsis", 18., t.text_dim)),
                    )
                    // ＋ 新会话入组
                    .child(
                        div()
                            .id(SharedString::from(format!("prj-new-{pi}")))
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_color(rgb(t.text_dim))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(t.text)))
                            .with_tip("psp.new_session_in_project")
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = w_new.update(cx, |c, cx| {
                                    c.new_session_in(path.clone(), cx);
                                });
                            })
                            .child(icon_hover("plus", 18., t.text_dim)),
                    ),
            )
        })
        .into_any_element()
}

/// 会话行：15px 状态槽（运行=旋转圈 / 未读=绿点 / 空）+ 标题；hover 详情卡。
fn session_row_view(
    chat: &Chat,
    p: usize,
    s: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    cx: &gpui::Context<Chat>,
) -> gpui::AnyElement {
    let Some(g) = chat.psp.projects.get(p) else {
        return div().into_any_element();
    };
    let Some(info) = g.sessions.get(s) else {
        return div().into_any_element();
    };
    let is_active = chat
        .psp.active_file
        .as_deref()
        .is_some_and(|f| crate::services::workspace::same_path(f, &info.path));
    // 运行态**每帧现算**：v54 把它缓存进 chat.running_files，而那份缓存的唯一
    // 写入点是 SessionEvent::Changed 订阅（只在进程退出 / 手动压缩 / 换工具预设
    // 时发），正常一轮的 AgentStart/AgentSettled/AgentEnd 都不发 Changed →
    // 缓存恒空，状态槽的旋转圈再也不出现。回到渲染期直接扫常驻 runtime。
    let running = chat.pool.runtimes.values().any(|rt| {
        let r = rt.read(cx);
        r.file.as_deref() == Some(info.path.as_path())
            && (r.agent_running || r.state.as_ref().is_some_and(|s| s.is_streaming))
    });
    let title: SharedString = info
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .map(SharedString::from)
        .unwrap_or_else(|| {
            if info.preview.is_empty() {
                "(empty)".into()
            } else {
                info.preview.clone().into()
            }
        });
    let path = info.path.clone();
    let path_click = path.clone();
    let path_card = path.clone();
    let path_bounds = path.clone();
    let w_click = weak.clone();
    let w_hover = weak.clone();
    let w_move = weak.clone();
    let w_bounds = weak.clone();

    // ---- ZCode TaskListItem 样式 parity（源码实测值）----
    // 列表 p-3 留白 + 行 pl-2.5/pr-1/py-1 + space-y-0.5 行距 + rounded-lg；
    // 选中/悬停 = 中性前景薄纱（10% / 5%，非 accent——accent 底会与毛玻璃
    // overlay 互相染色）；标题溢出 = 尾部 24px 渐隐，**量宽确认溢出才挂**
    // （短标题不糊尾）。overlay 尾色 = 行底合成视觉色（不透明）：透明端必
    // 须同色 a=0，透明黑会在 HSL 插值中段产生暗带。
    let row_sel = gpui::rgba((t.text << 8) | 0x1a); // 10%（悬停与选中同色）
    let row_base = if is_active {
        gpui::rgb(crate::appearance::theme::mix_rgb(t.text, t.nav, 0.1))
    } else {
        gpui::rgb(t.nav)
    };
    let fade_to = gpui::Hsla::from(row_base);
    let fade = gpui::linear_gradient(
        90.,
        gpui::linear_color_stop(gpui::Hsla { a: 0., ..fade_to }, 0.),
        gpui::linear_color_stop(fade_to, 1.),
    );
    // 悬停态行底变成 10% 白纱（over nav），渐隐尾色必须跟着换白纱的合成
    // 视觉色——否则纯 nav 的 overlay 盖在白纱底上就是一块深色（黑块回归）
    let fade_hover_to = gpui::Hsla::from(gpui::rgb(crate::appearance::theme::mix_rgb(t.text, t.nav, 0.1)));
    let fade_hover = gpui::linear_gradient(
        90.,
        gpui::linear_color_stop(gpui::Hsla { a: 0., ..fade_hover_to }, 0.),
        gpui::linear_color_stop(fade_hover_to, 1.),
    );

    // 溢出测量：标题真实排版宽 vs 行内可用宽（容器 px 10×2 + 行 pl 1/pr 10
    // + slot 15 + 两个 gap 16 + 时间列 38）
    let panel_family = crate::appearance::panel_font().family;
    let measure_font = gpui::Font {
        family: panel_family.clone().into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let measure_run = gpui::TextRun {
        len: title.len(),
        font: measure_font,
        color: rgb(t.text).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    // 量宽字号必须 = 渲染字号（ui_size(12.)，随界面字号设置缩放）。
    // 曾经写死 px(13.)：设置 ≥14 时低估标题宽 → 溢出不挂渐隐、生硬截断。
    let title_overflows = {
        let measure_size = crate::appearance::ui_size(12.);
        let title_w = window
            .text_system()
            .layout_line(&title, measure_size, &[measure_run], None)
            .width;
        let available = px(chat.layout.slp_w - 20. - 11. - 15. - 16. - 38.);
        title_w > available
    };

    // 行顶对齐钩子：详情卡的 top = 本行 bounds.origin.y（ZCode 用
    // getBoundingClientRect；gpui 等价物 = wrapper 的 children_prepainted）。
    // 自下而上进入行时鼠标首事件落在行底，只靠 mouse move 会偏下——这里
    // 用真实 bounds 校正；bounds 稳定后不再 notify。滚动时自动跟随。
    // 行高 h 同步写入：详情卡向上弹要贴行底（y + h）。
    let weak_for_row_el = weak.clone();
    let _ = weak_for_row_el;
    div()
        // 与项目行同理：滚动容器 overflow 压缩时会连内部行一起压（内层
        // h(32) 被 wrapper 的收缩顶掉），列表行一律禁止收缩
        .flex_shrink_0()
        .on_children_prepainted(move |children: Vec<gpui::Bounds<gpui::Pixels>>, _, cx| {
            if let Some(b) = children.first() {
                let top = b.origin.y;
                let h = b.size.height;
                let _ = w_bounds.update(cx, |c, cx| {
                    if let Some(card) = c.dlg.hover_card.as_mut() {
                        if crate::services::workspace::same_path(&card.path, &path_bounds)
                            && ((card.y - f32::from(top)).abs() > 0.5
                                || (card.h - f32::from(h)).abs() > 0.5)
                        {
                            card.y = f32::from(top);
                            card.h = f32::from(h);
                            cx.notify();
                        }
                    }
                });
            }
        })
        .child(
    div()
        .id(SharedString::from(format!("ps-{}", info.id)))
        .relative()
        .group("psrow")
        .w_full()
        // §5.8：mb 补间距写法废除，真行高 32
        .h(px(crate::ui::tokens::row::MAIN))
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(1.))
        .pr(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(is_active, |d| d.bg(row_sel).hover(|s| s.bg(row_sel)))
        .when(!is_active, |d| d.hover(|s| s.bg(row_sel)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = w_click.update(cx, |c, cx| {
                c.open_session(path_click.clone(), false, cx);
            });
        })
        .on_hover(move |h, _w, cx| {
            let leaving = !*h;
            let _ = w_hover.update(cx, |c, _cx| {
                if leaving {
                    if let Some(card) = c.dlg.hover_card.as_mut() {
                        // 鼠标若已进卡（card_hovered），行退出事件晚到
                        // 也不得启动消失宽限
                        if !card.card_hovered {
                            card.hide_at = Some(std::time::Instant::now());
                        }
                    }
                }
            });
        })
        .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
            let y = f32::from(ev.position.y);
            let _ = w_move.update(cx, |c, cx| {
                let same = c
                    .dlg.hover_card
                    .as_ref()
                    .is_some_and(|card| crate::services::workspace::same_path(&card.path, &path_card));
                if !same {
                    c.dlg.hover_card = Some(crate::function_panel::HoverCard {
                        path: path_card.clone(),
                        // 临时估计（鼠标 y - 半行高）；prepaint 钩子会把
                        // y 校正为本行的真实顶部（bounds.origin.y），行高
                        // h 随同一钩子校正（详情卡贴行底对齐要用）
                        y: y - 16.,
                        h: crate::ui::tokens::row::MAIN,
                        hide_at: None,
                        show_at: std::time::Instant::now(),
                        shown: false,
                        confirming: false,
                        card_hovered: false,
                        renaming: false,
                        rename_input: None,
                    });
                    cx.notify();
                } else if let Some(card) = c.dlg.hover_card.as_mut() {
                    // 鼠标在本行内移动：只要不在卡上就持续取消消失宽限
                    if !card.card_hovered {
                        card.hide_at = None;
                    }
                }
            });
        })
        .child(
            div()
                .size(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .child(if running {
                    spinner(15., t.accent)
                } else if chat.psp.turn_errors.contains(&info.path) {
                    // 红 = 外部错误/崩溃（t.danger，§6 danger 系唯一表达）
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(rgb(t.danger))
                        .into_any_element()
                } else if chat.psp.turn_warnings.contains(&info.path) {
                    // 黄 = 内部中断（上限截断/中止）
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(rgb(crate::appearance::theme::WARN))
                        .into_any_element()
                } else if chat.psp.unread.contains(&info.path) {
                    // 青蓝 = 完成通知（绿色退出通知语义，仅表达正确性）
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(rgb(crate::appearance::theme::NOTICE))
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        )
        .child(
            // 标题：flex_1 到时间区左缘为止（不进入时间区）；仅确认溢出时
            // 挂尾部 24px 渐隐（ZCode TaskTitleOverflowText parity）
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_family(panel_family)
                // 会话标签 = 面板设置值（字体大小设置.md §1）
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(title)
                .when(title_overflows, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(24.))
                            .bg(fade)
                            .group_hover("psrow", |s| s.bg(fade_hover)),
                    )
                }),
        )
        // 时间区（v55 fmtAgo）：固定宽右对齐成一列（宽度与左侧图标区+
        // padding 视觉平衡，标题可用空间最大化）
        .child(
            div()
                .flex_shrink_0()
                .w(px(38.))
                .flex()
                .items_center()
                .justify_end()
                // 时间 = 面板字号−2（2026-10-10 定夺，原 −1）
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(crate::services::format::fmt_ago(
                    info.modified,
                ))),
        ),
        )
        .into_any_element()
}

/// 伪标签行（draft 已发送、pi 未落盘的占位）：行高/字号/选中与悬停底色
/// 同 [`session_row_view`]（ZCode TaskListItem 样式），左侧状态槽 =
/// spinner（此刻进程正在 boot/跑，与真标签的 running 态同视觉），标题 =
/// 首条消息首行（真标签落地后的 preview 与它一致，衔接无跳变）。没有文
/// 件就没有 hover 卡 / 改名 / 删除——点击只做 switch_to。FileBound 落地
/// 后本行被真标签顶替。
fn pending_row_view(
    chat: &Chat,
    key: &str,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(entry) = chat.psp.pending_tabs.iter().find(|e| e.key == key) else {
        return div().into_any_element();
    };
    let is_active = chat.pool.active_key == entry.key;
    let title: SharedString = if entry.preview.is_empty() {
        tr("session.new").to_string().into()
    } else {
        entry.preview.clone().into()
    };
    let row_sel = gpui::rgba((t.text << 8) | 0x1a); // 10%（与 session 行同色）
    let panel_family = crate::appearance::panel_font().family;
    let key_click = entry.key.clone();
    let w_click = weak.clone();
    div()
        // 与 session 行同理：滚动容器 overflow 压缩时行不许收缩
        .flex_shrink_0()
        .child(
            div()
                .id(SharedString::from(format!("ps-draft-{key}")))
                .w_full()
                .h(px(crate::ui::tokens::row::MAIN))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(1.))
                .pr(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(is_active, |d| d.bg(row_sel).hover(|s| s.bg(row_sel)))
                .when(!is_active, |d| d.hover(|s| s.bg(row_sel)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let key = key_click.clone();
                    let _ = w_click.update(cx, |c, cx| {
                        if let Some(rt) = c.pool.runtimes.get(&key).cloned() {
                            c.switch_pool_rt(rt, cx);
                        }
                    });
                })
                .child(
                    // 状态槽（15px，与会话行同位）：伪标签期间轮次必在飞
                    div()
                        .size(px(15.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(spinner(15., t.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(panel_family)
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text))
                        .child(title),
                )
                // 时间区占位：与 session 行同宽，标题截断位对齐
                .child(div().flex_shrink_0().w(px(38.))),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// dock container (015 v54): nav 底色 + 三视图 + 右缘 resizer
// ---------------------------------------------------------------------------

pub(crate) fn dock(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let view: gpui::AnyElement = match chat.layout.dock_panel {
        DockPanel::Sessions => psp_view(chat, entity, weak.clone(), window, cx),
        DockPanel::Files => files_view(chat, weak, cx).into_any_element(),
        DockPanel::Git => git_panel::view(chat, weak, cx).into_any_element(),
    };
    div()
        .id("dock")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .bg(rgb(t.nav))
        .child(view)
        .into_any_element()
}

/// files view：树渲染在 `file_tree.rs`（vlist 虚拟化 + 展平缓存，render
/// 不碰磁盘；Zed 图标主题 + gitignore 过滤在 services::file_tree/。
/// file_icons）。
pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    file_tree::files_view(chat, weak, _cx)
}

// ---- psp/文件树 overlay 状态类型（自 shell/types.rs 下沉，本域自持）----

#[derive(Debug, Clone)]
pub(crate) struct HoverCard {
    pub path: PathBuf,
    /// 悬停行的真实顶部（prepaint 钩子校正，初始为鼠标 y − 半行高估计值）
    pub y: f32,
    /// 悬停行高（prepaint 钩子随 y 同步校正）：详情卡向上弹时卡底 = y + h
    /// （贴行底），向下弹卡顶 = y（贴行顶）——精准对齐是调用方语义
    pub h: f32,
    pub hide_at: Option<std::time::Instant>,
    /// 创建时刻：悬停满 300ms 才显示（快速滑过列表不弹卡、不干扰行悬停）
    pub show_at: std::time::Instant,
    /// show_at 到期后由 120ms tick 置 true 并渲染一次
    pub shown: bool,
    pub confirming: bool,
    /// 鼠标当前是否在卡上（gpui 的行退出/卡进入事件顺序不保证，
    /// 行退出只在 !card_hovered 时才启动消失宽限）
    pub card_hovered: bool,
    /// 标题点击后原地改名（设计稿：卡内变输入框，Enter 提交 / Esc 取消）
    pub renaming: bool,
    pub rename_input: Option<gpui::Entity<crate::ui::TextInput>>,
}

#[derive(Debug, Clone)]
pub(crate) enum PspMenu {
    /// sub: 0=列表方式, 1=排序方式
    Sort { sub: Option<u8>, x: f32, y: f32 },
    Project { path: PathBuf, x: f32, y: f32 },
}

#[derive(Debug, Clone)]
pub(crate) enum TreeMenu {
    Dir { path: PathBuf, x: f32, y: f32 },
    File { path: PathBuf, x: f32, y: f32 },
}

#[derive(Debug, Clone)]
pub(crate) struct TreeEdit {
    pub(crate) kind: TreeEditKind,
    pub(crate) is_dir: bool,
    pub(crate) input: gpui::Entity<TextInput>,
    /// 实时校验态（键入即查，Zed populate_validation_error 移植）
    pub(crate) validation: TreeValidation,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeEditKind {
    /// 在该目录下新建（Zed leaf_entry_id: None——is_new_entry 判据）
    New { dir: PathBuf },
    /// 同目录改名该条目
    Rename { path: PathBuf },
}

/// Zed ValidationState 移植：错误（重名/非法）拦 Enter 提交，警告放行。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum TreeValidation {
    #[default]
    None,
    Error(String),
    Warning(String),
}
