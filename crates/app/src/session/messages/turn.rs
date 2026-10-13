//! 消息行与轮渲染：render_block 块分发、render_msg 用户行、
//! render_bash_msg、render_assistant_turn 轮装配与操作栏悬停接线。

use super::*;

use std::collections::HashMap;

use gpui::{Animation, AnimationExt, MouseButton, SharedString, div, prelude::*, px, relative, rgb, rgba};
use pi_link::protocol::Block;

use crate::session::actions_bar::{self, user_action_bar};
use crate::session::fork::ForkAnchor;
use crate::Chat;
use crate::i18n::tr;
use crate::editor::markdown;
use crate::appearance::theme;
use crate::ui::icon;


pub(crate) fn render_block(
    b: &Block,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // 流式中的消息：markdown 代码块跳过 syntect 高亮（v56-3 c15）
    streaming: bool,
    // thinking 时长（pi-web ThinkingBlock 右侧 Ns；快照按消息首尾时间差）
    thinking_dur: Option<i64>,
) -> gpui::Div {
    match b {
        Block::Text { text, .. } if !text.trim().is_empty() => {
            div().w_full().child(markdown::render(text, t, streaming))
        }
        Block::Thinking { text, content_index } if !text.trim().is_empty() => {
            let key = (msg_ix, *content_index);
            // 「展示思考」开关 = 新思考块的默认展开态：开=默认展开全文，
            // 关（默认）=收成一行（灯泡+单行预览，点击可展开）；用户手动
            // 展开过的块以 collapsed 里的显式值为准
            let expanded = collapsed
                .get(&key)
                .copied()
                .unwrap_or(crate::services::workspace::show_thinking());
            let weak = weak.clone();
            // pi-web getThinkingPreview: first line, up to 240 chars, trimmed
            let preview: String = text
                .trim_start()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(240)
                .collect::<String>()
                .trim_end()
                .to_string();
            // pi-web ThinkingBlock: flat var(--bg) strip with 1px border —
            // [lightbulb] [one-line preview] collapsed, [lightbulb] [pre-wrap
            // muted body] expanded; amber bulb while expanded, no chevron, no
            // "thinking" label
            let toggle = div()
                .id(SharedString::from(format!("th-{msg_ix}-{content_index}")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .text_color(rgb(t.text_muted))
                .when(expanded, |d| d.flex_shrink_0())
                .when(!expanded, |d| d.flex_1())
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let k = key;
                    let _ = weak.update(cx, |c, cx| {
                        let rt = c.rt();
                        rt.update(cx, |r, _| {
                            let next = !r.collapsed.get(&k).copied().unwrap_or(true);
                            r.collapsed.insert(k, next);
                        });
                        cx.notify();
                    });
                })
                .child(if expanded {
                    icon("lightbulb", 14., 0xd4a017)
                } else {
                    icon("lightbulb", 14., t.text_muted)
                })
                .children((!expanded).then(|| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(if preview.is_empty() {
                            SharedString::from("...").into_any_element()
                        } else {
                            SharedString::from(preview).into_any_element()
                        })
                }));
            let mut block = div()
                .w_full()
                // 块间距由上层 gap 8 容器统一（pi-web 块容器 gap:8），
                // 块自身不挂 margin（此前 my_1=4px 与工具卡 0px 不均）
                .flex()
                .items_start()
                .gap_1p5()
                .min_w_0()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                // 思考块字号 = 会话字号 -2（字体大小设置.md §2；族保持等宽）
                .text_size(crate::appearance::sess_size(-2.))
                .line_height(relative(1.5))
                .child(toggle);
            if expanded {
                block = block.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(text.clone())),
                );
            }
            // pi-web {duration}s：右侧 text-dim
            if let Some(d) = thinking_dur {
                block = block.child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!("{d}s"))),
                );
            }
            block
        }
        Block::ToolCall {
            content_index,
            name,
            args,
            result,
            is_error,
            images,
            duration_s,
            details,
            args_partial,
            result_arrived,
            ..
        } => {
            // v56-2 c8/c9: pi-web ToolCallBlock parity —— 状态色卡片 +
            // 参数摘要 + 耗时 + 旋转箭头 + 展开体（参数 pre / 结果 pre）；
            // diff（c10）与图片（c11）见后续提交。收进「工作详情」组内。
            render_tool_card(
                msg_ix,
                *content_index,
                name,
                args,
                result,
                *is_error,
                images,
                details.as_ref(),
                *duration_s,
                *args_partial,
                *result_arrived,
                weak,
                collapsed,
                t,
            )
        }
        _ => div().w_full(),
    }
}

/// pi-web lib/slash-display.ts skillExpansionToCommand 的 Rust 移植：识别
/// pi _expandSkillCommand 输出的信封，还原紧凑命令（仅显示用，存储文本
/// 不变）。贪婪正文（正文可能含示例 </skill>，取最后一个），可选双换行
/// 后缀为用户参数。
pub(crate) fn skill_expansion_to_command(text: &str) -> Option<String> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" location=\"")?;
    let (_loc, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(">\n")?;
    let rest = rest.strip_prefix("References are relative to ")?;
    let rest = rest.split_once('\n')?.1; // 跳过 base 目录行
    let rest = rest.strip_prefix('\n')?; // 空行
    let close = rest.rfind("\n</skill>")?;
    let tail = &rest[close + 9..];
    let args = match tail.strip_prefix("\n\n") {
        Some(a) => a.to_string(),
        None if tail.is_empty() => String::new(),
        None => return None,
    };
    Some(if args.is_empty() {
        format!("/skill:{}", name)
    } else {
        format!("/skill:{} {}", name, args)
    })
}

