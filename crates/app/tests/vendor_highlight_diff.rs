//! vendor gpui-component `unique_styles` 扫描线重写（PF 2026-10-16）的差分
//! 回归。vendor 的测试目标因 workspace 排除（dev-dependencies 无法解析）
//! 跑不了，这里把重写前的旧实现（「区间 × 样式」平方扫描，原样拷贝）作为
//! 参照，对随机样式集做新旧输出全等比对；另保留 vendor 原手写用例的期望
//! 输出，钉死语义没漂。
//!
//! 语义要点（两边都必须满足）：
//! - 区间样式 = 「按 styles 向量顺序扫描、对覆盖者依次 merge（后者覆盖
//!   前者字段）」；
//! - 未覆盖区间输出 default；
//! - 相邻同色区间合并，但**样式终点**（significant end）处不合并。

use gpui::{FontWeight, HighlightStyle, Hsla, UnderlineStyle};
use gpui_component::highlighter::unique_styles;
use std::ops::Range;

fn st(palette_ix: usize, weight_ix: usize) -> HighlightStyle {
    let colors = [gpui::red(), gpui::green(), gpui::blue(), gpui::yellow()];
    let mut s = HighlightStyle::default();
    s.color = Some(colors[palette_ix % colors.len()]);
    s.font_weight = Some(if weight_ix % 2 == 0 {
        FontWeight::NORMAL
    } else {
        FontWeight::BOLD
    });
    if weight_ix % 3 == 0 {
        s.underline = Some(UnderlineStyle {
            thickness: gpui::px(1.),
            ..Default::default()
        });
    }
    s
}

fn clean() -> HighlightStyle {
    HighlightStyle::default()
}

/// 重写前的旧实现，逐行照抄（vendor highlighter.rs 改动前版本）。
fn reference_unique_styles(
    total_range: &Range<usize>,
    styles: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if styles.is_empty() {
        return styles;
    }

    let mut intervals = std::collections::BTreeSet::new();
    let mut significant_intervals = std::collections::BTreeSet::new();

    intervals.insert(total_range.start);
    intervals.insert(total_range.end);
    for (range, _) in &styles {
        intervals.insert(range.start);
        intervals.insert(range.end);
        significant_intervals.insert(range.end);
    }

    let intervals: Vec<usize> = intervals.into_iter().collect();
    let mut result = Vec::with_capacity(intervals.len().saturating_sub(1));

    for i in 0..intervals.len().saturating_sub(1) {
        let interval = intervals[i]..intervals[i + 1];
        if interval.start >= interval.end {
            continue;
        }

        let mut top_style: Option<HighlightStyle> = None;
        for (range, style) in &styles {
            if range.start <= interval.start && interval.end <= range.end {
                if let Some(top_style) = &mut top_style {
                    merge_highlight_style(top_style, style);
                } else {
                    top_style = Some(*style);
                }
            }
        }

        if let Some(style) = top_style {
            result.push((interval, style));
        } else {
            result.push((interval, HighlightStyle::default()));
        }
    }

    let mut merged: Vec<(Range<usize>, HighlightStyle)> = Vec::with_capacity(result.len());
    for (range, style) in result {
        if let Some((last_range, last_style)) = merged.last_mut() {
            if last_range.end == range.start
                && *last_style == style
                && !significant_intervals.contains(&range.start)
            {
                last_range.end = range.end;
                continue;
            }
        }
        merged.push((range, style));
    }

    merged
}

fn merge_highlight_style(style: &mut HighlightStyle, other: &HighlightStyle) {
    if let Some(color) = other.color {
        style.color = Some(color);
    }
    if let Some(font_weight) = other.font_weight {
        style.font_weight = Some(font_weight);
    }
    if let Some(font_style) = other.font_style {
        style.font_style = Some(font_style);
    }
    if let Some(background_color) = other.background_color {
        style.background_color = Some(background_color);
    }
    if let Some(underline) = other.underline {
        style.underline = Some(underline);
    }
    if let Some(strikethrough) = other.strikethrough {
        style.strikethrough = Some(strikethrough);
    }
    if let Some(fade_out) = other.fade_out {
        style.fade_out = Some(fade_out);
    }
}

/// vendor 原手写用例（test_unique_styles）的期望输出，钉死语义。
#[test]
fn vendor_documented_case() {
    fn color_style(color: Hsla) -> HighlightStyle {
        let mut style = HighlightStyle::default();
        style.color = Some(color);
        style
    }
    let red = color_style(gpui::red());
    let green = color_style(gpui::green());
    let blue = color_style(gpui::blue());
    let clean_st = clean();

    let input = vec![
        (2..10, clean_st),
        (2..10, clean_st),
        (5..11, red),
        (2..6, clean_st),
        (10..15, green),
        (15..30, clean_st),
        (29..35, blue),
        (35..40, green),
        (45..60, blue),
    ];
    let expected = vec![
        (0..5, clean_st),
        (5..6, red),
        (6..10, red),
        (10..11, green),
        (11..15, green),
        (15..29, clean_st),
        (29..30, blue),
        (30..35, blue),
        (35..40, green),
        (40..45, clean_st),
        (45..60, blue),
        (60..65, clean_st),
    ];

    assert_eq!(unique_styles(&(0..65), input.clone()), expected);
    assert_eq!(reference_unique_styles(&(0..65), input), expected);
}

/// 确定性伪随机差分：新旧实现对同一输入必须输出完全一致。
#[test]
fn differential_random_inputs() {
    // xorshift64，固定种子保证可复现。
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for case in 0..2000 {
        let n = 1 + (next() % 60) as usize;
        let total = 0..n;
        let k = (next() % 40) as usize;
        let mut input = Vec::with_capacity(k);
        for _ in 0..k {
            let start = (next() as usize) % (n + 2); // 允许略越界，旧实现同样接受
            let len = (next() as usize) % 9;
            input.push((start..(start + len), st(next() as usize, next() as usize)));
        }

        let new_out = unique_styles(&total, input.clone());
        let ref_out = reference_unique_styles(&total, input);

        if new_out != ref_out {
            panic!(
                "case {case}: scan-line rewrite diverged from reference\ntotal: {total:?}\nnew: {new_out:?}\nref: {ref_out:?}"
            );
        }
    }
}

/// 空输入 / 全空样式 / 全覆盖单条：退化形状不炸、语义不变。
#[test]
fn degenerate_shapes() {
    for total in [0..0, 5..5, 0..1, 10..20] {
        for input in [
            vec![],
            vec![(3..3, st(0, 0))],
            vec![(0..usize::MAX, st(1, 1))],
            vec![(10..20, st(2, 2)), (10..20, st(3, 3))],
        ] {
            assert_eq!(
                unique_styles(&total, input.clone()),
                reference_unique_styles(&total, input),
                "total {total:?}"
            );
        }
    }
}
