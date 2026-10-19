//! Pi 扩展市场页（040-1-Pi的扩展市场.md）——标准页 + topbar 标签。
//!
//! 数据面在 [`pi_link::market::pi_ext_market`]（npm 搜索 + latest 清单 + 首页快照）；
//! 本文件只做页内状态机与渲染：
//!
//! - 进页面：读首页快照先上屏（0 网络），后台单次刷新（stale-while-revalidate）；
//! - 拉一页 = 1 次搜索 + ≤50 次清单；清单按 10 条一批补全，**每批判完就 append
//!   上屏**（不攒满一页、绝不「上了再移除」）；
//! - 已装/有新版全部在渲染时实时派生（`cfg.mc_pkgs_global` + `aux.plugin_updates`），
//!   装/卸/更走 040 既有 CLI op 管道（`mc_*_package`），**不在这里发明新写路径**；
//! - 内存预算很小：列表上限 200 条、kinds 表会话内有效、磁盘只有一份首页快照。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};
use pi_link::market::pi_ext_market::{
    HomeSnapshot, MarketEntry, MarketKind, PAGE_SIZE, SNAPSHOT_MAX,
};

use crate::Chat;
use crate::ContentView;
use crate::PanelTab;
use crate::appearance::theme::Theme;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::ui::tokens::{radius, space};
use crate::ui::{BtnSize, BtnVariant, TextInput};

/// 列表内存上限（200 条 = 4 页；内存稳定在几十 KB）。
const LIST_CAP: usize = 200;
/// 每批补清单的条数：= 补全并发数，一批正好一轮请求（实测 10 条一批 1.57s、
/// 8 条 ≈ 1s）。首批上屏 = 搜索 + 一批 ≈ 1.8-2.4s（见文档 §3.5）。
const BATCH: usize = 8;
/// 搜索框去抖。
const DEBOUNCE_MS: u64 = 250;
/// 行高（等高虚拟列表）。
const ROW_H: f32 = 72.;

/// 「未安装 / 已安装」筛选（040-1 UI 设计：右侧两个按钮）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExtFilter {
    #[default]
    NotInstalled,
    Installed,
}

/// Pi 扩展市场页状态（挂 `aux.market.pi_ext`）。
#[derive(Default)]
pub(crate) struct PiExtState {
    /// 搜索框（页面首次打开时创建；on_change 去抖后触发刷新）
    pub(crate) input: Option<gpui::Entity<TextInput>>,
    pub(crate) query: String,
    pub(crate) filter: ExtFilter,
    pub(crate) entries: Vec<MarketEntry>,
    /// 会话内 kinds 表（name → 资源类型；不落盘）
    pub(crate) kinds: HashMap<String, Vec<MarketKind>>,
    pub(crate) total: usize,
    /// 下一次搜索的起点（每次拉页 += PAGE_SIZE）
    pub(crate) from: usize,
    /// 单飞闸：为真时不再发新页请求
    pub(crate) loading: bool,
    /// 当前 entries 来自首页快照、后台正在刷新
    pub(crate) stale: bool,
    pub(crate) error: Option<String>,
    /// 网络失败但快照可用 → 顶部黄条
    pub(crate) offline: bool,
    /// 请求代际（换词/刷新 ++；回包不符即丢弃）
    pub(crate) gen_ix: u64,
    /// 行内操作中的包来源（`npm:<name>`）：转圈 + 全行按钮禁用
    pub(crate) pending: Option<String>,
    /// 已装载过（快照或网络），避免重复初始化
    pub(crate) loaded_once: bool,
}

// ---------------------------------------------------------------------------
// 打开 / 装载
// ---------------------------------------------------------------------------