/// 操作栏悬停显影的**唯一**接线：用户消息行与 agent 轮块都走它，不许各写
/// 一段内联 `on_hover`（两套写法必然分叉，见下）。
///
/// 状态 = [`Chat::bar_hover`]：单个 `Option<usize>`，存该行在列表里的索引
/// （用户行 = 消息索引，agent 轮 = 轮首消息索引；两者角色不同，索引永不撞车）。
///
/// **为什么必须统一且小心后代遮挡**：gpui `Window::hit_test` 命中
/// `HitboxBehavior::BlockMouse`（即 `div().occlude()`）就 `break`，插入更早
/// 的 hitbox（**祖先全部**）连 `ids` 都进不去，`hitbox.is_hovered()` 恒
/// false。用户气泡曾挂 `occlude()`（本意「禁止鼠标透传到下层消息」），于是
/// 鼠标停在气泡上时行级 on_hover 永不触发、操作栏不显影；agent 轮块没有遮挡
/// 后代，一切正常——这就是「同一种行为两处表现」的根因。
///
/// 挂在**有 id 的行**上（用户行 `msgrow-*` / 轮块 `turn-*`），故要
/// `StatefulInteractiveElement`：`on_hover` 在它上面，不在裸 `InteractiveElement`。
/// 回归锁见本文件测试模块 `bar_hover_hit_test`（正反两侧）。
fn bar_hover_wired<E: StatefulInteractiveElement>(
    el: E,
    weak: &gpui::WeakEntity<Chat>,
    ix: usize,
) -> E {
    let weak = weak.clone();
    el.on_hover(move |hovered, _, cx| {
        let _ = weak.update(cx, |c, cx| {
            let next = if *hovered { Some(ix) } else { None };
            if c.bar_hover != next {
                c.bar_hover = next;
                cx.notify();
            }
        });
    })
}

