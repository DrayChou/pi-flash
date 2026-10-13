//! 文件树渲染（FileExplorer.tsx TreeNodeView parity + Zed project_panel
//! 渲染结构）：消费 `services::file_tree` 的展平缓存 [`TreeRow`]，vlist
//! 虚拟化——只构建视口内的行，render 不再碰磁盘。
//!
//! 行结构对齐 Zed project_panel（list_item.rs + indent_guides.rs 的几何）：
//! 缩进引导线是行内绝对定位竖线（Zed 装饰层同位还原：x = 级×缩进 + 偏移，
//! 恰落在父级行图标列中央），内容列 = ml(深度×缩进) + px(6) → 目录/文件
//! 类型图标（Zed "Zed (Default)" 主题）→ 名称（ellipsis）→ git 徽标 /
//! 变更目录圆点。目录无 chevron（Zed 默认 folder_indicator = "icon"，
//! 展开态由开/闭文件夹图标区分）。
//!
//! sticky 祖先链（Zed project_panel `sticky_scroll`，ui::sticky_items +
//! render_sticky_entries 同构）挂在 vlist 的 `UniformListDecoration` 上：
//! 每次 paint 按当前可见范围/滚动位重算，滚进深层目录时把锚点行的祖先链
//! 钉在列表顶部，点击钉住行滚回对应目录。

use std::ops::Range;
use std::path::Path;

use gpui::{
    Bounds, MouseButton, Pixels, Point, SharedString, UniformListDecoration, div, hsla,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb,
};

use crate::{Chat, i18n::tr, same_path};
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::appearance::theme;
use crate::ui::{VListHeight, WithTip, icon_path, vlist};
use crate::*;

/// 行高（pi-web FileExplorer 24px 行）。
const ROW_H: f32 = 24.;
/// 每级缩进（Zed project_panel `indent_size` 默认 20px）。
const INDENT: f32 = 20.;
/// 内容列左右内边距（Zed ListItem `px(DynamicSpacing::Base06)` = 6px）。
const CONTENT_PAD: f32 = 6.;
/// 引导线水平偏移：落在父级行图标列中央（图标 14px @ [6,20)，中心 13；
/// Zed 常量 15 是按其 16px 图标列取的）。
const GUIDE_X: f32 = 13.;
/// 文件树 vlist 的元素 id——sticky 点击「滚到目录」按同一 id 取句柄。
const FILE_TREE_LIST_ID: &str = "file-tree";

impl Chat {
    /// 行内改名前置（Zed project_panel 语义）：沿途展开祖先目录 + 把改名
    /// 行滚进视口。树右键菜单入口行本就可见（幂等）；编辑区右键菜单入口
    /// 可能该行还没展开/滚出视口，不做这些就没有输入框可编辑。
    pub(crate) fn reveal_tree_row(&mut self, path: &Path) {
        let mut anc = path.parent().map(|p| p.to_path_buf());
        let mut opened = false;
        while let Some(p) = anc {
            if same_path(&p, &self.cwd) {
                break;
            }
            opened |= self.expanded_dirs.insert(p.clone());
            anc = p.parent().map(|q| q.to_path_buf());
        }
        if opened {
            self.rebuild_tree();
        }
        // 「只看变更」列表可能不含目标（无 git 变更的文件）：回退树模式
        if self.changes_only && !self.tree_rows.iter().any(|r| same_path(&r.path, path)) {
            self.changes_only = false;
            self.rebuild_tree();
            self.persist_ui();
        }
        let Some(row) = self.tree_rows.iter().find(|r| same_path(&r.path, path)) else {
            return;
        };
        // 滚动落点给 sticky 祖先链留位：行顶对齐到「链高 = 深度×行高」
        // 之下（对齐 Zed scroll_to_item_strict_with_offset 的 sticky_index
        // 语义），祖先全可见后钉住链自然不触发；变更列表 depth 恒 0 恒顶
        let ix = self
            .tree_rows
            .iter()
            .position(|r| same_path(&r.path, path))
            .unwrap_or(0);
        let y = ix.saturating_sub(row.depth) as f32
            * if self.changes_only { CHANGES_ROW_H } else { ROW_H };
        let handle = crate::ui::vlist::scroll_handle(FILE_TREE_LIST_ID);
        handle.0.borrow().base_handle.set_offset(point(px(0.), px(y)));
    }
}

pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let rows = chat.tree_rows.clone();
    let changes_only = chat.changes_only;
    let weak = weak.clone();
    let t = theme::theme();
    // 021 操作栏：项目目录行固定在树顶（高度 40），名字居左、操作钮
    // 右对齐（切换显示变更 / 搜索文件 / 资源管理器 / 终端），不随列表滚动
    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.nav))
        // Zed EditState 收场主路：行内编辑激活时，面板内任何左键按下
        // （点别的行/空白/操作栏）= 提交。Zed 里这次点击先靠 uniform_list
        // `track_focus` 把焦点抢到面板（编辑行随之 blur → confirm_edit），
        // pi-flash 的面板不可聚焦（焦点保持见 `Chat::render` 级联末级，
        // 行内编辑存续期即焦点所有者），所以点击语义在面板本层直接兑现。
        // 编辑输入框内部点击在行内 stop_propagation 不受影响；点面板外的
        // 可聚焦目标（对话框/终端/文件编辑器/其它输入框）走真夺焦 → blur
        // 收场。
        .on_mouse_down(gpui::MouseButton::Left, {
            let weak_commit = weak.clone();
            move |_, _, cx| {
                let _ = weak_commit.update(cx, |c, cx| {
                    if c.dlg.tree_edit.is_some() {
                        c.tree_edit_confirm_from_click(cx);
                    }
                });
            }
        })
        .child(action_bar(chat, &weak, t));
    if changes_only {
        // 「只看变更」= 扁平列表（021 二改定稿：不要树状）。头行 =
        // N 个文件 + numstat（+绿 -红，色同 git 徽标盘）；行 = 文件名
        // 前置左对齐 + 目录段后置（目录段可截断、文件名恒可见）。
        let (add, del) = chat.git_add_del;
        let n = rows.len();
        if n > 0 {
            let mut head = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tf(
                    "file_tree.files",
                    &[("n", n.to_string())],
                )));
            if add > 0 {
                head = head.child(
                    div()
                        .text_color(rgb(crate::services::git::GitStatus::Added.color()))
                        .child(SharedString::from(format!("+{add}"))),
                );
            }
            if del > 0 {
                head = head.child(
                    div()
                        .text_color(rgb(crate::services::git::GitStatus::Deleted.color()))
                        .child(SharedString::from(format!("-{del}"))),
                );
            }
            col = col.child(head);
        }
        let weak_c = weak.clone();
        let weak_bg = weak.clone();
        let cwd_bg = chat.cwd.clone();
        let editing = chat.dlg.tree_edit.clone();
        col = col.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .py(px(6.))
                // 空白处右键 = 根目录菜单（021 四改：根行已移除，根下的
                // 新建/改名等入口由这里兜底；行内右键自己 stop_propagation
                // 不会漏到这层）
                .on_mouse_down(gpui::MouseButton::Right,
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = weak_bg.update(cx, |c, cx| {
                            c.dlg.tree_menu = Some(crate::function_panel::TreeMenu::Dir {
                                path: cwd_bg.clone(),
                                x: f32::from(ev.position.x),
                                y: f32::from(ev.position.y),
                            });
                            cx.notify();
                        });
                    },
                )
                .child(vlist(
                    FILE_TREE_LIST_ID,
                    rows.len(),
                    CHANGES_ROW_H,
                    VListHeight::Fill,
                    false,
                    true,
                    tr("file_tree.no_changes"),
                    None,
                    move |ix, _window, _cx| match rows.get(ix) {
                        Some(row) => changes_row(row.clone(), editing.clone(), weak_c.clone()),
                        None => div().into_any_element(),
                    },
                )),
        );
    } else {
        let weak_bg = weak.clone();
        let cwd_bg = chat.cwd.clone();
        let edit = chat.dlg.tree_edit.clone();
        // 新建行的落点（Zed new_entry：虚拟行插在目录行下方、子项之前；
        // 目标 = 项目根时插列表顶）。改名行不注入（原地替换该行）。
        let edit_slot: Option<(usize, usize)> = edit.as_ref().and_then(|e| match &e.kind {
            crate::function_panel::TreeEditKind::New { dir } => {
                if same_path(dir, &chat.cwd) {
                    Some((0, 0))
                } else {
                    Some(
                        rows.iter()
                            .position(|r| r.is_dir && same_path(&r.path, dir))
                            .map_or((rows.len(), 1), |ix| (ix + 1, rows[ix].depth + 1)),
                    )
                }
            }
            crate::function_panel::TreeEditKind::Rename { .. } => None,
        });
        let edit_ix = edit_slot.map(|(ix, _)| ix);
        let total = rows.len() + edit_slot.map_or(0, |_| 1);
        // 键入中的文件名（Zed：新建文件行的类型图标跟名字实时换）
        let live_name = edit
            .as_ref()
            .map(|e| e.input.read(cx).value().to_string())
            .unwrap_or_default();
        let sticky = Box::new(StickyAncestors { weak: weak.clone(), edit_ix }) as Box<dyn UniformListDecoration>;
        col = col.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .py(px(10.))
                // 空白处右键 = 根目录菜单（同变更分支，根行入口兜底）
                .on_mouse_down(gpui::MouseButton::Right,
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = weak_bg.update(cx, |c, cx| {
                            c.dlg.tree_menu = Some(crate::function_panel::TreeMenu::Dir {
                                path: cwd_bg.clone(),
                                x: f32::from(ev.position.x),
                                y: f32::from(ev.position.y),
                            });
                            cx.notify();
                        });
                    },
                )
                .child(vlist(
                    FILE_TREE_LIST_ID,
                    total,
                    ROW_H,
                    VListHeight::Fill,
                    false,
                    true,
                    "暂无文件",
                    Some(sticky),
                    move |ix, _window, _cx| {
                        // 行内新建：该输出位渲染编辑行，其余行做索引平移
                        if edit_slot.is_some_and(|(eix, _)| ix == eix) {
                            let Some((_, depth)) = edit_slot else { return div().into_any_element() };
                            return tree_edit_row(
                                edit.clone().expect("slot checked"),
                                depth,
                                &live_name,
                            );
                        }
                        let row_ix = match edit_ix {
                            Some(eix) if ix > eix => ix - 1,
                            _ => ix,
                        };
                        let Some(row) = rows.get(row_ix) else { return div().into_any_element() };
                        // 邻行深度决定各段引导线的首尾裁剪（整段只在首行留上
                        // 4px、尾行留下 4px，段中连续成一条线）；编辑行占位时
                        // 邻行深度取编辑行的深度
                        let depth_at = |out_ix: usize| -> Option<usize> {
                            if Some(out_ix) == edit_ix {
                                edit_slot.map(|(_, d)| d)
                            } else {
                                let rix = match edit_ix {
                                    Some(eix) if out_ix > eix => out_ix - 1,
                                    _ => out_ix,
                                };
                                rows.get(rix).map(|r| r.depth)
                            }
                        };
                        let prev_depth = if ix == 0 { None } else { depth_at(ix - 1) };
                        let next_depth = depth_at(ix + 1);
                        tree_row(
                            row.clone(),
                            prev_depth,
                            next_depth,
                            edit.clone(),
                            weak.clone(),
                        )
                    },
                )),
        );
    }
    col.into_any_element()
}

