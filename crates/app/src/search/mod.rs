//! 083 项目搜索——Zed `crates/search/src/project_search.rs` 面板的 PF 复刻。
//!
//! 三层职责：
//! - [`engine`]：执行层（路径候选 → glob 过滤 → 读盘扫描 → 流式回传），
//!   无索引、纯实时，语义对齐 Zed（取证与取舍见该文件头注释）；
//! - [`panel`]：搜索条（查询行 + 大小写/整词/正则三开关 + 命中计数 +
//!   prev/next + 过滤行 + 替换行），Zed `ProjectSearchBar` 的版式；
//! - [`results`]：结果面板（按文件分组的摘录块，命中高亮），Zed
//!   多缓冲结果编辑器的视觉等价物。
//!
//! 与 Zed 的唯一产品级差异：Zed 的「找文件」（Ctrl+P/文件查找器）与「找
//! 内容」（Ctrl+Shift+F 项目搜索）是两个面板，PF 把**文件名命中也并进同一
//! 结果列表**（唯一入口 Ctrl+Shift+F），因为用户的心智模型是「一次输入、
//! 文件与内容一起搜」。文件名命中排在最前的独立组里（`MAX_PATH_HITS` 封顶），
//! 内容命中按路径序分组在后。
//!
//! 全部状态住 [`SearchUi`]（挂在 `WsUi.search`，见 shell/chat.rs），因为搜索
//! 是工作区级而非会话级状态：切会话不丢、切项目重跑。

pub(crate) mod engine;
mod panel;
mod results;

pub(crate) use panel::{opts_labels, view};

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Context, Task};

use crate::*;
use engine::{CompiledQuery, FileHit, PathFilters, SearchMsg, SearchOpts};

/// 结果列表行高（与 Zed 结果编辑器一致：等宽等高的行，命中/上下文同高）。
/// 等高的代价是文件头也只能这么高——用底色 + 上边框做出分组感。
pub(crate) const ROW_H: f32 = 22.;
/// 结果列表 id（vlist 句柄表的键，`&'static str`）
const LIST_ID: &'static str = "project-search-results";
/// 搜索联动的防抖（Zed `SEARCH_ON_TYPE_DEBOUNCE` = 250ms）
const DEBOUNCE_MS: u64 = 250;

/// 搜索阶段（Zed `SearchState` 的简化版：PF 没有 Remote/OpenBuffersOnly）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Idle,
    Running,
    Done,
}

/// 结果列表的一行（扁平模型：文件头 + 摘录行，全部等高）。
#[derive(Clone, Debug)]
pub(crate) enum Row {
    /// 文件组头：路径 + 该文件命中行数（文件名命中 count = 0，表示「只有
    /// 文件名匹配，内容里没有」）
    Header {
        rel: String,
        count: usize,
        path_only: bool,
    },
    /// 摘录行（命中行 hit = true，上下文行 hit = false）
    Line {
        line: u32,
        text: String,
        spans: Vec<Range<usize>>,
        hit: bool,
        /// 命中行的全场序号（0-based）；导航与高亮活跃项都按它
        ordinal: Option<usize>,
    },
}

/// 项目搜索的界面状态。
pub(crate) struct SearchUi {
    /// 四个输入框（首次打开搜索面板时懒建；跨面板开关复用——清空靠
    /// set_value，不重建实体）
    pub(crate) query: Option<gpui::Entity<TextInput>>,
    pub(crate) replace: Option<gpui::Entity<TextInput>>,
    pub(crate) include: Option<gpui::Entity<TextInput>>,
    pub(crate) exclude: Option<gpui::Entity<TextInput>>,
    pub(crate) opts: SearchOpts,
    /// 查询词镜像（`on_change` 时同步；结果区空态判定与替换动作不再需要
    /// 读输入框实体，渲染帧零实体读）
    pub(crate) query_text: String,
    pub(crate) show_replace: bool,
    pub(crate) show_filters: bool,
    pub(crate) phase: Phase,
    /// 结果行模型。跨帧只克隆 `Arc`（每帧给 vlist 一份快照，零拷贝）；追加
    /// 走 `Arc::make_mut`，换代（新一轮搜索）直接换新 `Arc`。
    pub(crate) rows: Arc<Vec<Row>>,
    /// 命中的内容文件数 / 文件名命中数 / 命中行总数 / 已读盘文件数
    pub(crate) files: usize,
    pub(crate) path_files: usize,
    pub(crate) matches: usize,
    pub(crate) scanned: usize,
    /// 当前选中的命中序号（0-based，对应 `Row::Line.ordinal`）
    pub(crate) active: Option<usize>,
    /// 查询/过滤器的编译错误（Zed `panels_with_errors`：红字提示在条下）
    pub(crate) error: Option<String>,
    /// 触到结果上限（Zed LimitReached：计数后缀 `+`）
    pub(crate) limit: bool,
    /// 替换结果提示（文本, 是否失败）
    pub(crate) notice: Option<(String, bool)>,
    /// 代际：每次发起搜索 +1，陈旧批次的回包按代际丢弃
    pub(crate) generation: u64,
    /// 在跑的后台任务取消旗标（新一轮搜索先置位）
    pub(crate) cancel: Arc<AtomicBool>,
    /// 在跑的泵任务（drop 即取消）
    pub(crate) pending: Option<Task<()>>,
    /// 在跑的防抖任务（新输入 drop 旧任务 = 取消旧定时器，Zed
    /// `debounced_search` 同构）
    pub(crate) debounce: Option<Task<()>>,
    /// 请求渲染帧把焦点交给查询框（面板打开 / 快捷键重复触发）
    pub(crate) focus_query: bool,
}