pub(crate) fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    expanded_skills: &std::collections::HashSet<String>,
    bubble_scrolls: &std::rc::Rc<
        std::cell::RefCell<std::collections::HashMap<String, gpui::ScrollHandle>>,
    >,
    _collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // Some(est_tokens) 仅当此消息是流式中的最后一条（工作中回复）
    _stream_info: Option<u64>,
    _meta: MsgMeta,
    // 操作栏悬停显影（Chat.bar_hover，pi-web hovered state parity）
    bar_revealed: bool,
    // 复制反馈（032 恢复）：runtime.copy_flash 点亮中 → 栏换 ✓ 已复制
    copied: bool,
) -> gpui::Div {
    // pi-web 消息间距 marginBottom 16（v 此前 22 偏大）
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    // 消息内文件路径/本地文件链接点击的目标（markdown 渲染构建期捕获进
    // 事件闭包；文件 md 预览走 content.rs 自设 base）
    markdown::set_link_target(weak.clone(), None);
    if m.role == Role::User {
        // v56-4 c18-c22（pi-web UserMessageView parity）：右对齐 85% 宽、
        // user_bg 底 + 1px 蓝边框 rgba(59,130,246,.2)、圆角 12、pad 8/12、
        // 内容走 markdown、内嵌图片 240 上限、超高 300px 内部滚动；
        // 操作行（复制/编辑/新分支）hover 淡入 = 自定义豁免项。
        let text = m.plain_text();

        // v58: 用户消息图片 = 气泡上方独立缩略图行（参考截图 parity，不再
        // 内嵌气泡）：67×67（composer 56 的 +20%）、间隔 5px、右对齐（随
        // 行 items_end）、点击开 ImagePreview 大图弹窗
        let image_row: Option<gpui::AnyElement> = {
            let mut thumbs: Vec<gpui::AnyElement> = Vec::new();
            for (bi, b) in m.blocks.iter().enumerate() {
                if let Block::Image { mime, data, .. } = b {
                    if let Some(format) = mime_to_image_format(mime) {
                        if let Ok(image) = decode_image_cached(data, format) {
                            let image_for_open = image.clone();
                            let weak_open = weak.clone();
                            thumbs.push(
                                div()
                                    .id(SharedString::from(format!("uimg-{msg_ix}-{bi}")))
                                    .size(px(67.))
                                    .flex_shrink_0()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_panel))
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let image = image_for_open.clone();
                                        let _ = weak_open.update(cx, |c, cx| {
                                            c.dlg.dialog =
                                                Some(crate::Dialog::ImagePreview { image });
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        gpui::img(image)
                                            .size_full()
                                            .object_fit(gpui::ObjectFit::Cover),
                                    )
                                    .into_any_element(),
                            );
                        }
                    }
                }
            }
            (!thumbs.is_empty()).then(|| {
                div()
                    .w(relative(0.85))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(5.))
                    .children(thumbs)
                    .into_any_element()
            })
        };
        // c19: 用户内容走 markdown；但 HTML 不渲染、标签原样显示
        // （render_user）——用户消息是发出内容的凭证，气泡吞标签会让
        // 用户无法核对 agent 实际收到的文本（v57 用户反馈）
        //
        // v57: 技能展开消息（CLI 把 /skill:xxx 展开成多行全文）默认折叠
        // 成 mono 命令行 + 展开箭头（pi-web parity），展开后内容区限高
        // 带滚动条；单行 /skill:xxx（未展开回显）按普通渲染
        // v57: pi-web UserMessageView parity——skill 展开消息（信封文本）
        // 默认折叠：mono 技能名 + 展开箭头 + 参数原文；展开显示全文
        // markdown；复制/编辑目标都是紧凑命令（copyTarget/editTarget parity）
        let command_text = skill_expansion_to_command(&text);
        let skill_key = m
            .entry_id
            .clone()
            .unwrap_or_else(|| format!("skill-{}", msg_ix));
        let skill_open = expanded_skills.contains(&skill_key);
        let md: gpui::AnyElement = if text.trim().is_empty() {
            div().into_any_element()
        } else if let Some(cmd) = &command_text {
            let (cmd_name, cmd_args) = match cmd.split_once(' ') {
                Some((n, a)) => (n.to_string(), a.to_string()),
                None => (cmd.clone(), String::new()),
            };
            let weak_skill = weak.clone();
            let chevron = if skill_open { "chevron-up" } else { "chevron-down" };
            let mut stack = div().flex().flex_col().w_full().gap(px(6.));
            stack = stack.child(
                div().flex().items_start().gap(px(8.)).flex_wrap()
                    .child(
                        div()
                            .id(SharedString::from(format!("skill-toggle-{}", msg_ix)))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(13.))
                            .text_color(rgb(t.accent))
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.85))
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                let key = skill_key.clone();
                                let _ = weak_skill.update(cx, |c, cx| {
                                    if !c.expanded_skills.remove(&key) {
                                        c.expanded_skills.insert(key);
                                    }
                                    cx.notify();
                                });
                            })
                            .child(SharedString::from(cmd_name))
                            .child(icon(chevron, 11., t.accent)),
                    )
                    .when(!cmd_args.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(crate::appearance::ui_size(14.))
                                .text_color(rgb(t.text))
                                .child(SharedString::from(cmd_args)),
                        )
                    }),
            );
            if skill_open {
                stack = stack.child(markdown::render_user(&text, t));
            }
            stack.into_any_element()
        } else {
            markdown::render_user(&text, t)
        };

        let scroll_key = m
            .entry_id
            .clone()
            .unwrap_or_else(|| format!("ububble-{}", msg_ix));
        let (scroll_handle, scroll_state) = {
            let mut map = bubble_scrolls.borrow_mut();
            let h = map
                .entry(scroll_key)
                .or_insert_with(gpui::ScrollHandle::new)
                .clone();
            let st = gpui_component::scroll::ScrollbarState::default();
            (h, st)
        };

        let bottom = user_action_bar(msg_ix, weak, &text, m.ts, bar_revealed, copied, t);
        // 纯图片消息（空文本）不渲染空泡；缩略图行在气泡上方
        let has_text = !text.trim().is_empty();
        let mut row = bar_hover_wired(
            div()
                .id(SharedString::from(format!("msgrow-{msg_ix}")))
                .w_full()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(3.)),
            weak,
            msg_ix,
        );
        if let Some(imgs) = image_row {
            row = row.child(imgs);
        }
        if has_text {
            row = row.child(
                div()
                    .max_w(relative(0.85))
                    .flex()
                    .min_w_0()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(SharedString::from(format!("ububble-{msg_ix}")))
                                    // c21: 超高气泡 300px 内部滚动（USER_BUBBLE_MAX_HEIGHT）
                                    .max_h(px(300.))
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll_handle)
                                    .flex_1()
                                    .min_w_0()
                                    // 此处**不得** occlude()：gpui hit_test 命中
                                    // BlockMouse 即 break，会把祖先（本行的
                                    // bar_hover_wired）一起挡掉 → 鼠标停在气泡上
                                    // 操作栏永不显影（v62 修复）。滚轮也一并受益：
                                    // 气泡内滚到底后能继续滚会话列表。
                                    .px(px(12.))
                                    .pr(px(14.))
                                    .py(px(8.))
                                    .rounded(px(12.))
                                    .bg(rgb(t.user_bg))
                                    .border_1()
                                    .border_color(gpui::rgba(theme::rgba_a(0x3b82f6, 0.2)))
                                    .text_color(rgb(t.text))
                                    .child(div().flex().flex_col().child(md)),
                            )
                            // 滚动条仅在实际溢出限高时渲染（max_offset>0
                            // = 内容超高；未溢出无条）
                            .children(
                                (scroll_handle.max_offset().height > px(0.)).then(|| {
                                    div()
                                        .absolute()
                                        .top(px(8.))
                                        .bottom(px(8.))
                                        .right(px(3.))
                                        .w(px(8.))
                                        .child(
                                            gpui_component::scroll::Scrollbar::vertical(
                                                &scroll_state,
                                                &scroll_handle,
                                            )
                                            .scroll_size(gpui::size(
                                                px(0.),
                                                px(300.)
                                                    + scroll_handle.max_offset().height,
                                            )),
                                        )
                                }),
                            ),
                    ),
            );
        }
        row = row.child(bottom);
        col = col.child(row);
    }
    // 非 User 臂已删：mod.rs 分发入口只在 m.role == Role::User 时调
    // render_msg（assistant 轮走 render_assistant_turn、bash 走 render_bash_msg），
    // 曾有的「单条 assistant」防御分支不可达，见 pzy 拆分前清理。
    col
}