/// 变更列表行高（比树行稍宽舒，对齐 pi-web 截图的呼吸感）。
const CHANGES_ROW_H: f32 = 28.;

/// 「只看变更」的扁平行：文件名前置左对齐（恒可见，shrink 0）+
/// 目录段后置 dim（过长收缩截断），点击开文件 tab，右键沿用文件菜单。
/// 行处于行内改名时（Zed 同款）名字段整体换成紧凑输入框，点击开文件
/// 让位给输入框。
fn changes_row(
    row: crate::services::file_tree::TreeRow,
    editing: Option<crate::function_panel::TreeEdit>,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let rel = row.name.clone();
    let file_name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let dir_part = rel[..rel.len() - file_name.len()].to_string();
    let editing = editing.filter(|e| {
        matches!(&e.kind, crate::function_panel::TreeEditKind::Rename { path } if same_path(path, &row.path))
    });
    // 021 三改：徽标与文件图标都去掉（信息冗余——变更态整表皆然，
    // 数量/增删在头行 numstat），行 = 纯路径
    let mut el = div()
        .id(SharedString::from(format!("chg-{}", row.path.display())))
        .w_full()
        .h(px(CHANGES_ROW_H))
        .flex()
        .items_center()
        .px(px(12.))
        .gap(px(7.))
        .overflow_hidden()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .text_size(crate::appearance::ui_size(11.5));
    if let Some(e) = editing {
        // 行内改名态：输入框吃满名字区（目录段一并隐去），边框与校验
        // 消息同 Zed 编辑行
        let (border, message) = edit_chrome(&e, t);
        el = el
            .border_1()
            .border_color(rgb(border))
            .relative()
            .child(edit_input_slot(e.input.clone()))
            .children(message.map(|m| validation_note(m, border)));
    } else {
        // 021 五改：文件名前置（各行文件名同一起点左对齐、恒可见），目录段
        // 后置 dim、仅在过长时收缩截断（中段省略），尾部弹性空隙吃掉剩余宽
        el = el.child(
            div()
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text))
                .child(SharedString::from(file_name)),
        );
        el = el.child(
            div()
                .flex_shrink()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(dir_part)),
        );
        el = el.child(div().flex_1());
        let fp = row.path.clone();
        let weak_click = weak.clone();
        el = el.on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak_click.update(cx, |c, cx| {
                c.open_file_tab(fp.clone(), cx);
            });
        });
    }
    el.child(tree_right_click(&weak, &row))
        .into_any_element()
}