impl Chat {
    /// 打开「Pi扩展市场」标准页（菜单项、后续快捷键都走这里）。
    pub(crate) fn open_pi_ext_market(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.market_menu = None;
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::PiExtMarket))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(PanelTab::PiExtMarket);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(ContentView::PiExtMarket);
        self.pi_ext_ensure_input(cx);
        self.pi_ext_ensure_loaded(cx);
        // 已装条目的「有新版」依赖 082 的检测结果：还没跑过就顺手补一次
        //（不阻塞首屏，后台线程 + 结果回来后 notify）
        if self.aux.plugin_updates.results.is_empty() && !self.aux.plugin_updates.checking {
            self.spawn_plugin_updates_check(cx);
        }
        cx.notify();
    }

    /// 搜索框懒创建：页面首次打开时建（TextInput 需要 `&mut Context`）。
    fn pi_ext_ensure_input(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.input.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| {
            let mut ti = TextInput::new(cx).placeholder(tr("market.search_placeholder"));
            ti.set_on_change(Box::new(move |v, cx| {
                let v = v.to_string();
                let _ = weak.update(cx, |c, cx| c.pi_ext_on_query(v, cx));
            }));
            ti
        });
        self.aux.market.pi_ext.input = Some(input);
    }

    /// 首次装载：读首页快照上屏 + 起一次后台刷新。已装载过则只补刷新。
    pub(crate) fn pi_ext_ensure_loaded(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.loaded_once {
            if self.aux.market.pi_ext.entries.is_empty() && !self.aux.market.pi_ext.loading {
                self.pi_ext_reload(cx);
            }
            return;
        }
        self.aux.market.pi_ext.loaded_once = true;
        if let Some(snap) = HomeSnapshot::load() {
            self.aux.market.pi_ext.query = snap.query.clone();
            self.aux.market.pi_ext.entries = snap
                .entries
                .into_iter()
                .filter(|e| e.is_extension())
                .take(SNAPSHOT_MAX)
                .collect();
            self.aux.market.pi_ext.stale = true;
            if let Some(ix) = self.aux.market.pi_ext.input.as_ref() {
                let q = self.aux.market.pi_ext.query.clone();
                ix.update(cx, |ti, cx| ti.set_value(q, cx));
            }
        }
        self.pi_ext_reload(cx);
    }

    /// 重新拉第一页（换词 / 手动刷新 / 重试共用）。
    pub(crate) fn pi_ext_reload(&mut self, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.gen_ix += 1;
        self.aux.market.pi_ext.from = 0;
        self.aux.market.pi_ext.total = 0;
        self.aux.market.pi_ext.error = None;
        self.aux.market.pi_ext.offline = false;
        // 快照内容留在屏幕上（stale），首批到达时整体替换
        self.aux.market.pi_ext.loading = false;
        self.pi_ext_fetch(cx);
    }

    /// 续页（列表底部「加载更多」）。
    pub(crate) fn pi_ext_load_more(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.entries.len() >= LIST_CAP || self.aux.market.pi_ext.loading {
            return;
        }
        self.aux.market.pi_ext.stale = false;
        self.pi_ext_fetch(cx);
    }

    /// 搜索框变化：去抖 250ms 后重拉（`gen_ix` 不匹配则放弃）。
    fn pi_ext_on_query(&mut self, v: String, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.query = v;
        self.aux.market.pi_ext.gen_ix += 1;
        let gen_ix = self.aux.market.pi_ext.gen_ix;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(DEBOUNCE_MS))
                .await;
            let _ = this.update(cx, |c, cx| {
                if c.aux.market.pi_ext.gen_ix == gen_ix {
                    c.pi_ext_reload(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 筛选切换：纯客户端重算，**0 网络**。
    pub(crate) fn pi_ext_set_filter(&mut self, f: ExtFilter, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.filter == f {
            return;
        }
        self.aux.market.pi_ext.filter = f;
        cx.notify();
    }

    /// 清错误 + 重试（红条/黄条上的按钮）。有快照时先回到快照内容。
    pub(crate) fn pi_ext_retry(&mut self, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.error = None;
        self.aux.market.pi_ext.offline = false;
        self.pi_ext_reload(cx);
    }
}

// ---------------------------------------------------------------------------
// 拉页（搜索 → 分批补清单 → 流式 append）
// ---------------------------------------------------------------------------

impl Chat {
    fn pi_ext_fetch(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.loading {
            return;
        }
        self.aux.market.pi_ext.loading = true;
        self.aux.market.pi_ext.error = None;
        let gen_ix = self.aux.market.pi_ext.gen_ix;
        let query = self.aux.market.pi_ext.query.clone();
        let from = self.aux.market.pi_ext.from;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let searched = cx
                .background_executor()
                .spawn(
                    async move { pi_link::market::pi_ext_market::search(&query, from, PAGE_SIZE) },
                )
                .await;
            let page = match searched {
                Ok(p) => p,
                Err(e) => {
                    let _ = this.update(cx, |c, cx| c.pi_ext_fail(gen_ix, e, cx));
                    return;
                }
            };
            let total = page.total;
            let mut raw = page.entries;
            if raw.is_empty() {
                let _ = this.update(cx, |c, cx| c.pi_ext_finish(gen_ix, total, cx));
                return;
            }
            while !raw.is_empty() {
                let take = raw.len().min(BATCH);
                let batch: Vec<MarketEntry> = raw.drain(..take).collect();
                let filled = cx
                    .background_executor()
                    .spawn(async move {
                        let mut entries = batch;
                        let mut kinds = HashMap::new();
                        pi_link::market::pi_ext_market::fill_kinds(&mut entries, &mut kinds);
                        (entries, kinds)
                    })
                    .await;
                let ok = this
                    .update(cx, |c, cx| c.pi_ext_append(gen_ix, filled.0, filled.1, cx))
                    .is_ok();
                if !ok {
                    return; // 实体已亡（窗口关闭）：停
                }
            }
            let _ = this.update(cx, |c, cx| c.pi_ext_finish(gen_ix, total, cx));
        })
        .detach();
    }

    /// 一批清单结果上屏：合并 kinds → 过滤（扩展 + 未安装/已安装）→ append。
    fn pi_ext_append(
        &mut self,
        gen_ix: u64,
        entries: Vec<MarketEntry>,
        kinds: HashMap<String, Vec<MarketKind>>,
        cx: &mut gpui::Context<Self>,
    ) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        // 快照内容是「旧的一页」：首批到达时整体替换（不追加、不闪烁）
        if self.aux.market.pi_ext.stale {
            self.aux.market.pi_ext.entries.clear();
            self.aux.market.pi_ext.stale = false;
        }
        self.aux.market.pi_ext.kinds.extend(kinds);
        // 只按「扩展」过滤；**未安装/已安装不在这里过滤**——那个状态随时会变
        // （装完立刻变），必须渲染时派生，否则切筛选看不到另一侧（空列表）
        for e in entries {
            if !e.is_extension() {
                continue;
            }
            if self.aux.market.pi_ext.entries.len() >= LIST_CAP {
                break;
            }
            self.aux.market.pi_ext.entries.push(e);
        }
        cx.notify();
    }

    fn pi_ext_finish(&mut self, gen_ix: u64, total: usize, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        self.aux.market.pi_ext.loading = false;
        self.aux.market.pi_ext.stale = false;
        self.aux.market.pi_ext.total = total;
        let first_page = self.aux.market.pi_ext.from == 0;
        self.aux.market.pi_ext.from = self.aux.market.pi_ext.from.saturating_add(PAGE_SIZE);
        // 首页快照：每次首页拉完覆盖写（唯一的磁盘缓存）
        if first_page {
            let snap = HomeSnapshot::new_now(
                &self.aux.market.pi_ext.query,
                self.aux
                    .market
                    .pi_ext
                    .entries
                    .iter()
                    .take(SNAPSHOT_MAX)
                    .cloned()
                    .collect(),
            );
            snap.save();
        }
        cx.notify();
    }

    fn pi_ext_fail(&mut self, gen_ix: u64, err: String, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        self.aux.market.pi_ext.loading = false;
        // 有内容（快照或已拉到的一半）→ 黄条降级；完全没有 → 红条
        if self.aux.market.pi_ext.entries.is_empty() {
            self.aux.market.pi_ext.error = Some(err);
        } else {
            self.aux.market.pi_ext.offline = true;
        }
        cx.notify();
    }

    /// 已装来源集合（渲染与过滤共用；`cfg.mc_pkgs_global` 是 op 泵维护的活数据）。
    fn pi_ext_installed_sources(&self) -> HashSet<String> {
        self.cfg
            .mc_pkgs_global
            .iter()
            .map(pi_link::skills::entry_source)
            .filter(|s| !s.is_empty())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 行内动作（安装 / 更新 / 卸载 / 复制命令）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn pi_ext_install(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        let source = format!("npm:{name}");
        self.aux.market.pi_ext.pending = Some(source.clone());
        self.mc_install_package(source, cx);
    }

    pub(crate) fn pi_ext_update(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        let source = format!("npm:{name}");
        self.aux.market.pi_ext.pending = Some(source.clone());
        self.mc_update_package(source, cx);
    }

    pub(crate) fn pi_ext_remove(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        // 高危动作统一走 040 的确认浮层（cfg.pkg_confirm_remove）
        self.mc_ask_remove_package(format!("npm:{name}"), cx);
    }

    pub(crate) fn pi_ext_update_all(&mut self, cx: &mut gpui::Context<Self>) {
        self.mc_update_all_packages(cx);
    }

    fn pi_ext_copy(&mut self, text: String, cx: &mut gpui::Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.set_status(tr("market.copied").to_string(), cx);
        cx.notify();
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

/// 行状态（渲染时派生，不落状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowStatus {
    NotInstalled,
    UpdateAvailable,
    Installed,
}

struct RowData {
    name: String,
    desc: String,
    downloads: String,
    kinds: String,
    cmd: String,
    status: RowStatus,
    busy: bool,
}

/// 页面主体（content.rs `ContentView::PiExtMarket` 分支）。
pub(crate) fn page(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let t: &'static Theme = T();
    let st = &chat.aux.market.pi_ext;
    let installed = chat.pi_ext_installed_sources();
    let pending_source = st.pending.clone();
    let busy_any = chat.cfg.pkg_op.is_some();

    let rows: Vec<RowData> = st
        .entries
        .iter()
        .filter(|e| match st.filter {
            ExtFilter::Installed => installed.contains(&e.source()),
            ExtFilter::NotInstalled => !installed.contains(&e.source()),
        })
        .map(|e| {
            let source = e.source();
            let status = if !installed.contains(&source) {
                RowStatus::NotInstalled
            } else if chat.aux.plugin_updates.state_of(&source)
                == Some(crate::services::plugin_updates::PkgUpdateState::UpdateAvailable)
            {
                RowStatus::UpdateAvailable
            } else {
                RowStatus::Installed
            };
            RowData {
                name: e.name.clone(),
                desc: e.description.clone(),
                downloads: e.monthly_downloads.to_string(),
                kinds: kinds_label(&e.kinds),
                cmd: e.install_command(),
                status,
                busy: pending_source.as_deref() == Some(source.as_str()),
            }
        })
        .collect();

    let mut page = gpui::div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .bg(gpui::rgb(t.bg))
        .child(filter_bar(chat, weak, busy_any, t));

    if let Some(banner) = status_banner(chat, weak, t) {
        page = page.child(banner);
    }

    // 列表区：有行 → 虚拟列表；空 → 骨架/空态
    let list_area = if rows.is_empty() {
        empty_area(chat, t)
    } else {
        crate::ui::vlist::vlist(
            "pi-ext-market-list",
            rows.len(),
            ROW_H,
            crate::ui::vlist::VListHeight::Fill,
            false,
            true,
            tr("market.empty"),
            None,
            {
                let weak = weak.clone();
                move |ix, _window, _cx| market_row(&rows[ix], &weak, t, busy_any)
            },
        )
    };
    page = page.child(
        gpui::div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list_area),
    );

    // 底栏：加载中 / 加载更多 / 到底提示
    let entries_len = st.entries.len();
    let mut footer = gpui::div()
        .flex_shrink_0()
        .h(gpui::px(38.))
        .flex()
        .items_center()
        .justify_center()
        .border_t_1()
        .border_color(gpui::rgb(t.border))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(gpui::rgb(t.text_dim));
    if st.loading {
        footer = footer
            .gap(gpui::px(space::SP2))
            .child(crate::ui::spinner(12., t.text_dim))
            .child(tr("market.loading").to_string());
    } else if entries_len >= LIST_CAP {
        footer = footer.child(tr("market.cap_reached").to_string());
    } else if st.from > 0 && st.total > entries_len {
        let weak_more = weak.clone();
        footer = footer.child(crate::ui::button(
            "pi-ext-more",
            tr("market.load_more"),
            BtnSize::Sm,
            BtnVariant::Secondary,
            false,
            move |_, _, cx| {
                let _ = weak_more.update(cx, |c, cx| c.pi_ext_load_more(cx));
            },
        ));
    } else if st.from > 0 {
        footer = footer.child(tr("market.end_of_list").to_string());
    }
    page = page.child(footer);

    page.into_any_element()
}

/// 顶部过滤栏：搜索框 + 【未安装】【已安装】+【全部更新(n)】。
fn filter_bar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    busy_any: bool,
    t: &'static Theme,
) -> AnyElement {
    let st = &chat.aux.market.pi_ext;
    let mut bar = gpui::div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP4))
        .px(gpui::px(space::SP5))
        .py(gpui::px(space::SP4))
        .border_b_1()
        .border_color(gpui::rgb(t.border));

    if let Some(input) = st.input.clone() {
        bar = bar.child(gpui::div().w(gpui::px(360.)).flex_shrink_0().child(input));
    }
    bar = bar.child(gpui::div().flex_1());

    // 未安装 / 已安装（单选切换，0 网络）
    for (filter, id, label) in [
        (
            ExtFilter::NotInstalled,
            "pi-ext-f-none",
            tr("market.filter.not_installed"),
        ),
        (
            ExtFilter::Installed,
            "pi-ext-f-inst",
            tr("market.filter.installed"),
        ),
    ] {
        let active = st.filter == filter;
        let weak_f = weak.clone();
        bar = bar.child(crate::ui::button(
            id,
            label,
            BtnSize::Sm,
            if active {
                BtnVariant::Primary
            } else {
                BtnVariant::Secondary
            },
            false,
            move |_, _, cx| {
                let _ = weak_f.update(cx, |c, cx| c.pi_ext_set_filter(filter, cx));
            },
        ));
    }

    // 全部更新(n)：n = 082 检测出的可更新数；0 或进行中 → 禁用
    let n = chat.aux.plugin_updates.available();
    let label = if n > 0 {
        format!("{} ({n})", tr("market.update_all"))
    } else {
        tr("market.update_all").to_string()
    };
    let weak_all = weak.clone();
    bar = bar.child(crate::ui::button(
        "pi-ext-update-all",
        label,
        BtnSize::Sm,
        BtnVariant::Primary,
        n == 0 || busy_any || chat.aux.plugin_updates.checking,
        move |_, _, cx| {
            let _ = weak_all.update(cx, |c, cx| c.pi_ext_update_all(cx));
        },
    ));
    bar.into_any_element()
}