/// Role::Bash 渲染（mod.rs 分发入口；pi-web BashExecutionView parity）：
/// 合成 toolCall 卡片复用 render_tool_card（bash 卡 = 命令预览 + 结果体），
/// 执行中加一行「执行中」状态。独立消息行，不进轮分组。
pub(crate) fn render_bash_msg(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    let Some(info) = &m.bash else {
        return col;
    };
    // pi-web：excluded 的命令显示 "bash (local)"（输出仅本地，不进模型）
    let name = if info.excluded { "bash (local)" } else { "bash" };
    let args = serde_json::json!({ "command": info.command }).to_string();
    // is_error 对齐 agent 的 bash 工具卡：非零退出码 = 红（取消不算）
    let is_error = !info.cancelled && info.exit_code.is_some_and(|c| c != 0);
    let done = !info.running;
    // pi-web bashExecutionToText 的后缀行（展示即模型所见）
    let mut result = info.output.trim_end().to_string();
    if result.is_empty() {
        result = "(no output)".into();
    }
    if info.cancelled {
        result.push_str("\n(command cancelled)");
    } else if let Some(code) = info.exit_code {
        if code != 0 {
            result.push_str(&format!("\nCommand exited with code {code}"));
        }
    }
    if info.truncated {
        result.push_str(&format!(
            "\n[Output truncated. Full output: {}]",
            info.full_output_path.as_deref().unwrap_or("")
        ));
    }
    let duration_s = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
    col = col.child(render_tool_card(
        msg_ix,
        0,
        name,
        &args,
        &result,
        is_error,
        &[],
        None,
        duration_s,
        false,
        done,
        weak,
        collapsed,
        t,
    ));
    if info.running {
        col = col.child(
            div()
                .mt(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text_dim))
                .child(
                    gpui::svg()
                        .path(SharedString::from("icons/loader.svg"))
                        .text_color(rgb(t.accent))
                        .size(px(11.))
                        .with_animation(
                            SharedString::from(format!("bash-spin-{msg_ix}")),
                            Animation::new(std::time::Duration::from_millis(900)),
                            |el, delta| {
                                el.with_transformation(gpui::Transformation::rotate(
                                    gpui::radians(delta * 2. * std::f32::consts::PI),
                                ))
                            },
                        )
                        .into_any_element(),
                )
                .child(SharedString::from(tr("composer.running_hint"))),
        );
    }
    col
}

/// Block 渲染可见性（组内条目计数/过滤用；空 thinking/空 text 跳过）。
/// 思考块永远可见（收起=一行），不随「展示思考」开关消失。
fn block_displayable(b: &Block) -> bool {
    match b {
        Block::Text { text, .. } | Block::Thinking { text, .. } => !text.trim().is_empty(),
        Block::ToolCall { name, .. } => !name.is_empty(),
        Block::Image { .. } => true,
    }
}

/// pi-web splitFinalAssistantBlocks：最终回答 = 尾部 text/image 连续段，
/// 返回首个 answer 块下标（无连续段时 == len）。
fn split_answer_start(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .rposition(|b| !matches!(b, Block::Text { .. } | Block::Image { .. }))
        .map_or(0, |i| i + 1)
}

/// pi-web hasFinalAssistantAnswer：存在非空 text 或 image 块。
fn msg_has_answer(m: &Msg) -> bool {
    m.blocks.iter().any(|b| match b {
        Block::Text { text, .. } => !text.trim().is_empty(),
        Block::Image { .. } => true,
        _ => false,
    })
}

/// 模型名行（pi-web MessageView model label）：模型名 + 流式期间的 ↓token
/// 估算 + t/s 徽章（四档配色同 pi-web：≥50 青 / ≥30 绿 / ≥15 黄 / 其余红）。
fn model_label_div(label: &str, est: Option<u64>, tps: Option<f32>, t: &theme::Theme) -> gpui::Div {
    // agent 名称 = 会话字号 -1（字体大小设置.md §2）
    let mut row = div()
        .text_size(crate::appearance::sess_size(-1.))
        .text_color(rgb(t.text_dim))
        .mb(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(SharedString::from(label.to_string()));
    if let Some(e) = est.filter(|e| *e > 0) {
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .text_color(rgb(t.text))
                .child(SharedString::from(format!(
                    "\u{2193} {}",
                    crate::services::format::fmt_thousand(e)
                ))),
        );
        if let Some(v) = tps {
            let bg = if v >= 50. {
                0x53b3cb
            } else if v >= 30. {
                0x9bc53d
            } else if v >= 15. {
                0xf9c22e
            } else {
                0xe01a4f
            };
            row = row.child(
                div()
                    .ml(px(6.))
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(gpui::rgb(bg))
                    .text_size(crate::appearance::sess_size(-1.))
                    .text_color(rgb(0xffffff))
                    .child(SharedString::from(format!("{v:.1} t/s"))),
            );
        }
    }
    row
}