/// 021 操作栏（高 40，固定）：左侧 4 颗图标钮——切换显示变更（开 =
/// accent 高亮）/ 搜索文件 / 在资源管理器中打开 / 在终端中打开，其后
/// 是项目目录名（flex_1 ellipsis）。按钮命中区 26px（§5.2 槽位），
/// 视觉与 psp 标题行同族。
fn action_bar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let project: SharedString = chat
        .cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| chat.cwd.to_string_lossy().to_string())
        .into();
    let changes_only = chat.changes_only;
    let w_toggle = weak.clone();
    let w_search = weak.clone();
    let w_term = weak.clone();
    let w_menu = weak.clone();
    let cwd_menu = chat.cwd.clone();
    let cwd_explorer = chat.cwd.clone();
    let cwd_term = chat.cwd.clone();
    // 021 二改定稿：操作栏即根行——项目名居左（粗体）、操作钮右对齐
    // （间距 10px，各带 tooltip）；名字区右键 = 根目录菜单（树体不再有
    // 根行，根下的新建/改名等入口保留下）
    div()
        .id("ft-action-bar")
        .w_full()
        .h(px(40.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pl(px(10.))
        .pr(px(10.))
        .gap(px(2.))
        .border_b_1()
        .border_color(rgb(t.border))
        .child(
            div()
                .id("ft-project")
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_soft))
                .child(project)
                .on_mouse_down(gpui::MouseButton::Right,
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = w_menu.update(cx, |c, cx| {
                            c.dlg.tree_menu = Some(crate::function_panel::TreeMenu::Dir {
                                path: cwd_menu.clone(),
                                x: f32::from(ev.position.x),
                                y: f32::from(ev.position.y),
                            });
                            cx.notify();
                        });
                    },
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(bar_btn(
                    "ft-toggle-changes",
                    "git-commit-horizontal",
                    tr("file_tree.show_changes_only"),
                    t,
                    changes_only,
                    move |_, _, cx| {
                        let _ = w_toggle.update(cx, |c, cx| c.toggle_changes_only(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-search",
                    "search",
                    tr("file_tree.search"),
                    t,
                    false,
                    move |_, _, cx| {
                        let _ = w_search.update(cx, |c, cx| c.open_file_search(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    t,
                    false,
                    move |_, _, _cx| {
                        crate::services::shell_open::open_in_explorer(&cwd_explorer);
                    },
                ))
                .child(bar_btn(
                    "ft-term",
                    "icon-terminal-solid",
                    tr("file_tree.open_in_terminal"),
                    t,
                    false,
                    move |_, window, cx| {
                        let _ = w_term.update(cx, |c, cx| {
                            c.open_terminal_in(cwd_term.clone(), window, cx)
                        });
                    },
                )),
        )
        .into_any_element()
}

/// 操作栏图标钮：26px 命中槽 + 16px 图标（icon_hover 悬停变亮；激活态
/// 用 accent 实色不参与 hover 变亮——开关点亮语义）。`tip` = hover
/// tooltip（ui::tooltip 统一入口 §5.10，显示期过 tr 随语言切换）。
fn bar_btn(
    id: &'static str,
    icon_name: &'static str,
    tip: &'static str,
    t: &'static crate::appearance::theme::Theme,
    active: bool,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .with_tip(tip)
        .on_mouse_down(gpui::MouseButton::Left, on_click)
        .child(if active {
            crate::ui::icon(icon_name, 16., t.accent).into_any_element()
        } else {
            crate::ui::icon_hover(icon_name, 16., t.text_dim).into_any_element()
        })
        .into_any_element()
}

/// One flattened row: indent guides + chevron + type icon + name + git badge.
fn tree_row(
    row: TreeRow,
    prev_depth: Option<usize>,
    next_depth: Option<usize>,
    editing: Option<crate::function_panel::TreeEdit>,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let weak_menu = weak.clone();
    // 本行处于行内改名（Zed EditState 命中该行）：名字标签换输入框，点
    // 击展开/打开让位给输入框
    let editing = editing.filter(|e| {
        matches!(&e.kind, crate::function_panel::TreeEditKind::Rename { path } if same_path(path, &row.path))
    });
    let mut el = div()
        .id(SharedString::from(format!("tree-{}", row.path.display())))
        .relative()
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        // 目录树 = 面板设置值 -1（字体大小设置.md §1；text_xs 固定 12px
        // 不随设置走）
        .text_size(crate::appearance::ui_size(11.))
        // gitignored 行置灰（Zed parity：显示但 dimmed，仍可点开浏览）
        .text_color(rgb(if row.ignored { t.text_faint } else { t.text }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));

    // 缩进引导线：Zed indent_guides 装饰层的行内画法（见 guide_lines）。
    for g in guide_lines(row.depth, prev_depth, next_depth, t.border) {
        el = el.child(g);
    }

    // 内容列：Zed ListItem 内层 ml(深度×缩进) + px(Base06)，引导线与悬停
    // 背景都在行全宽上（Zed 行底色也是全宽的）。
    let mut inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .ml(px(row.depth as f32 * INDENT))
        .px(px(CONTENT_PAD));

    if let Some(e) = &editing {
        // 编辑行 chrome（Zed show_focused_border + validation 同位）：边框
        // 随校验态变色，校验消息浮在该行下缘
        let (border, message) = edit_chrome(e, t);
        el = el.border_1().border_color(rgb(border));
        let icon = if row.is_dir {
            file_icons::get_generic_folder_icon(false)
        } else {
            file_icons::get_icon(&row.path)
        };
        inner = inner
            .child(div().flex_shrink_0().child(icon_path(icon, 14., t.text_dim)))
            .child(edit_input_slot(e.input.clone()));
        el = el.child(inner).children(message.map(|m| validation_note(m, border)));
        return el.into_any_element();
    }

    if row.is_dir {
        // Zed project_panel 默认 folder_indicator = "icon"：纯文件夹图标，
        // 无 chevron，展开态由开/闭图标区分（面板后续若加该设置再接
        // get_folder_indicators 的 Both/Chevron 分支）
        let folder = file_icons::get_generic_folder_icon(row.expanded);
        inner = inner
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon_path(folder, 14., t.text_dim)),
            )
            .child(name_label(&row.name));
        el = el.child(inner);
        let dir_path = row.path.clone();
        el = el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                if !c.expanded_dirs.remove(&dir_path) {
                    c.expanded_dirs.insert(dir_path.clone());
                }
                c.rebuild_tree();
                cx.notify();
            });
        });
        el = el.child(tree_right_click(&weak_menu, &row));
        // 目录圆点：子树有变更（祖先链上浮，services 层已算好）
        if row.changed_dot && crate::services::workspace::git_markers() {
            el = el.child(changed_dot());
        }
    } else {
        let icon = file_icons::get_icon(&row.path);
        inner = inner
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon_path(icon, 14., t.text_dim)),
            )
            .child(name_label(&row.name));
        el = el.child(inner);
        let fp = row.path.clone();
        // 023：文件点击一律进 fileView（编辑器区）。旧行为「带 git 变更
        // 徽标的文件点击弹 GitDiff」废弃——diff 入口收敛在 git 面板。
        el = el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.open_file_tab(fp.clone(), cx);
            });
        });
        el = el.child(tree_right_click(&weak_menu, &row));
        // git badge on files（pi-web 徽标语义：M/A/D/R/U/C；023 默认关）
        if let Some(st) = row.git.filter(|_| crate::services::workspace::git_markers()) {
            el = el.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::appearance::ui_size(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(st.color()))
                    .child(st.badge()),
            );
        }
    }
    el.into_any_element()
}