impl Default for SearchUi {
    fn default() -> Self {
        Self {
            query: None,
            replace: None,
            include: None,
            exclude: None,
            opts: SearchOpts::default(),
            query_text: String::new(),
            show_replace: false,
            show_filters: false,
            phase: Phase::Idle,
            rows: Arc::new(Vec::new()),
            files: 0,
            path_files: 0,
            matches: 0,
            scanned: 0,
            active: None,
            error: None,
            limit: false,
            notice: None,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            pending: None,
            debounce: None,
            focus_query: false,
        }
    }
}

impl Chat {
    // -------------------------------------------------------------------
    // 入口：打开 / 聚焦搜索面板
    // -------------------------------------------------------------------

    /// Ctrl+Shift+F：打开搜索 tab（单例）；已打开则聚焦查询框。
    pub(crate) fn open_project_search(&mut self, cx: &mut Context<Self>) {
        self.ensure_search_inputs(cx);
        self.ws.search.focus_query = true;
        let existing = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::Search));
        match existing {
            Some(ix) => {
                self.activate_panel_tab(ix, cx);
                self.set_content_view(ContentView::Search);
            }
            None => {
                self.ws.panel_tabs.push(PanelTab::Search);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
                self.set_content_view(ContentView::Search);
            }
        }
        cx.notify();
    }

    /// 懒建四个输入框（`TextInput::new` 不需要 Window，可在动作里建）。
    /// 查询框：on_change → 防抖重搜；Enter → 立即搜；Esc → 焦点交给结果。
    fn ensure_search_inputs(&mut self, cx: &mut Context<Self>) {
        if self.ws.search.query.is_some() {
            return;
        }
        let weak_q = cx.weak_entity();
        let weak_enter = cx.weak_entity();
        let weak_esc = cx.weak_entity();
        let query = cx.new(|cx| {
            let mut ti = TextInput::new(cx)
                .placeholder(tr("search.placeholder"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    let q = q.to_string();
                    let _ = weak_q.update(cx, |c, cx| {
                        c.ws.search.query_text = q;
                        c.search_debounced(cx);
                    });
                }))
                .on_escape(Box::new(move |cx: &mut App| {
                    // Esc = 收起搜索 tab（回到会话；文件搜索弹窗同款语义）。
                    // defer 出帧再关：关 tab 会 drop 正在派发事件的输入框实体
                    let weak = weak_esc.clone();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |c, cx| c.close_search_tab(cx));
                    });
                }));
            // Enter = 立即搜索（Zed ConfirmSearch：不等防抖）
            ti.set_on_submit(Box::new(move |_q: &str, cx: &mut App| {
                let _ = weak_enter.update(cx, |c, cx| c.search_start(cx));
            }));
            ti
        });
        let weak_r = cx.weak_entity();
        let replace = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("search.replace_placeholder"))
                .on_change(Box::new(move |_q: &str, cx: &mut App| {
                    // 替换词只影响「替换」动作，不触发重搜（Zed 同：替换框
                    // 编辑不重跑搜索）
                    let _ = weak_r.update(cx, |c, cx| {
                        c.ws.search.notice = None;
                        cx.notify();
                    });
                }))
        });
        let weak_i = cx.weak_entity();
        let include = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("search.include_placeholder"))
                .on_change(Box::new(move |_q: &str, cx: &mut App| {
                    let _ = weak_i.update(cx, |c, cx| c.search_debounced(cx));
                }))
        });
        let weak_e = cx.weak_entity();
        let exclude = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("search.exclude_placeholder"))
                .on_change(Box::new(move |_q: &str, cx: &mut App| {
                    let _ = weak_e.update(cx, |c, cx| c.search_debounced(cx));
                }))
        });
        let ui = &mut self.ws.search;
        ui.query = Some(query);
        ui.replace = Some(replace);
        ui.include = Some(include);
        ui.exclude = Some(exclude);
    }

    // -------------------------------------------------------------------
    // 发起搜索
    // -------------------------------------------------------------------

    /// 防抖重搜（输入变更路径；Zed `schedule_search_on_type` 同构）。
    /// 旧定时器随任务 drop 取消——逐字输入不会排出 N 轮搜索。
    fn search_debounced(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        self.ws.search.debounce = Some(cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(DEBOUNCE_MS))
                .await;
            let _ = weak.update(cx, |c, cx| c.search_start(cx));
        }));
    }

    /// 立即搜索（Enter / 防抖到点 / 开关变化）。
    pub(crate) fn search_start(&mut self, cx: &mut Context<Self>) {
        self.ensure_search_inputs(cx);
        let ui = &mut self.ws.search;
        // 旧的一轮先取消（旗标 + drop 泵任务；后台循环每文件查一次旗标）
        ui.cancel.store(true, Ordering::Relaxed);
        ui.pending = None;
        ui.generation += 1;
        let generation = ui.generation;
        ui.cancel = Arc::new(AtomicBool::new(false));
        ui.rows = Arc::new(Vec::new());
        ui.files = 0;
        ui.path_files = 0;
        ui.matches = 0;
        ui.scanned = 0;
        ui.active = None;
        ui.limit = false;
        ui.error = None;
        ui.notice = None;

        let text = ui.query_text.clone();
        if text.is_empty() {
            ui.phase = Phase::Idle;
            cx.notify();
            return;
        }
        let Some(cwd) = self.ws.cwd.clone() else {
            ui.phase = Phase::Idle;
            ui.error = Some(tr("search.no_project").to_string());
            cx.notify();
            return;
        };
        let include = split_globs(
            ui.include
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let exclude = split_globs(
            ui.exclude
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let opts = ui.opts;
        let (query, filters) = match (
            CompiledQuery::new(&text, opts),
            PathFilters::new(&include, &exclude),
        ) {
            (Ok(q), Ok(f)) => (q, f),
            (Err(e), _) if !e.is_empty() => {
                ui.phase = Phase::Idle;
                ui.error = Some(e);
                cx.notify();
                return;
            }
            (_, Err(e)) => {
                ui.phase = Phase::Idle;
                ui.error = Some(e);
                cx.notify();
                return;
            }
            _ => {
                ui.phase = Phase::Idle;
                cx.notify();
                return;
            }
        };
        ui.phase = Phase::Running;
        let cancel = ui.cancel.clone();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<SearchMsg>();
        cx.background_spawn(async move {
            engine::run_search(cwd, query, filters, opts, cancel, tx);
        })
        .detach();

        // 前台泵：把批次收拢成一帧一次 notify（避免逐文件重绘）
        self.ws.search.pending = Some(cx.spawn(async move |weak, cx| {
            use futures::StreamExt as _;
            while let Some(first) = rx.next().await {
                let mut batch = vec![first];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                }
                let mut done = false;
                let alive = weak
                    .update(cx, |chat, cx| {
                        if chat.ws.search.generation != generation {
                            // 陈旧批次：整批丢弃（新一轮已接管）
                            return false;
                        }
                        for msg in batch {
                            done |= chat.search_apply(msg);
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive || done {
                    break;
                }
            }
        }));
        cx.notify();
    }

    /// 消费一条后台消息；返回 true = 本轮结束。
    fn search_apply(&mut self, msg: SearchMsg) -> bool {
        let ui = &mut self.ws.search;
        match msg {
            SearchMsg::PathHits(paths) => {
                ui.path_files = paths.len();
                let rows = Arc::make_mut(&mut ui.rows);
                for rel in paths {
                    rows.push(Row::Header {
                        rel,
                        count: 0,
                        path_only: true,
                    });
                }
                false
            }
            SearchMsg::Hit(hit) => {
                self.search_push_hit(hit);
                false
            }
            SearchMsg::Limit => {
                ui.limit = true;
                false
            }
            SearchMsg::Done { scanned } => {
                ui.scanned = scanned;
                ui.phase = Phase::Done;
                // 首个命中自动选中（Zed `update_match_index` 同义：搜索完成
                // 即有 active，prev/next 才有起点）
                if ui.active.is_none() && ui.matches > 0 {
                    ui.active = Some(0);
                }
                true
            }
        }
    }

    fn search_push_hit(&mut self, hit: FileHit) {
        let ui = &mut self.ws.search;
        ui.files += 1;
        let rows = Arc::make_mut(&mut ui.rows);
        rows.push(Row::Header {
            rel: hit.rel.clone(),
            count: hit.hits.len(),
            path_only: false,
        });
        for block in &hit.blocks {
            for (line, text) in block {
                // 命中行集合里有它才是命中行（上下文行同块混排）
                let spans = hit
                    .hits
                    .iter()
                    .find(|h| h.line == *line)
                    .map(|h| h.spans.clone())
                    .unwrap_or_default();
                let is_hit = !spans.is_empty();
                let ordinal = is_hit.then(|| {
                    let o = ui.matches;
                    ui.matches += 1;
                    o
                });
                rows.push(Row::Line {
                    line: *line,
                    text: text.clone(),
                    spans,
                    hit: is_hit,
                    ordinal,
                });
            }
        }
    }

    /// 搜索条任一输入框是否持焦——render 每帧焦点仲裁的白名单判据
    /// （同 040-1 市场搜索框/023 文件编辑器：不白名单的话每帧都会被抢回
    /// composer/中性锚点，搜索框点进去打不进字）。
    pub(crate) fn search_input_focused(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        let ui = &self.ws.search;
        [&ui.query, &ui.replace, &ui.include, &ui.exclude]
            .iter()
            .any(|slot| {
                slot.as_ref().is_some_and(|e| {
                    e.read(cx).has_inner() && e.read(cx).focus_handle_in(cx).is_focused(window)
                })
            })
    }

    /// 关搜索 tab（Esc / 标签 × / 标签菜单 Close 共用）：搜索态保留（再开
    /// 还是上次的查询与结果，Zed「搜索面板重开保留查询」同义）。
    pub(crate) fn close_search_tab(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::Search))
        else {
            return;
        };
        self.ws.search.cancel.store(true, Ordering::Relaxed);
        self.ws.search.pending = None;
        self.ws.search.debounce = None;
        self.close_panel_tab(ix, cx);
        if self.layout.content_view == ContentView::Search {
            self.fallback_after_closes(cx);
        }
        cx.notify();
    }

    /// 自动化入口：设置查询词并立即开搜（UI 测试 `search.query`）。
    pub(crate) fn set_search_query(&mut self, text: String, cx: &mut Context<Self>) {
        self.ensure_search_inputs(cx);
        self.ws.search.query_text = text.clone();
        if let Some(q) = self.ws.search.query.clone() {
            q.update(cx, |s, cx| s.set_value(text, cx));
        }
        self.open_project_search(cx);
    }

    /// 自动化入口：按名字切开关（UI 测试 `search.toggle`）。
    pub(crate) fn search_toggle_by_name(&mut self, which: &str, cx: &mut Context<Self>) {
        let w = match which {
            "case_sensitive" => SearchToggle::CaseSensitive,
            "whole_word" => SearchToggle::WholeWord,
            "regex" => SearchToggle::Regex,
            "include_ignored" => SearchToggle::IncludeIgnored,
            "filters" => SearchToggle::Filters,
            "replace" => SearchToggle::Replace,
            _ => return,
        };
        self.search_toggle(w, cx);
    }

    // -------------------------------------------------------------------
    // 开关 / 导航 / 跳转
    // -------------------------------------------------------------------

    /// 三个搜索开关（大小写 / 整词 / 正则）+ 两个过滤器开关
    /// （只搜打开的、扫 ignored）——切任一都立刻重搜（Zed 同）。
    pub(crate) fn search_toggle(&mut self, which: SearchToggle, cx: &mut Context<Self>) {
        let ui = &mut self.ws.search;
        match which {
            SearchToggle::CaseSensitive => ui.opts.case_sensitive = !ui.opts.case_sensitive,
            SearchToggle::WholeWord => ui.opts.whole_word = !ui.opts.whole_word,
            SearchToggle::Regex => ui.opts.regex = !ui.opts.regex,
            SearchToggle::IncludeIgnored => ui.opts.include_ignored = !ui.opts.include_ignored,
            SearchToggle::Filters => {
                ui.show_filters = !ui.show_filters;
                let show = ui.show_filters;
                cx.notify();
                if !show {
                    // 关过滤器 = 清空 include/exclude 并重搜（Zed ToggleFilters
                    // 不清输入，但 PF 的输入框会隐藏、留着条件会让结果莫名
                    // 变少——清掉更符合「关掉就是不要它影响」的直觉）
                    if let Some(inc) = ui.include.clone() {
                        inc.update(cx, |s, cx| s.set_value(String::new(), cx));
                    }
                    if let Some(exc) = ui.exclude.clone() {
                        exc.update(cx, |s, cx| s.set_value(String::new(), cx));
                    }
                    self.search_start(cx);
                }
                return;
            }
            SearchToggle::Replace => {
                ui.show_replace = !ui.show_replace;
                cx.notify();
                return;
            }
        }
        self.search_start(cx);
    }

    /// prev/next 命中（Zed SelectNextMatch/SelectPreviousMatch，默认环绕）。
    pub(crate) fn search_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let ords: Vec<(usize, usize)> = self
            .ws
            .search
            .rows
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| match row {
                Row::Line {
                    ordinal: Some(o), ..
                } => Some((ix, *o)),
                _ => None,
            })
            .collect();
        if ords.is_empty() {
            return;
        }
        let pos = self
            .ws
            .search
            .active
            .and_then(|a| ords.iter().position(|(_, o)| *o == a));
        let next = match (pos, forward) {
            (None, true) => 0,
            (None, false) => ords.len() - 1,
            (Some(i), true) => (i + 1) % ords.len(),
            (Some(i), false) => (i + ords.len() - 1) % ords.len(),
        };
        let (row_ix, ordinal) = ords[next];
        self.ws.search.active = Some(ordinal);
        crate::ui::vlist::scroll_handle(LIST_ID)
            .scroll_to_item(row_ix, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    /// 点一行结果：打开该文件的 tab 并跳到对应行（Zed 点击结果的跳转语义）。
    /// 文件名命中行没有行号，只开文件到首行。
    pub(crate) fn search_open_row(&mut self, row_ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.ws.search.rows.get(row_ix).cloned() else {
            return;
        };
        let (rel, line, ordinal) = match row {
            Row::Header { rel, .. } => (rel, 1, None),
            Row::Line { line, ordinal, .. } => {
                // 行所属文件：向上找最近的文件头
                let rel = self.ws.search.rows[..row_ix]
                    .iter()
                    .rev()
                    .find_map(|r| match r {
                        Row::Header { rel, .. } => Some(rel.clone()),
                        _ => None,
                    });
                match rel {
                    Some(rel) => (rel, line, ordinal),
                    None => return,
                }
            }
        };
        if let Some(o) = ordinal {
            self.ws.search.active = Some(o);
        }
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let abs = cwd.join(&rel);
        // 行跳转请求：文件 tab 渲染帧消费（view.rs 里灌进编辑器光标）
        self.ws.pending_jump = Some((abs.clone(), line));
        self.open_file_tab(abs, cx);
        cx.notify();
    }

    // -------------------------------------------------------------------
    // 替换
    // -------------------------------------------------------------------

    /// 替换全部命中（Zed ReplaceAll）。活跃文件有未保存修改时跳过（不写盘、
    /// 计数回报），避免与编辑器的脏缓冲打架。
    pub(crate) fn search_replace_all(&mut self, cx: &mut Context<Self>) {
        self.search_replace(cx, false);
    }

    /// 替换下一处（Zed ReplaceNext）：只动当前活跃命中所在文件的第一个
    /// 命中，然后重搜并把选中推进到下一处。
    pub(crate) fn search_replace_next(&mut self, cx: &mut Context<Self>) {
        self.search_replace(cx, true);
    }

    fn search_replace(&mut self, cx: &mut Context<Self>, next_only: bool) {
        let text = self.ws.search.query_text.clone();
        let replacement = self
            .ws
            .search
            .replace
            .as_ref()
            .map(|e| e.read(cx).value().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            return;
        }
        let Ok(q) = CompiledQuery::new(&text, self.ws.search.opts) else {
            return;
        };
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // 待改文件：ReplaceAll = 每个内容命中文件；ReplaceNext = 活跃命中
        // 所在文件（活跃序号反查所属文件头）
        let target: Option<String> = if next_only {
            let active = self.ws.search.active;
            self.ws
                .search
                .rows
                .iter()
                .enumerate()
                .find_map(|(ix, row)| match row {
                    Row::Line {
                        ordinal: Some(o), ..
                    } if Some(*o) == active => {
                        self.ws.search.rows[..ix]
                            .iter()
                            .rev()
                            .find_map(|r| match r {
                                Row::Header {
                                    rel,
                                    path_only: false,
                                    ..
                                } => Some(rel.clone()),
                                _ => None,
                            })
                    }
                    _ => None,
                })
        } else {
            None
        };
        let mut files: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in self.ws.search.rows.iter() {
            if let Row::Header {
                rel,
                path_only: false,
                ..
            } = row
            {
                if seen.insert(rel.clone()) && target.as_ref().is_none_or(|t| *t == *rel) {
                    files.push(rel.clone());
                }
            }
        }
        if files.is_empty() {
            self.ws.search.notice = Some((tr("search.replace_none").to_string(), true));
            cx.notify();
            return;
        }
        let mut changed_files = 0usize;
        let mut changed_hits = 0usize;
        let mut skipped_dirty = 0usize;
        for rel in &files {
            let abs = cwd.join(rel);
            // 有未保存修改的文件不动（写盘会触发冲突横幅，改动归属不明）
            if self
                .ws
                .file_cache
                .get(&abs)
                .is_some_and(|f| f.dirty || f.conflict.is_some())
            {
                skipped_dirty += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(&abs) else {
                continue;
            };
            let Ok(decoded) = crate::editor::file_content::decode_text(bytes) else {
                continue;
            };
            let src = decoded.text;
            let src = src.strip_suffix('\r').map(str::to_string).unwrap_or(src);
            let before = q.count_matches(&src);
            if before == 0 {
                continue;
            }
            let out = if next_only {
                // 只换第一处（按字节偏移精确定位后拼回，保留其余原文）
                match q.find_first(&src) {
                    Some(range) => {
                        let mut s = String::with_capacity(src.len());
                        s.push_str(&src[..range.start]);
                        s.push_str(&replacement);
                        s.push_str(&src[range.end..]);
                        s
                    }
                    None => continue,
                }
            } else {
                q.replace_all(&src, &replacement)
            };
            let encoded = crate::editor::file_content::encode_text(
                out.clone(),
                decoded.encoding,
                decoded.has_bom,
            );
            if std::fs::write(&abs, encoded).is_err() {
                continue;
            }
            changed_files += 1;
            changed_hits += if next_only { 1 } else { before };
            // 已开 tab 同步（内容 + 磁盘基准 + 编辑器值），避免下一次 watcher
            // 事件把它判成「外部改动」
            if let Some(ft) = self.ws.file_cache.get_mut(&abs) {
                ft.content = out.clone();
                ft.conflict = None;
                ft.dirty = false;
                ft.reload_pending = false;
                ft.disk_sig = crate::services::workspace::file_sig(&abs);
                // 编辑器实体的值由渲染帧灌入（set_value 需要 &mut Window）：
                // reload_pending 走既有的「外部改动灌值」通道
                if ft.editor.is_some() {
                    ft.reload_pending = true;
                }
            }
            if next_only {
                break;
            }
        }
        let mut msg = crate::i18n::tf(
            "search.replaced",
            &[
                ("files", changed_files.to_string()),
                ("hits", changed_hits.to_string()),
            ],
        );
        if skipped_dirty > 0 {
            msg.push_str(&crate::i18n::tf(
                "search.replace_skipped_dirty",
                &[("n", skipped_dirty.to_string())],
            ));
        }
        let failed = changed_files == 0;
        self.ws.search.notice = Some((msg, failed));
        // 重搜（磁盘已变，结果要跟着变）；ReplaceNext 顺势推进选中
        let advance = next_only && !failed;
        self.search_start(cx);
        if advance {
            self.search_step(true, cx);
        }
        cx.notify();
    }
}

/// include/exclude 输入框 → glob 列表（Zed 用逗号分隔；PF 额外支持换行）。
/// 单个 glob 内不允许含逗号，故无需考虑转义（Zed 同）。
pub(crate) fn split_globs(text: String) -> Vec<String> {
    text.split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// 搜索面板上的六个开关（三个匹配选项 + 过滤器/替换/扫 ignored）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchToggle {
    CaseSensitive,
    WholeWord,
    Regex,
    IncludeIgnored,
    Filters,
    Replace,
}