/// pi-web `isLiveTail` 的平铺渲染：运行中的这一轮**不分组、不折叠**——每条
/// assistant 消息一个模型名行 + 全部块（thinking/toolCall/text 就地展开），
/// 流式那条的模型名行带 ↓token 估算 + t/s 徽章。
///
/// 「工作详情」折叠行与最终回答分区只在轮末整形后生成；usage 行/复制栏也照
/// pi-web MessageView 的 `!isStreaming` 规则留给轮末（见 `render_assistant_turn`）。
#[allow(clippy::too_many_arguments)]
fn live_turn_body(
    turn: &[&Msg],
    turn_ixs: &[usize],
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    model_label: &str,
    est: Option<u64>,
    tps: Option<f32>,
) -> gpui::Div {
    let mut body = div().flex().flex_col();
    for (i, (m, &gix)) in turn.iter().zip(turn_ixs).enumerate() {
        if m.role != Role::Assistant {
            continue;
        }
        let disp: Vec<&Block> = m.blocks.iter().filter(|b| block_displayable(b)).collect();
        if disp.is_empty() {
            continue;
        }
        // 轮内最后一条 = 正在流式输出/干活的那条（徽章与 streaming 态挂它）
        let streaming = i == turn.len() - 1;
        let mut item = div().mb(px(16.)).flex().flex_col().gap(px(8.));
        item = item.child(model_label_div(
            m.model.as_deref().unwrap_or(model_label),
            if streaming { est } else { None },
            if streaming { tps } else { None },
            t,
        ));
        // pi-web thinkingDurationFromFile：该消息的生成时长挂到 thinking 块
        let msg_think_dur = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
        for b in disp {
            item = item.child(render_block(b, gix, weak, collapsed, t, streaming, msg_think_dur));
        }
        body = body.child(item);
    }
    body
}