fn name_label(name: &str) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        // Zed 内容列 gap = DynamicSpacing::Base06（图标 → 名称 6px）
        .ml(px(6.))
        .child(SharedString::from(name.to_string()))
        .into_any_element()
}

/// 编辑态 chrome：边框色（校验错误红 / 警告黄 / 常态 accent）+ 校验消息。
fn edit_chrome(e: &crate::function_panel::TreeEdit, t: &'static crate::appearance::theme::Theme) -> (u32, Option<SharedString>) {
    match &e.validation {
        crate::function_panel::TreeValidation::Error(m) => (t.danger, Some(SharedString::from(m.clone()))),
        crate::function_panel::TreeValidation::Warning(m) => (theme::WARN, Some(SharedString::from(m.clone()))),
        crate::function_panel::TreeValidation::None => (t.accent, None),
    }
}

/// 校验消息浮条（Zed render 的 validation_color_and_message 同位：行下缘
/// 绝对定位、描边小卡）。
fn validation_note(message: SharedString, color: u32) -> gpui::AnyElement {
    div()
        .absolute()
        .top_full()
        .left(px(-1.))
        .right(px(-1.))
        .py(px(4.))
        .px(px(8.))
        .border_1()
        .border_color(rgb(color))
        .bg(rgb(theme::theme().bg))
        .text_size(crate::appearance::ui_size(10.5))
        .text_color(rgb(color))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(message)
        .into_any_element()
}

/// 行内编辑输入槽：输入框吃满名字区，内部左键 stop_propagation——点击
/// 输入框本身不得触发面板级「点别处提交」。
fn edit_input_slot(input: gpui::Entity<crate::ui::TextInput>) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .ml(px(6.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(input)
        .into_any_element()
}

/// 行内新建行（Zed NEW_ENTRY_ID 虚拟行）：目录行下方、子项之前，缩进 =
/// 目录深度 + 1；文件图标随键入名实时变化；行带编辑边框 + 校验消息。
/// 无点击动作（行体点击 = 点别处，冒泡到面板收场，同 Zed blur）。
fn tree_edit_row(edit: crate::function_panel::TreeEdit, depth: usize, live_name: &str) -> gpui::AnyElement {
    let t = theme::theme();
    let (border, message) = edit_chrome(&edit, t);
    let icon = if edit.is_dir {
        file_icons::get_generic_folder_icon(false)
    } else if live_name.chars().count() > 2 {
        file_icons::get_icon(Path::new(live_name))
    } else {
        file_icons::get_icon(Path::new(""))
    };
    let mut el = div()
        .relative()
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text))
        .border_1()
        .border_color(rgb(border));
    for g in guide_lines(depth, Some(depth), Some(depth), t.border) {
        el = el.child(g);
    }
    let inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .ml(px(depth as f32 * INDENT))
        .px(px(CONTENT_PAD))
        .child(div().flex_shrink_0().child(icon_path(icon, 14., t.text_dim)))
        .child(edit_input_slot(edit.input.clone()));
    el.child(inner)
        .children(message.map(|m| validation_note(m, border)))
        .into_any_element()
}

/// 缩进引导线（Zed indent_guides 装饰层的行内画法）：第 L 级竖线在
/// L×INDENT+GUIDE_X（父级行图标列中央）；整段只在首行缩顶部 4px、尾行缩
/// 底部 4px（Zed PADDING_Y），段中行互相续上成一条线。`prev`/`next` 是
/// 渲染语境里上/下邻行的深度（普通行 = 展平表邻居，钉住行 = 链上邻居）。
fn guide_lines(
    depth: usize,
    prev: Option<usize>,
    next: Option<usize>,
    color: u32,
) -> impl Iterator<Item = gpui::AnyElement> {
    (0..depth).map(move |level| {
        let top = if prev.is_none_or(|d| d <= level) { 4. } else { 0. };
        let bottom = if next.is_none_or(|d| d <= level) { 4. } else { 0. };
        div()
            .absolute()
            .left(px(level as f32 * INDENT + GUIDE_X))
            .top(px(top))
            .h(px(ROW_H - top - bottom))
            .w(px(1.))
            .bg(rgb(color))
            .into_any_element()
    })
}