/// 安全提示 + 离线黄条 / 错误红条（有则渲染，没有则 None）。
fn status_banner(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> Option<AnyElement> {
    let st = &chat.aux.market.pi_ext;
    let shell = || {
        gpui::div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(gpui::px(space::SP3))
            .px(gpui::px(space::SP5))
            .py(gpui::px(space::SP2))
            .text_size(crate::appearance::ui_size(11.))
    };

    if let Some(err) = &st.error {
        let weak_retry = weak.clone();
        return Some(
            shell()
                .bg(gpui::rgba(crate::appearance::theme::danger_wash(t)))
                .text_color(gpui::rgb(t.danger))
                .child(crate::i18n::tf("market.load_failed", &[("e", err.clone())]))
                .child(crate::ui::button(
                    "pi-ext-retry",
                    tr("market.retry"),
                    BtnSize::Sm,
                    BtnVariant::Danger,
                    false,
                    move |_, _, cx| {
                        let _ = weak_retry.update(cx, |c, cx| c.pi_ext_retry(cx));
                    },
                ))
                .into_any_element(),
        );
    }

    // 网络失败但有内容 → 黄条降级（显示缓存/已拉到的部分）
    if st.offline {
        let weak_retry = weak.clone();
        return Some(
            shell()
                .bg(gpui::rgb(t.bg_panel))
                .text_color(gpui::rgb(t.text_muted))
                .child(crate::i18n::tf(
                    "market.offline",
                    &[("n", st.entries.len().to_string())],
                ))
                .child(crate::ui::button(
                    "pi-ext-retry-2",
                    tr("market.retry"),
                    BtnSize::Sm,
                    BtnVariant::Secondary,
                    false,
                    move |_, _, cx| {
                        let _ = weak_retry.update(cx, |c, cx| c.pi_ext_retry(cx));
                    },
                ))
                .into_any_element(),
        );
    }

    // 常驻安全提示（第三方包 = 可执行 node 代码）
    Some(
        shell()
            .text_color(gpui::rgb(t.text_dim))
            .child(crate::ui::icon("info", 12., t.text_dim))
            .child(tr("market.security_hint").to_string())
            .into_any_element(),
    )
}

/// 空态：首屏加载中 → 骨架提示；否则空态文案。
fn empty_area(chat: &Chat, t: &'static Theme) -> AnyElement {
    let st = &chat.aux.market.pi_ext;
    let mut col = gpui::div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(gpui::px(space::SP3))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(gpui::rgb(t.text_dim));
    if st.loading {
        col = col
            .child(crate::ui::spinner(16., t.text_dim))
            .child(tr("market.loading").to_string());
    } else {
        col = col.child(tr("market.empty").to_string());
    }
    col.into_any_element()
}

/// 单行：名称/说明/下载量·类型·安装命令 + 【安装|更新】【卸载】。
fn market_row(
    row: &RowData,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    busy_any: bool,
) -> AnyElement {
    let line = gpui::div()
        .id(SharedString::from(format!("pi-ext-row-{}", row.name)))
        .h(gpui::px(ROW_H))
        .w_full()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP5))
        .px(gpui::px(space::SP5))
        .rounded(gpui::px(radius::CTRL))
        .hover(|s| s.bg(gpui::rgb(t.bg_hover)))
        // 说明（单行省略）
        .child(
            gpui::div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(gpui::px(space::SP1))
                .child(
                    gpui::div()
                        .text_size(crate::appearance::ui_size(14.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(gpui::rgb(t.text))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(row.name.clone())),
                )
                .child(
                    gpui::div()
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(gpui::rgb(t.text_muted))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(row.desc.clone())),
                )
                .child(meta_line(row, t)),
        )
        .child(actions(row, weak, t, busy_any));
    line.into_any_element()
}