/// 一轮 agent 回复（用户消息 → 下一用户消息之间的全部 assistant 消息）。
/// pi-web ChatWindow 轮分组 parity（v56-1）：思考+工具调用全部收进
/// 「工作详情」组——有最终回答时默认折叠、流式中/无最终回答时展开；
/// 最终回答 = 末条 assistant 的尾部 text/image 连续段
/// （splitFinalAssistantBlocks parity），其前置块同样入组；每条 assistant
/// 消息自带模型名标签；usage 只取最终消息（omitUsage parity）。hover
/// 操作栏（复制整轮/用时/时间）为自定义豁免项。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_assistant_turn(
    turn: &[&Msg],
    // 轮内各消息的全局索引（thinking/copy key 用）
    turn_ixs: &[usize],
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    model_label: &str,
    // Some(est_tokens)：此轮正在流式（最后一个 assistant 消息）
    stream_info: Option<u64>,
    // Some(tps)：流式速度（pi-web 300ms tick 估算，四档配色）
    stream_tps: Option<f32>,
    meta: MsgMeta,
    // 操作栏悬停显影（Chat.bar_hover，pi-web hovered state parity）
    bar_revealed: bool,
    // 复制反馈（032 恢复）：runtime.copy_flash 点亮中 → 栏换 ✓ 已复制
    copied: bool,
    // 「新分支」目标（None = 锚点还没回来，按钮不出）
    fork: Option<ForkAnchor>,
) -> gpui::AnyElement {
    // pi-web onMouseEnter/Leave parity：悬停整轮（无遮挡后代，见 bar_hover_wired）
    let mut col = bar_hover_wired(
        div()
            .id(SharedString::from(format!("turn-{start_ix}")))
            .w_full()
            .mb(px(16.))
            .flex()
            .flex_col(),
        weak,
        start_ix,
    );
    let is_working = stream_info.is_some();

    // pi-web findFinalAssistantIndex：有 answer 连续段的末条 assistant，
    // 兜底取末条 assistant
    let final_pos = turn
        .iter()
        .rposition(|m| m.role == Role::Assistant && msg_has_answer(m))
        .or_else(|| turn.iter().rposition(|m| m.role == Role::Assistant));
    let Some(final_pos) = final_pos else {
        return col.into_any_element();
    };
    let final_msg = turn[final_pos];
    let final_gix = turn_ixs[final_pos];
    let answer_start = split_answer_start(&final_msg.blocks);
    let answer_len = final_msg.blocks.len() - answer_start;
    let final_error = final_msg.stop_reason.as_deref() == Some("error");
    let final_truncated = final_msg.stop_reason.as_deref() == Some("length");
    let has_final_answer = answer_len > 0 || final_error || final_truncated;

    // 复制整轮文本（hover 操作栏，自定义保留）
    let mut turn_text = String::new();
    for m in turn {
        for b in &m.blocks {
            if let Block::Text { text, .. } = b {
                if !text.trim().is_empty() {
                    if !turn_text.is_empty() {
                        turn_text.push_str("\n\n");
                    }
                    turn_text.push_str(text);
                }
            }
        }
    }

    // ---- 运行中：pi-web `isLiveTail` 平铺渲染（不分组、不折叠）----
    // pi-web ChatWindow：`isLiveTail = (sessionBusy || isStreaming) &&
    // endIdx === messages.length && userIdx === lastAnchorIdx` 时，把这一轮直接
    // 平铺——每条 assistant 一个模型名行、thinking/toolCall/text 就地展开，流式
    // 那条的模型名行带 ↓token 估算 + t/s 徽章。usage 行 / 复制栏（pi-web
    // MessageView `!isStreaming` 才渲染）与「工作详情」折叠行、最终回答分区一律
    // 等到**轮末整形**（下面的 !is_working 分支）才出现。
    // 旧实现轮中就套上折叠组：思考/工具被折起来（streaming 时看不见思考框），
    // 内容高度忽大忽小，还把翻页钉顶搅乱。
    let est = stream_info.filter(|_| is_working);
    let tps = stream_tps.filter(|_| is_working);
    if is_working {
        return col
            .child(live_turn_body(turn, turn_ixs, weak, collapsed, t, model_label, est, tps))
            .into_any_element();
    }

    // ---- 「工作详情」组：全部 assistant 消息的 thinking/toolCall，最终
    // 消息只贡献 answer 连续段之前的前置块（pi-web processViews parity）----
    let mut group_body = div().mt(px(8.)).flex().flex_col();

    let mut n_views = 0usize;
    let mut n_tools = 0usize;
    for (i, (m, &gix)) in turn.iter().zip(turn_ixs).enumerate().take(final_pos + 1) {
        if m.role != Role::Assistant {
            continue;
        }
        let end = if i == final_pos { answer_start } else { m.blocks.len() };
        let disp: Vec<&Block> = m.blocks[..end].iter().filter(|b| block_displayable(b)).collect();
        if disp.is_empty() {
            continue;
        }
        n_views += 1;
        // 块容器 gap 8（pi-web AssistantMessageView：label 下方 flexDirection
        // column + gap 8）；label 自带 mb 4 → label→首块 12px，与 pi-web 一致
        let mut item = div().mb(px(16.)).flex().flex_col().gap(px(8.));
        // 每条 assistant 消息自带模型名标签（pi-web AssistantMessageView）
        item = item.child(model_label_div(
            m.model.as_deref().unwrap_or(model_label),
            None,
            None,
            t,
        ));
        // pi-web thinkingDurationFromFile：该消息的生成时长挂到 thinking 块
        let msg_think_dur = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
        for b in disp {
            if matches!(b, Block::ToolCall { .. }) {
                n_tools += 1;
            }
            let streaming = is_working && i == turn.len() - 1;
            item = item.child(render_block(b, gix, weak, collapsed, t, streaming, msg_think_dur));
        }
        group_body = group_body.child(item);
    }

    if n_views > 0 {
        // 折叠状态：显式覆盖 > 每态默认（流式中/无最终回答 = 展开，
        // 有最终回答 = 折叠；pi-web defaultExpanded={!finalAnswerMessage}）
        let key = (start_ix, usize::MAX);
        let default_open = is_working || !has_final_answer;
        let open = collapsed.get(&key).copied().unwrap_or(default_open);
        let label = crate::i18n::tf(
            "nav.work_details",
            &[("m", n_views.to_string()), ("t", n_tools.to_string())],
        );
        let weak_fold = weak.clone();
        let fold_row = div()
            .id(SharedString::from(format!("work-fold-{start_ix}")))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .ml(px(-8.))
            // 工作详情行 = 会话字号（字体大小设置.md §2）
            .text_size(crate::appearance::sess_size(0.))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .rounded(px(6.))
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_fold.update(cx, |c, cx| {
                    let rt = c.rt();
                    rt.update(cx, |r, _| {
                        let next = !r.collapsed.get(&key).copied().unwrap_or(false);
                        r.collapsed.insert(key, next);
                    });
                    cx.notify();
                });
            })
            .child(icon(
                if open { "chevron-down" } else { "chevron-right" },
                12.,
                t.text_dim,
            ))
            .child(SharedString::from(label));
        col = col.child(fold_row);
        if open {
            col = col.child(group_body);
        }
    }

    // ---- 轮末整形：最终回答分区（pi-web finalAnswerMessage）----
    // 走到这里必然 !is_working（运行中已在上面平铺返回），所以模型名行不带
    // ↓token/t-s 徽章——pi-web 的徽章只属于 isStreaming 的那条消息。
    if answer_len > 0 || final_error || final_truncated {
        col = col.child(model_label_div(
            final_msg.model.as_deref().unwrap_or(model_label),
            None,
            None,
            t,
        ));
    }
    if answer_len > 0 {
        // pi-web 最终回答块容器 gap 8
        let mut answer = div().w_full().flex().flex_col().gap(px(8.));
        for b in &final_msg.blocks[answer_start..] {
            answer = answer.child(render_block(b, final_gix, weak, collapsed, t, false, None));
        }
        col = col.child(answer);
    }
    // c26/c27（pi-web providerError / truncated parity）：错误红框、截断黄框
    if final_error {
        let msg = final_msg
            .error_message
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("Unknown provider error");
        col = col.child(alert_box(
            format!("Error: {msg}"),
            0xef4444,
            0xef4444,
            t,
        ));
    }
    if final_truncated {
        col = col.child(alert_box(
            tr("messages.truncated").to_string(),
            0xca8a04,
            0xeab308,
            t,
        ));
    }
    // c24: 轮内写文件 chips（pi-web TurnWrittenFiles parity：由
    // 成功的 write/edit/apply_patch 工具调用推导，绝不扫描回复文本）
    let written = turn_written_files(turn);
    if !written.is_empty() {
        let mut chips = div().flex().flex_wrap().gap(px(6.)).mt(px(6.));
        for path in &written {
            let p = path.clone();
            let weak_open = weak.clone();
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("wf-{start_ix}-{name}")))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(6.))
                    .bg(rgba(t.bg_subtle))
                    .border_1()
                    .border_color(rgb(t.border))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::sess_size(-2.))
                    .text_color(rgb(t.text))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let p = p.clone();
                        let _ = weak_open.update(cx, |c, cx| {
                            c.open_file_tab(std::path::PathBuf::from(&p), cx)
                        });
                    })
                    .child(icon("file", 12., t.text_dim))
                    .child(SharedString::from(name)),
            );
        }
        col = col.child(chips);
    }
    col = col.child(actions_bar::assistant_action_bar(
        start_ix,
        weak,
        &turn_text,
        turn.last().and_then(|m| m.end_ts),
        meta.turn_user_ts,
        is_working,
        bar_revealed,
        copied,
        fork,
        t,
    ));
    col.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（图片上下留白）：宽图受 max_h 约束时，绘制框必须同比例缩放，
    /// 不能把宽定成 720、高定成 520 —— 那样 Contain 会在盒内再留白。
    #[test]
    fn fit_box_keeps_aspect_ratio_inside_max() {
        // 2:1 宽图 → 受宽度约束（720×360，高度还远没到 520）
        assert_eq!(fit_box(2880, 1440, 720., 520.), (720., 360.));
        // 16:9 宽图 → 受宽度约束，高 405 < 520
        assert_eq!(fit_box(1920, 1080, 720., 520.), (720., 405.));
        // 竖图 → 受高度约束
        assert_eq!(fit_box(1000, 4000, 720., 520.), (130., 520.));
        // 小图不放大小（保持 gpui 原生像素尺寸）
        assert_eq!(fit_box(100, 50, 720., 520.), (100., 50.));
        // 退化输入不 panic
        assert_eq!(fit_box(0, 0, 720., 520.), (720., 520.));
    }

    #[test]
    fn natural_size_parses_png_jpeg_gif_bmp_webp() {
        // PNG 8 字节签名 + IHDR
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&[0; 20]);
        assert_eq!(
            image_natural_size(&png, gpui::ImageFormat::Png),
            Some((1920, 1080))
        );

        // JPEG：SOI + APP0 段 + SOF0
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.extend_from_slice(&16u16.to_be_bytes());
        jpg.extend_from_slice(&[0; 14]);
        jpg.extend_from_slice(&[0xFF, 0xC0]);
        jpg.extend_from_slice(&17u16.to_be_bytes());
        jpg.push(8);
        jpg.extend_from_slice(&1080u16.to_be_bytes());
        jpg.extend_from_slice(&1920u16.to_be_bytes());
        jpg.push(3);
        jpg.extend_from_slice(&[0; 9]);
        assert_eq!(
            image_natural_size(&jpg, gpui::ImageFormat::Jpeg),
            Some((1920, 1080))
        );

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        gif.extend_from_slice(&[0; 8]);
        assert_eq!(
            image_natural_size(&gif, gpui::ImageFormat::Gif),
            Some((320, 240))
        );

        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 16]);
        bmp.extend_from_slice(&(-800i32).to_le_bytes());
        bmp.extend_from_slice(&600i32.to_le_bytes());
        assert_eq!(
            image_natural_size(&bmp, gpui::ImageFormat::Bmp),
            Some((800, 600))
        );

        let mut vp8 = b"RIFF".to_vec();
        vp8.extend_from_slice(&[0; 4]);
        vp8.extend_from_slice(b"WEBP");
        vp8.extend_from_slice(b"VP8 ");
        vp8.extend_from_slice(&[0; 4]);
        vp8.extend_from_slice(&[0x30, 0x01, 0x00]); // 帧 tag
        vp8.extend_from_slice(&[0x9d, 0x01, 0x2a]); // 同步码
        vp8.extend_from_slice(&1280u16.to_le_bytes());
        vp8.extend_from_slice(&720u16.to_le_bytes());
        assert_eq!(
            image_natural_size(&vp8, gpui::ImageFormat::Webp),
            Some((1280, 720))
        );

        assert_eq!(image_natural_size(b"<svg/>", gpui::ImageFormat::Svg), None);
    }


}