/// 目录圆点：子树有变更（amber，pi-web 同色）。
fn changed_dot() -> gpui::AnyElement {
    div()
        .size(px(6.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(0xd6a84b))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// sticky 祖先链 —— Zed project_panel sticky_scroll（crates/ui/src/components/
// sticky_items.rs + project_panel.rs render_sticky_entries 同构）
// ---------------------------------------------------------------------------

/// 钉住祖先链的装饰层：可见行里找锚点（祖先已滚出视口的行），把它的祖先
/// 目录链钉在列表顶部。挂在 vlist 的 `UniformListDecoration` 上，每次
/// paint 按当前可见范围与滚动位重算——滚动中实时跟手（行闭包只在视口内
/// 重建，装饰层同样只在 paint 期取数，render 不多跑）。`edit_ix` = 行内
/// 新建的虚拟行位置（不在 tree_rows 里，可见范围做一次索引平移）。
struct StickyAncestors {
    weak: gpui::WeakEntity<Chat>,
    edit_ix: Option<usize>,
}

impl UniformListDecoration for StickyAncestors {
    fn compute(
        &self,
        visible_range: Range<usize>,
        _bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        _item_height: Pixels,
        _item_count: usize,
        _window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> gpui::AnyElement {
        // paint 期 Chat 的 render 借用已结束，update 合法（Zed sticky_items
        // 同款）；weak 失联（理论上不发生）就整层不画
        if self.weak.upgrade().is_none() {
            return div().into_any_element();
        }
        self.weak
            .update(cx, |chat, _| self.render_stack(chat, visible_range, scroll_offset))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl StickyAncestors {
    fn render_stack(
        &self,
        chat: &Chat,
        visible_range: Range<usize>,
        scroll_offset: Point<Pixels>,
    ) -> gpui::AnyElement {
        let rows = &chat.tree_rows;
        // Zed show_sticky_entries 门控：未滚动时不钉（否则「深度 < 视口
        // 序号」规则会在兄弟较多的树上于 scroll 0 误触发，把根行钉成重影）
        if scroll_offset.y >= px(0.) {
            return div().into_any_element();
        }
        let Some((anchor_ix, drifting)) = find_sticky_anchor(rows, visible_range, self.edit_ix)
        else {
            return div().into_any_element();
        };
        let chain = ancestor_chain(rows, anchor_ix);
        if chain.is_empty() {
            return div().into_any_element();
        }

        // 装饰层坐标系随内容滚动（prepaint 原点已含 scroll_offset），钉在
        // 屏幕上 = 每行 top 补偿 -scroll_offset.y（全按 f32 算，px() 只在
        // 落样式处取）
        let scroll_y = f32::from(scroll_offset.y);
        let compensation = -scroll_y;
        let stack_h = chain.len() as f32 * ROW_H;
        // Zed 漂移态：锚点行滚进钉住区时，链末行（锚点父目录）贴着锚点行
        // 底缘一起上滑，直到被钉住区吞没
        let drift = if drifting {
            let anchor_bottom = (anchor_ix as f32 + 1.) * ROW_H + scroll_y;
            (anchor_bottom - stack_h).min(0.)
        } else {
            0.
        };

        let last = chain.len() - 1;
        let mut stack = div().relative().w_full();
        for (k, &row_ix) in chain.iter().enumerate() {
            let y = px(k as f32 * ROW_H + if k == last { drift } else { 0. } + compensation);
            let prev_depth = if k == 0 { None } else { Some(rows[chain[k - 1]].depth) };
            // 末行引导线向锚点行续（不封底），与列表里的行无缝相接
            let next_depth =
                if k == last { Some(rows[anchor_ix].depth) } else { Some(rows[chain[k + 1]].depth) };
            stack = stack.child(sticky_dir_row(
                &rows[row_ix],
                prev_depth,
                next_depth,
                y,
                row_ix,
                k,
                self.weak.clone(),
            ));
        }
        // Zed 同款：钉住区底缘 1.5px 渐隐阴影（黑 0.12 → 0）
        stack = stack.child(
            div()
                .absolute()
                .left_0()
                .w_full()
                .top(px(stack_h + drift + compensation - 1.5))
                .h(px(1.5))
                .bg(linear_gradient(
                    0.,
                    linear_color_stop(hsla(0., 0., 0., 0.12), 1.),
                    linear_color_stop(hsla(0., 0., 0., 0.), 0.),
                )),
        );
        stack.into_any_element()
    }
}

/// 钉住的目录行：普通目录行的视觉（引导线按链上邻居接续）+ 点击滚回
/// 对应目录（Zed sticky 点击语义），并拦截冒泡——底下压着的列表行不得
/// 收到这次点击（否则会触发折叠）。
#[allow(clippy::too_many_arguments)]
fn sticky_dir_row(
    row: &TreeRow,
    prev_depth: Option<usize>,
    next_depth: Option<usize>,
    top: Pixels,
    row_ix: usize,
    slot: usize,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let mut el = div()
        .absolute()
        .left_0()
        .w_full()
        .top(top)
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        .bg(rgb(t.nav))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));
    for g in guide_lines(row.depth, prev_depth, next_depth, t.border) {
        el = el.child(g);
    }
    let folder = file_icons::get_generic_folder_icon(row.expanded);
    let inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .ml(px(row.depth as f32 * INDENT))
        .px(px(CONTENT_PAD))
        .child(div().flex_shrink_0().child(icon_path(folder, 14., t.text_dim)))
        .child(name_label(&row.name));
    el = el.child(inner);
    if row.changed_dot && crate::services::workspace::git_markers() {
        el = el.child(changed_dot());
    }
    // 点击滚到该目录：让目录行落在钉住区里它自己的槽位（Zed
    // scroll_to_item_strict_with_offset(index, Top, sticky_index)），再
    // +1px 防「刚好落在钉住位上重新参与锚点判定」的抖动
    let target_y = px((slot as f32 - row_ix as f32) * ROW_H + 1.);
    let weak_menu = weak.clone();
    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        let handle = crate::ui::vlist::scroll_handle(FILE_TREE_LIST_ID);
        handle.0.borrow().base_handle.set_offset(point(px(0.), target_y));
        let _ = weak.update(cx, |_, cx| cx.notify());
        cx.stop_propagation();
    })
    .child(tree_right_click(&weak_menu, row))
    .into_any_element()
}

/// 行右键：挂文件树右键菜单（目录/文件两套，锚点 = 事件坐标）。
/// 挂为行尾子元素占位（无视觉），事件在行容器上接。
fn tree_right_click(weak: &gpui::WeakEntity<Chat>, row: &TreeRow) -> gpui::AnyElement {
    let weak = weak.clone();
    let path = row.path.clone();
    let is_dir = row.is_dir;
    div()
        .absolute()
        .size_full()
        .top_0()
        .left_0()
        .on_mouse_down(MouseButton::Right, move |ev: &gpui::MouseDownEvent, _, cx| {
            cx.stop_propagation();
            let path = path.clone();
            let _ = weak.update(cx, |c, cx| {
                c.dlg.tree_menu = Some(if is_dir {
                    crate::function_panel::TreeMenu::Dir {
                        path,
                        x: f32::from(ev.position.x),
                        y: f32::from(ev.position.y),
                    }
                } else {
                    crate::function_panel::TreeMenu::File {
                        path,
                        x: f32::from(ev.position.x),
                        y: f32::from(ev.position.y),
                    }
                });
                cx.notify();
            });
        })
        .into_any_element()
}

/// Zed `ui::sticky_items::find_sticky_anchor` 同构：在可见行里找锚点行，
/// 返回（绝对行号，是否漂移）。
///
/// - ① `depth < 视口序号`：该行的祖先已滚出视口（序号 0..ix 里放不下
///   depth 个祖先）；
/// - ② `depth == 序号(+1)` 且下一行退出本子树（`next.depth + 1 == depth`）：
///   该行是子树内最后可见的行，其父链即将全部滚出；+1 的情况（深度刚好
///   比序号大 1）父链只差一行滚出，标记 drifting（链末行跟着锚点滑）。
fn find_sticky_anchor(
    rows: &[TreeRow],
    visible: Range<usize>,
    edit_ix: Option<usize>,
) -> Option<(usize, bool)> {
    // 行内新建的虚拟行夹在列表中间：输出序号 → tree_rows 序号平移一位
    // （虚拟行自身不参与锚点判定）
    let to_row = |abs: usize| -> Option<usize> {
        match edit_ix {
            Some(eix) if abs == eix => None,
            Some(eix) if abs > eix => Some(abs - 1),
            _ => Some(abs),
        }
    };
    for abs in visible.start..visible.end {
        let ix = abs - visible.start;
        let Some(row_ix) = to_row(abs) else { continue };
        let Some(row) = rows.get(row_ix) else { break };
        let depth = row.depth;
        if depth < ix {
            return Some((row_ix, false));
        }
        // 下一「输出行」的深度：虚拟行夹在中间时取虚拟行前一行（近似——
        // 虚拟行深度与被编辑目录子项同级，规则只做引导线裁剪，误差可忽略）
        let next_row_ix = row_ix + 1;
        if Some(abs + 1) == edit_ix {
            continue;
        }
        if let Some(next) = rows.get(next_row_ix)
            && next.depth + 1 == depth
            && (depth == ix || depth == ix + 1)
        {
            return Some((row_ix, depth == ix + 1));
        }
    }
    None
}

/// 锚点行的祖先行号（根在前）。DFS 展平里「前面最近的 depth-k 行」必是
/// depth-k 祖先（锚点行与各祖先之间只可能夹更深的行），折叠链行
/// （a/b/c 一行）不破坏该性质。
fn ancestor_chain(rows: &[TreeRow], anchor_ix: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut cur = anchor_ix;
    while rows[cur].depth > 0 {
        let target = rows[cur].depth - 1;
        let mut found = None;
        let mut j = cur;
        while j > 0 {
            j -= 1;
            if rows[j].depth == target {
                found = Some(j);
                break;
            }
        }
        let Some(j) = found else {
            // 理论不可达：根行（depth 0）恒在 index 0
            return Vec::new();
        };
        chain.push(j);
        cur = j;
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod sticky_tests {
    use std::path::PathBuf;

    use super::{ancestor_chain, find_sticky_anchor};
    use crate::services::file_tree::TreeRow;

    fn row(name: &str, depth: usize) -> TreeRow {
        TreeRow {
            path: PathBuf::from(name),
            name: name.to_string(),
            depth,
            is_dir: true,
            expanded: true,
            git: None,
            changed_dot: false,
            ignored: false,
        }
    }

    /// root(0) / A(1) / a1 a2(2) / B(1) / b1(2) / bk(3) / C(1) / c(2)
    fn fixture() -> Vec<TreeRow> {
        vec![
            row("root", 0),
            row("A", 1),
            row("a1", 2),
            row("a2", 2),
            row("B", 1),
            row("b1", 2),
            row("bk", 3),
            row("C", 1),
            row("c", 2),
        ]
    }

    #[test]
    fn anchor_fires_on_first_row_shallower_than_viewport_index() {
        // 未滚动（门控由渲染层负责，此处测纯函数）：a2 深度 2 < 视口序号 3
        // → 首个命中行为锚点。滚动 1px 起即钉 [root, A]——钉住行恰好盖在
        // 这两行的真实位置上，视觉无缝（Zed 同款行为）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 0..9, None), Some((3, false)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
    }

    #[test]
    fn deep_subtree_pins_full_chain() {
        // 滚到 a1（视口 2..9）：a2 是子树内最后可见行且下一行退出子树、
        // 深度恰比视口序号大 1 → 锚点 a2、漂移态；链 = [root, A]（A 已滚出）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 2..9, None), Some((3, true)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
        assert_eq!(ancestor_chain(&rows, 6), vec![0, 4, 5]);
    }

    #[test]
    fn anchor_at_subtree_tail_with_visible_parent() {
        // 视口 [A, a1, a2, B, ...]（滚 1 行）：a2 下一行退出子树、深度恰
        // 等于视口序号 → 锚点 a2、不漂移，链 = [root, A]（A 在视口内，钉住
        // 行覆盖其上，Zed 同款）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 1..9, None), Some((3, false)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
    }

    #[test]
    fn drifting_when_anchor_one_row_past_its_slot() {
        // 视口 [a1, a2, B, ...]（滚 2 行）：a2 深度 2 = 视口序号 1 + 1 且
        // 下一行退出子树 → 漂移态
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 2..9, None), Some((3, true)));
    }

    #[test]
    fn anchor_walks_into_next_subtree_after_tail() {
        // 继续滚 1 行（视口 [a2, B, b1, bk, C, ...]）：a2 不再满足任何规则，
        // bk 深度 3 > 序号 1 也不满足，规则①在 C（深度 1 < 序号 2）命中。
        // C 与 B 是兄弟，链只有根
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 3..9, None), Some((7, false)));
        assert_eq!(ancestor_chain(&rows, 7), vec![0]);
        // bk 在 B 子树深处：它的链才是 [root, B, b1]（锚点判定另有规则，
        // 这里只测链回溯）
        assert_eq!(ancestor_chain(&rows, 6), vec![0, 4, 5]);
    }

    #[test]
    fn chain_walk_skips_folded_and_sibling_rows() {
        // B 的父链要跳过 A 的整棵子树；折叠链行（depth 不变的多级目录）
        // 同样只按深度回溯。锚点：B 深度 1 < 视口序号 4（规则①）
        let rows = vec![
            row("root", 0),
            row("A", 1),
            row("a/deep/chain", 2),
            row("a/deeper", 3),
            row("B", 1),
            row("b/x/y", 2),
        ];
        assert_eq!(find_sticky_anchor(&rows, 0..6, None), Some((4, false)));
        assert_eq!(ancestor_chain(&rows, 5), vec![0, 4]);
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1, 2]);
    }
}