/// 元信息行：下载量 · 类型 · 安装命令（等宽，可复制）。
fn meta_line(row: &RowData, t: &'static Theme) -> AnyElement {
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP4))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(gpui::rgb(t.text_dim))
        .child(
            gpui::div()
                .flex()
                .items_center()
                .gap(gpui::px(space::SP1))
                .child(crate::ui::icon("download", 11., t.text_dim))
                .child(SharedString::from(format!(
                    "{} {}",
                    row.downloads,
                    tr("market.per_month")
                ))),
        )
        .child(SharedString::from(row.kinds.clone()))
        .child(
            gpui::div()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .child(SharedString::from(row.cmd.clone())),
        )
        .into_any_element()
}

/// 行右侧按钮组。
fn actions(
    row: &RowData,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    busy_any: bool,
) -> AnyElement {
    let name = row.name.clone();
    let mut group = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP3))
        .flex_shrink_0();

    if row.busy {
        group = group.child(crate::ui::spinner(14., t.accent));
        return group.into_any_element();
    }

    match row.status {
        RowStatus::NotInstalled => {
            let weak_i = weak.clone();
            let n = name.clone();
            group = group.child(crate::ui::button(
                format!("pi-ext-install-{name}"),
                tr("market.btn.install"),
                BtnSize::Sm,
                BtnVariant::Primary,
                busy_any,
                move |_, _, cx| {
                    let n = n.clone();
                    let _ = weak_i.update(cx, |c, cx| c.pi_ext_install(n, cx));
                },
            ));
        }
        RowStatus::UpdateAvailable => {
            let weak_u = weak.clone();
            let n = name.clone();
            group = group.child(crate::ui::button(
                format!("pi-ext-update-{name}"),
                tr("market.btn.update"),
                BtnSize::Sm,
                BtnVariant::Primary,
                busy_any,
                move |_, _, cx| {
                    let n = n.clone();
                    let _ = weak_u.update(cx, |c, cx| c.pi_ext_update(n, cx));
                },
            ));
        }
        RowStatus::Installed => {
            group = group.child(crate::ui::button(
                format!("pi-ext-on-{name}"),
                tr("market.btn.installed"),
                BtnSize::Sm,
                BtnVariant::Secondary,
                true,
                |_, _, _| {},
            ));
        }
    }

    // 卸载：安装中的包才有（走 040 确认浮层）
    if row.status != RowStatus::NotInstalled {
        let weak_r = weak.clone();
        let n = name.clone();
        group = group.child(crate::ui::button(
            format!("pi-ext-remove-{name}"),
            tr("market.btn.remove"),
            BtnSize::Sm,
            BtnVariant::Danger,
            busy_any,
            move |_, _, cx| {
                let n = n.clone();
                let _ = weak_r.update(cx, |c, cx| c.pi_ext_remove(n, cx));
            },
        ));
    }

    // 复制安装命令
    let weak_c = weak.clone();
    let cmd = row.cmd.clone();
    let mut copy = gpui::div()
        .id(SharedString::from(format!("pi-ext-copy-{name}")))
        .flex()
        .items_center()
        .gap(gpui::px(space::SP1))
        .px(gpui::px(space::SP2))
        .py(gpui::px(space::SP1))
        .rounded(gpui::px(radius::XS))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgb(t.bg_selected)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let cmd = cmd.clone();
            let _ = weak_c.update(cx, |c, cx| c.pi_ext_copy(cmd, cx));
        });
    copy = copy.child(crate::ui::icon("copy", 12., t.text_dim));
    group = group.child(copy);
    group.into_any_element()
}

/// 类型徽标文案：`扩展` / `扩展 · 技能` / `—`（清单未补全）。
fn kinds_label(kinds: &[MarketKind]) -> String {
    if kinds.is_empty() {
        return tr("market.type.unknown").to_string();
    }
    kinds
        .iter()
        .map(|k| tr(k.i18n_key()))
        .collect::<Vec<_>>()
        .join(" · ")
}

// ---------------------------------------------------------------------------
// topbar 三横菜单（入口）：目前一项——「Pi扩展市场」
// ---------------------------------------------------------------------------

/// 菜单浮层（渲染挂 render.rs 根，与 psp_overlays 同层）：点卡片外 / ESC 关。
pub(crate) fn menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = chat.dlg.market_menu.expect("checked by caller");
    let edges = crate::ui::popup::PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.market_menu = None;
            cx.notify();
        });
    });
    let weak_item = weak.clone();
    let card = crate::function_panel::psp_overlays::menu_card(edges, t).child(
        crate::function_panel::psp_overlays::menu_item(
            "market-menu-pi-ext",
            "plug",
            tr("market.tab"),
            false,
            false,
        )
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak_item.update(cx, |c, cx| c.open_pi_ext_market(cx));
        }),
    );
    layer.child(card).into_any_element()
}