#[cfg(test)]
mod skill_fold_tests {
    use super::skill_expansion_to_command;

    // pi-web lib/slash-display.test.mjs skillExpansion fixture 同款
    fn envelope(body: &str, args: Option<&str>) -> String {
        let args_s = args
            .map(|a| format!("\n\n{}", a))
            .unwrap_or_default();
        format!(
            "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are relative to /path/to/review.\n\n{body}\n</skill>{args}"
            ,
            body = body,
            args = args_s,
        )
    }

    #[test]
    fn restores_with_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Review the supplied files.", Some("src/main.ts"))).as_deref(),
            Some("/skill:review src/main.ts")
        );
    }
    #[test]
    fn restores_without_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Review the supplied files.", None)).as_deref(),
            Some("/skill:review")
        );
    }
    #[test]
    fn multiline_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Body.", Some("first line\nsecond line"))).as_deref(),
            Some("/skill:review first line\nsecond line")
        );
    }
    #[test]
    fn final_closing_tag_wins() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Example:\n</skill>\nContinue.", Some("src"))).as_deref(),
            Some("/skill:review src")
        );
    }

    #[test]
    fn lookalike_not_collapsed() {
        assert_eq!(skill_expansion_to_command("<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nordinary user text"), None);
        assert_eq!(skill_expansion_to_command("<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are elsewhere.\n\nbody\n</skill>"), None);
        assert_eq!(skill_expansion_to_command("ordinary user text"), None);
    }
}