// ---------------------------------------------------------------------------
// 文件树与 fs 监听方法（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    /// 文件树展平缓存重建（services::file_tree::flatten：每展开目录读一次
    /// 盘 + gitignore 过滤 + Zed 语义排序 + git 徽标回填）。展开/折叠、
    /// git 刷新、fs 事件、切项目后都走这里。
    pub(crate) fn rebuild_tree(&mut self) {
        self.tree_rows = std::sync::Arc::new(if self.changes_only {
            // 「只看变更」= 扁平表直接吃 git status 结果（不扫盘）
            services::file_tree::flatten_changes(&self.cwd, &self.git_files)
        } else {
            services::file_tree::flatten(&self.cwd, &self.expanded_dirs, &self.git_files)
        });
    }

    /// 挂上 fs watcher（递归 watch cwd；旧句柄 drop 即解除）。切项目时
    /// 重挂。事件由 startup::spawn_fs_watch_pump 合批后驱动 refresh_git。
    pub(crate) fn attach_fs_watch(&mut self) {
        self.fs_watch = services::watcher::watch(&self.cwd, self.fs_watch_tx.clone()).ok();
    }

    /// 023 外部改动检测的监听面维护（对齐 Zed 的 worktree 监听面）：
    /// - cwd 内文件：cwd 的**递归** watch 已覆盖；
    /// - cwd 外文件（Obsidian 库 / pick_open_files / 远程 FILE_OPEN）：此前
    ///   完全没监听——磁盘怎么改都没有信号，编辑器不跟随。现按**所在目录**
    ///   挂非递归 watch（Zed single-file worktree 同款），事件与 cwd 事件
    ///   走同一条通道合批，`check_external_file_changes` 按 disk_sig 复查。
    ///
    /// 打开/关闭文件 tab、切项目后调用；不再被任何打开文件需要的目录卸掉
    /// （句柄 drop = 停止监听），避免长期持有无关目录的监听。
    pub(crate) fn retain_file_watches(&mut self) {
        let mut wanted: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for t in &self.panel_tabs {
            if let PanelTab::File(p) = t
                && let Some(dir) = p.parent()
                && !services::workspace::is_under(dir, &self.cwd)
            {
                wanted.insert(dir.to_path_buf());
            }
        }
        self.file_watches.retain(|dir, _| wanted.contains(dir));
        for dir in wanted {
            if self.file_watches.contains_key(&dir) {
                continue;
            }
            if let Ok(w) = services::watcher::watch_dir(&dir, self.fs_watch_tx.clone()) {
                self.file_watches.insert(dir, w);
            }
        }
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------

    pub(crate) fn load_project_files(&mut self) {
        self.project_files = walk_files(&self.cwd, 3, 400);
    }

}