#[cfg(test)]
mod bar_hover_hit_test {
    //! 用户消息行操作栏悬停（v62 修复）的 gpui 几何前提锁。
    //!
    //! `bar_hover_wired` 把 on_hover 挂在**整行**上，靠的是「行内后代不遮挡」
    //! 这条 gpui 语义：`Window::hit_test` 一旦命中 `HitboxBehavior::BlockMouse`
    //! （`div().occlude()`）就 `break`，被它挡住的祖先 hitbox 连 `ids` 都进不
    //! 去，`is_hovered()` 恒 false。用户气泡曾挂 `occlude()`，于是鼠标停在气泡
    //! 上时行级 on_hover 永不触发 → 操作栏不显影；agent 轮块无遮挡后代故一切
    //! 正常（同一个行为两处表现）。
    //!
    //! 两侧都锁：不遮挡 → 行悬停必须触发（线上行为）；遮挡 → 必须被挡掉（gpui
    //! 现语义；上游若改，这条先炸，提示回来复核用户行实现与上面那段注释）。

    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Context, IntoElement, Modifiers, ParentElement, Render, Styled, TestAppContext,
        VisualTestContext, Window, div, point, px, size, prelude::*,
    };

    /// 迷你复刻用户消息行：row = `bar_hover_wired` 的挂载点，bubble = 气泡本体
    /// （`bubble_occludes` 开关模拟 v62 前后两种写法）。
    struct Row {
        row_hovered: Rc<Cell<usize>>,
        bubble_hovered: Rc<Cell<usize>>,
        bubble_occludes: bool,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = self.row_hovered.clone();
            let bubble = self.bubble_hovered.clone();
            let bubble_el = div()
                .id("bubble")
                .when(self.bubble_occludes, |d| d.occlude())
                .w(px(100.))
                .h(px(40.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        bubble.set(bubble.get() + 1);
                    }
                });
            div()
                .id("row")
                .w(px(400.))
                .h(px(100.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        row.set(row.get() + 1);
                    }
                })
                .child(bubble_el)
        }
    }

    /// 画一帧后把指针移到气泡正中，返回 (行 on_hover 次数, 气泡 on_hover 次数)
    fn hover_bubble(cx: &mut TestAppContext, bubble_occludes: bool) -> (usize, usize) {
        let window: &mut VisualTestContext = cx.add_empty_window();
        let row_hovered: Rc<Cell<usize>> = Default::default();
        let bubble_hovered: Rc<Cell<usize>> = Default::default();
        let row = row_hovered.clone();
        let bubble = bubble_hovered.clone();
        let _ = window.draw(point(px(0.), px(0.)), size(px(400.), px(100.)), move |_, cx| {
            cx.new(|_| Row {
                row_hovered: row.clone(),
                bubble_hovered: bubble.clone(),
                bubble_occludes,
            })
        });
        window.simulate_mouse_move(point(px(50.), px(20.)), None, Modifiers::none());
        (row_hovered.get(), bubble_hovered.get())
    }

    /// 正例（v62 线上行为）：气泡不遮挡 → 行悬停触发 → `Chat.bar_hover` 置位
    /// → 操作栏显影，与 agent 轮块同构。
    #[gpui::test]
    fn bubble_without_occluder_lets_row_hover_fire(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, false);
        assert_eq!(bubble, 1, "指针在气泡内，气泡自身必须 hover");
        assert_eq!(row, 1, "行级 on_hover 必须触发——缺了它操作栏就不显影");
    }

    /// 反例（v62 之前的线上行为，勿再退化）：气泡挂 `occlude()` → 行 hitbox 被
    /// 截断在 hit-test 之外 → 行级 on_hover 永不触发 → 操作栏不显影。
    #[gpui::test]
    fn bubble_occluder_swallows_row_hover(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, true);
        assert_eq!(bubble, 1);
        assert_eq!(row, 0, "occlude 会连祖先一起截断（gpui 语义若变，此断言先炸）");
    }
}
