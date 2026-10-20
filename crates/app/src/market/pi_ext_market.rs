//! Pi 扩展市场页（040-1）——标准页 + topbar 标签，v4 单列卡片 + 翻页。
//!
//! 数据面在 [`pi_link::market::pi_ext_market`]（npm 搜索 + latest 清单 + 首页快照）；
//! 本文件只做页内状态机与渲染：
//!
//! - **单列卡片**：一张卡 = 包名/版本/类型 chip + 两行内描述 + 作者·下载量·时间 +
//!   npm/repo 链接 + 安装命令条（**无复制键**，见 040-1 定稿）；
//! - **翻页**（官方式）：← 上一页 / 页码窗口 / 下一页 →；一页 50 条，
//!   **内存只留当前页**（页切换重新拉，kinds 表命中则只剩一次搜索）；
//! - 已装/有新版在渲染时实时派生；「已安装」不再用文字键表达（有卸载 icon 就够了）：
//!   未装 = 【安装】；有新版 = 【🗑】【更新】；已最新 = 【🗑】。**卸载永远在最左**；
//! - 动作列固定宽 104 → 跨行按钮严格对齐（不再随内容长度漂移）；
//! - 不再有安全提示条（用户定稿：别唠叨）。

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};
use pi_link::market::pi_ext_market::{
    HomeSnapshot, MarketEntry, MarketKind, PAGE_SIZE, SNAPSHOT_MAX, days_since,
};

use crate::Chat;
use crate::ContentView;
use crate::PanelTab;
use crate::appearance::theme::Theme;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::ui::tokens::{radius, space};
use crate::ui::{BtnSize, BtnVariant, TextInput};

/// 每批补清单的条数：= 补全并发数，一批正好一轮请求（实测 8 条 ≈1.0-1.6s）。
const BATCH: usize = 8;
/// 搜索框去抖。
/// 缩略槽宽高。
const THUMB_W: f32 = 88.;
const THUMB_H: f32 = 72.;
/// 最多可翻到第几页（npm 搜索分页深度保守上限；也是本次会话的内存天花板）。
const MAX_PAGES: usize = 20;
/// 左缘内缩 = 列表容器内缩 SP5(10)：工具栏 / 摘要 / 翻页条的**左缘与卡片外框同一竖线**。
const PAGE_LEFT: f32 = space::SP5;
/// 翻页条高（底部高度）。列表底部留白与它同高：
/// 「已安装」页签没有翻页条，若不留这一尺寸，末卡就胶在窗口底边上。
const PAGER_H: f32 = 44.;
/// 行右边距 = SP7(16) = 滚动条 8 + 间隙 8：滚动条压不到卡片。
///
/// 为何不用容器 pr：`vlist` 的滚动条是 wrapper 内的
/// 绝对定位归属，容器 pr 会把 wrapper（连同滚动条）一起左移，
/// 滚动条仍压在卡片上。只有「行内 mr」能让卡片退让滚动条。
const ROW_PAD_R: f32 = space::SP7;
/// 右缘内缩 = 行右边距：工具栏按钮 / 摘要 / 翻页条 ⊥ **卡片外框右缘**。
const PAGE_RIGHT: f32 = ROW_PAD_R;
/// 工具栏（input 行）上 padding。
const TOOLBAR_PT: f32 = 20.;
/// 工具栏（input 行）下 padding。
const TOOLBAR_PB: f32 = 15.;
/// 注释（摘要）行下 padding。
const SUMMARY_PB: f32 = 15.;
/// 描述行高（行高 18 × 行数）。用 `line_clamp` 硬截行（gpui 按行数 shape），
/// 不用固定高度盖裁（会出现半行）。
const DESC_LINE_H: f32 = 18.;
/// 描述最多行数。
const DESC_LINES: usize = 3;
/// 整页内容栏上限（工具栏/摘要/列表/翻页同栏，超宽窗口整体居中）
const PAGE_MAX_W: f32 = 1080. + PAGE_LEFT + ROW_PAD_R;

/// 卡片高度一套量（均跟面板字号缩放）。
///
/// 卡高**由内部部件算出来**，不写死字面量：写死一旦面板字号
/// 变就会被 flex 挤掉描述的第三行（半行问题）。
mod card_metrics {
    use super::*;
    /// 名称行行高（字号 15 / 行高 20）。
    pub(super) fn head_h() -> f32 {
        f32::from(crate::appearance::ui_size(20.))
    }
    /// 描述行高（18 × 行数）。
    pub(super) fn desc_h() -> f32 {
        f32::from(crate::appearance::ui_size(super::DESC_LINE_H)) * super::DESC_LINES as f32
    }
    /// 底行高（MD 按钮档 28；页面按钮统一 MD）。
    pub(super) fn bottom_h() -> f32 {
        crate::ui::tokens::btn::MD_H
    }
    /// 文本列的子元素：标题 / 描述 / 弹性占位 / 底行 —— 共 4 个。
    /// flex 列的 gap 在**6 个相邻对之间**各生效 1 次，共 3 个间隙。
    const TEXT_COL_ITEMS: usize = 4;
    /// 卡片内容高（不含内距与边框）。
    pub(super) fn body_h() -> f32 {
        head_h() + space::SP2 * (TEXT_COL_ITEMS - 1) as f32 + desc_h() + bottom_h()
    }
    /// 卡片总高（含内距与边框）。
    pub(super) fn card_h() -> f32 {
        body_h() + space::SP5 * 2. + 2.
    }
    /// 列表行高（卡片 + 行间空隙）。
    pub(super) fn row_h() -> f32 {
        card_h() + space::SP4
    }
}

/// 「未安装 / 已安装」筛选（040-1 UI 设计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExtFilter {
    #[default]
    NotInstalled,
    Installed,
}

/// 排序（Downloads = npm 服务端序；Updated / Name = **当前页内**客户端排序，
/// npm registry search 无排序参数，跨页全局排序 v1 做不到，不装懂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExtSort {
    #[default]
    Downloads,
    Updated,
    Name,
}

/// Pi 扩展市场页状态（挂 `aux.market.pi_ext`）。
#[derive(Default)]
pub(crate) struct PiExtState {
    /// 排序（下拉选择，纯渲染期重排，0 网络）
    pub(crate) sort: ExtSort,
    /// 排序下拉展开态
    pub(crate) sort_open: bool,
    /// 排序下拉的收起防抖守卫（ensure_input 时建）
    pub(crate) sort_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    /// 搜索框（页面首次打开时创建；on_change 去抖后触发刷新）
    pub(crate) input: Option<gpui::Entity<TextInput>>,
    pub(crate) query: String,
    pub(crate) filter: ExtFilter,
    /// 当前页的条目（**只留当前页**，上限 PAGE_SIZE 条）
    pub(crate) entries: Vec<MarketEntry>,
    /// 会话内 kinds 表（name → 资源类型；不落盘）。翻回旧页时命中即 0 网络。
    pub(crate) kinds: HashMap<String, Vec<MarketKind>>,
    pub(crate) total: usize,
    /// 当前页（1 起）
    pub(crate) page: usize,
    /// 单飞闸：为真时不再发新页请求
    pub(crate) loading: bool,
    /// 当前 entries 来自首页快照、后台正在刷新（首批到达时整体替换）
    pub(crate) stale: bool,
    pub(crate) error: Option<String>,
    /// 网络失败但快照可用 → 顶部黄条
    pub(crate) offline: bool,
    /// 请求代际（换词/翻页/刷新 ++；回包不符即丢弃）
    pub(crate) gen_ix: u64,
    /// 行内操作中的包来源（`npm:<name>`）：转圈 + 全行按钮禁用
    pub(crate) pending: Option<String>,
    /// 已装载过（快照或网络），避免重复初始化
    pub(crate) loaded_once: bool,
}

impl PiExtState {
    /// 总页数：按服务端 total 折算，且不超过 [`MAX_PAGES`]。
    pub(crate) fn total_pages(&self) -> usize {
        let pages = self.total.div_ceil(PAGE_SIZE).max(1);
        pages.min(MAX_PAGES)
    }

    /// 翻页口径的唯一来源（渲染与点击处理共用）：返回 (当前页, 总页数)。
    ///
    /// 已安装页签 = 本地全量、不分页 → 固定 (1, 1)；若还用全局 total 折算，
    /// 点「下一页」会跳到一个不存在的第 2 页。
    pub(crate) fn pager(&self) -> (usize, usize) {
        if self.filter == ExtFilter::Installed {
            (1, 1)
        } else {
            (self.page, self.total_pages())
        }
    }
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
        if self.aux.plugin_updates.results.is_empty() && !self.aux.plugin_updates.checking {
            self.spawn_plugin_updates_check(cx);
        }
        cx.notify();
    }

    /// 搜索框懒创建：页面首次打开时建（TextInput 需要 `&mut Context`）。
    fn pi_ext_ensure_input(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.sort_guard.is_none() {
            self.aux.market.pi_ext.sort_guard = Some(cx.new(|_| crate::ui::DropdownState::new()));
        }
        if self.aux.market.pi_ext.input.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| {
            let mut ti = TextInput::new(cx).placeholder(tr("market.search_placeholder"));
            // **打字不触发搜索**（npm 是海量远端数据，逐键打过去是灾难）；
            // 回车 / 「搜索」按钮 = 显式提交才搜。
            //
            // ⚠ 回调里**不能**回头 `input.read(cx)`：本回调就是 TextInput 自己
            // 在 update 中发出的，重复租会直接 panic「cannot read … while it
            // is already being updated」——就是「回车搜索程序崩掉」的原因。
            // 它已经把提交值传进来了，直接用。
            ti.set_on_submit(Box::new(move |v, cx| {
                let q = v.to_string();
                let _ = weak.update(cx, |c, cx| c.pi_ext_search_with(q, cx));
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
        crate::ui::vlist::reset_scroll(LIST_ID);
        self.aux.market.pi_ext.page = 1;
        self.pi_ext_fetch_page(1, cx);
    }

    /// 翻到指定页（页码条 / 上一页 / 下一页共用）。同页 = 无操作。
    pub(crate) fn pi_ext_goto(&mut self, page: usize, cx: &mut gpui::Context<Self>) {
        crate::ui::vlist::reset_scroll(LIST_ID);
        let page = page.max(1);
        if page == self.aux.market.pi_ext.page {
            return;
        }
        self.pi_ext_fetch_page(page, cx);
    }

    fn pi_ext_fetch_page(&mut self, page: usize, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.page = page;
        self.aux.market.pi_ext.gen_ix += 1;
        self.aux.market.pi_ext.error = None;
        self.aux.market.pi_ext.offline = false;
        self.aux.market.pi_ext.loading = false;
        if page > 1 {
            // 翻页不留旧页内容（内存只留当前页）；快照只是首页，翻页时一并清掉
            self.aux.market.pi_ext.entries.clear();
            self.aux.market.pi_ext.stale = false;
        } else if !self.aux.market.pi_ext.entries.is_empty() {
            // 第 1 页重搜/刷新：现有内容按 stale 处理，首批到达时整体替换。
            // **不标 stale 的话 append 的「满 50 跳过」会把新结果全部丢掉**
            // ——「第二次起搜索永远不变」的根因（v18 真机反馈）
            self.aux.market.pi_ext.stale = true;
        }
        self.pi_ext_fetch(cx);
    }

    /// 显式搜索（「搜索」按钮 / 回车）：读输入框文本 → query → 回第一页。
    /// 打字**不**触发搜索（用户定稿：海量远端内容禁止逐键自动搜）。
    /// 用**已知词**搜（回车回调路径：值由 TextInput 传入，不碰实体）。
    pub(crate) fn pi_ext_search_with(&mut self, query: String, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.query = query;
        self.pi_ext_reload(cx);
    }

    /// 读输入框再搜（「搜索」按钮路径：按钮事件不在 TextInput 的
    /// update 里，这里 `read` 是合法的）。
    pub(crate) fn pi_ext_search_now(&mut self, cx: &mut gpui::Context<Self>) {
        let v = self
            .aux
            .market
            .pi_ext
            .input
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        self.pi_ext_search_with(v, cx);
    }

    /// 筛选切换：纯客户端重算，**0 网络**。
    pub(crate) fn pi_ext_set_filter(&mut self, f: ExtFilter, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.filter == f {
            return;
        }
        self.aux.market.pi_ext.filter = f;
        crate::ui::vlist::reset_scroll(LIST_ID);
        cx.notify();
    }

    /// 清错误 + 重试当前页。
    pub(crate) fn pi_ext_retry(&mut self, cx: &mut gpui::Context<Self>) {
        let page = self.aux.market.pi_ext.page;
        self.aux.market.pi_ext.error = None;
        self.aux.market.pi_ext.offline = false;
        self.pi_ext_fetch_page(page, cx);
    }
}

// ---------------------------------------------------------------------------
// 拉页（搜索 → 分批补清单 → 流式 append）
// ---------------------------------------------------------------------------

impl Chat {
    fn pi_ext_fetch(&mut self, cx: &mut gpui::Context<Self>) {
        // 不再因「loading 就 return」静默吞请求：那会让当前轮的 loading 永远不置位
        // （按钮全灰、下一页不可点）。轮次由 `gen_ix` 分步，旧回包自归废弃。
        self.aux.market.pi_ext.loading = true;
        self.aux.market.pi_ext.error = None;
        let gen_ix = self.aux.market.pi_ext.gen_ix;
        let query = self.aux.market.pi_ext.query.clone();
        let from = self.aux.market.pi_ext.page.saturating_sub(1) * PAGE_SIZE;
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
            // total 一到就上屏：摘要行与翻页条不必等 50 条清单补完（原来要等 7s+）
            let _ = this.update(cx, |c, cx| c.pi_ext_set_total(gen_ix, total, cx));
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

    /// 一批清单结果上屏：合并 kinds → 过滤（只按「扩展」）→ append。
    /// 搜索回包一到就写 total（不等分批补清单）——摘要行/翻页条的显示前提。
    fn pi_ext_set_total(&mut self, gen_ix: u64, total: usize, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        self.aux.market.pi_ext.total = total;
        cx.notify();
    }

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
        // 快照内容 = 旧的第一页：首批到达时整体替换（不追加、不闪烁）
        if self.aux.market.pi_ext.stale {
            self.aux.market.pi_ext.entries.clear();
            self.aux.market.pi_ext.stale = false;
        }
        self.aux.market.pi_ext.kinds.extend(kinds);
        // 未安装/已安装不在这里过滤——那个状态随时会变，必须渲染时派生
        for e in entries {
            if !e.is_extension() {
                continue;
            }
            if self.aux.market.pi_ext.entries.len() >= PAGE_SIZE {
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
        // 首页快照：第一页拉完覆盖写（唯一的磁盘缓存）
        if self.aux.market.pi_ext.page == 1 {
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
// 行内动作（安装 / 更新 / 卸载）
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
    version: String,
    author: String,
    downloads: String,
    ago: String,
    repo: Option<String>,
    cmd: String,
    status: RowStatus,
    busy: bool,
    /// 已装条目在 `cfg.mc_pkgs_global` 里的下标（未装 = None）
    installed_ix: Option<usize>,
    /// 已装且启用中（settings 条目四数组未置空）
    enabled: bool,
}

/// 列表 id（vlist 滚动句柄表按 id 常驻；换内容回顶也用它寻址）
const LIST_ID: &str = "pi-ext-market-list";

/// 页面主体（content.rs `ContentView::PiExtMarket` 分支）。
pub(crate) fn page(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let t: &'static Theme = T();
    let st = &chat.aux.market.pi_ext;
    let installed = chat.pi_ext_installed_sources();
    let pending_source = st.pending.clone();
    let busy_any = chat.cfg.pkg_op.is_some();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let rows: Vec<RowData> = if st.filter == ExtFilter::Installed {
        // 已装页签 = **本地全量**：npm 搜索只回当前页 ≤50 条，装过的包未必
        // 都在里面（离线更是 0 条）。直接列 `mc_pkgs_global`；npm 缓存里有
        // 的字段借用，缺的（版本/描述）读本地 package.json 补齐。每次渲染
        // 每包 2 个小文件读，仅本页签生效，量级 = 已装包数，可接受。
        let mut rows = Vec::new();
        for (ix, v) in chat.cfg.mc_pkgs_global.iter().enumerate() {
            let source = pi_link::skills::entry_source(v);
            if source.is_empty() {
                continue;
            }
            let name = pi_link::skills::display_source(&source).to_string();
            let cached = st.entries.iter().find(|e| e.name == name);
            let status = if chat.aux.plugin_updates.state_of(&source)
                == Some(crate::services::plugin_updates::PkgUpdateState::UpdateAvailable)
            {
                RowStatus::UpdateAvailable
            } else {
                RowStatus::Installed
            };
            rows.push(RowData {
                name,
                desc: cached
                    .map(|e| e.description.clone())
                    .filter(|d| !d.is_empty())
                    .or_else(|| pi_link::skills::package_description(&source))
                    .unwrap_or_default(),
                version: pi_link::skills::package_installed_version(&source)
                    .or_else(|| cached.map(|e| e.version.clone()))
                    .unwrap_or_default(),
                author: cached.and_then(|e| e.author.clone()).unwrap_or_default(),
                downloads: cached
                    .map(|e| pi_link::market::pi_ext_market::format_downloads(e.monthly_downloads))
                    .unwrap_or_default(),
                ago: cached
                    .map(|e| ago_label(&e.updated, now))
                    .unwrap_or_default(),
                repo: cached.and_then(|e| e.repo.clone()),
                cmd: String::new(), // 已装没有「安装命令」可抄
                status,
                busy: pending_source.as_deref() == Some(source.as_str()),
                installed_ix: Some(ix),
                enabled: !pi_link::skills::entry_disabled(v),
            });
        }
        rows
    } else {
        let mut list: Vec<&pi_link::market::pi_ext_market::MarketEntry> = st
            .entries
            .iter()
            .filter(|e| match st.filter {
                ExtFilter::Installed => installed.contains(&e.source()),
                ExtFilter::NotInstalled => !installed.contains(&e.source()),
            })
            .collect();
        match st.sort {
            ExtSort::Downloads => {} // npm 服务端序（权重 ≈ 下载量）
            ExtSort::Updated => list.sort_by(|a, b| b.updated.cmp(&a.updated)),
            ExtSort::Name => list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        }
        list.into_iter()
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
                let installed_ix = chat
                    .cfg
                    .mc_pkgs_global
                    .iter()
                    .position(|v| pi_link::skills::entry_source(v) == source);
                let enabled = installed_ix
                    .and_then(|ix| chat.cfg.mc_pkgs_global.get(ix))
                    .map(|v| !pi_link::skills::entry_disabled(v))
                    .unwrap_or(true);
                RowData {
                    name: e.name.clone(),
                    desc: e.description.clone(),
                    version: e.version.clone(),
                    author: e.author.clone().unwrap_or_default(),
                    downloads: pi_link::market::pi_ext_market::format_downloads(
                        e.monthly_downloads,
                    ),
                    ago: ago_label(&e.updated, now),
                    repo: e.repo.clone(),
                    cmd: e.install_command(),
                    status,
                    busy: pending_source.as_deref() == Some(source.as_str()),
                    installed_ix,
                    enabled,
                }
            })
            .collect()
    };

    // 内容列：工具栏 / 摘要 / 列表 / 翻页全部住这里，外层 items_center
    // 让它在超宽窗口下**整体居中**（v20 只限列表不居中，用户驳回）
    let mut col = gpui::div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(toolbar(chat, weak, t));

    if let Some(banner) = banner(chat, weak, t) {
        col = col.child(banner);
    }
    // 摘要行：已装页签 = 本地条数；未安装页签 = npm 结果数（total 未知不占位）
    if st.filter == ExtFilter::Installed {
        col = col.child(
            gpui::div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_start()
                .gap(gpui::px(space::SP3))
                .pl(gpui::px(PAGE_LEFT))
                .pr(gpui::px(PAGE_RIGHT))
                .pt(gpui::px(0.))
                .pb(gpui::px(SUMMARY_PB))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(gpui::rgb(t.text_soft))
                .child(
                    gpui::div()
                        .text_color(gpui::rgb(t.text_dim))
                        .child(SharedString::from(thousands(rows.len()))),
                )
                .child(crate::i18n::tf(
                    "market.installed_summary",
                    &[("n", String::new())],
                ))
                // 「全部更新」在摘要行右端
                .child(gpui::div().flex_1())
                .child(update_all_button(chat, weak, busy_any)),
        );
    } else {
        let count = if st.total > 0 {
            st.total
        } else {
            st.entries.len()
        };
        col = col.child(
            gpui::div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_start()
                .gap(gpui::px(space::SP3))
                .pl(gpui::px(PAGE_LEFT))
                .pr(gpui::px(PAGE_RIGHT))
                .pt(gpui::px(0.))
                .pb(gpui::px(SUMMARY_PB))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(gpui::rgb(t.text_soft))
                .child(
                    gpui::div()
                        .text_color(gpui::rgb(t.text_dim))
                        .child(SharedString::from(if count == 0 {
                            "—".to_string()
                        } else {
                            thousands(count)
                        })),
                )
                .child(tr("market.results").to_string())
                .child(dot(t))
                .child(tr("market.only_ext").to_string())
                // 「全部更新」在摘要行右端
                .child(gpui::div().flex_1())
                .child(update_all_button(chat, weak, busy_any)),
        );
    }

    let list_area = if rows.is_empty() {
        if st.loading {
            skeleton_rows(t)
        } else {
            empty_area(chat, t)
        }
    } else {
        crate::ui::vlist::vlist(
            LIST_ID,
            rows.len(),
            // 行高 = 卡片高 + 行间空隙（由 card_metrics 统一算，与卡片实际高度一致）
            card_metrics::row_h(),
            crate::ui::vlist::VListHeight::Fill,
            false,
            true,
            tr("market.empty"),
            None,
            {
                let weak = weak.clone();
                move |ix, _window, _cx| pkg_card(&rows[ix], &weak, t, busy_any)
            },
        )
    };
    col = col.child(
        gpui::div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .pl(gpui::px(PAGE_LEFT))
            .child(list_area),
    );

    // 翻页条：未安装页签**恒常渲染**。底部永远占 44px，列表不会被撑到底边、
    // 末条不被削；total 未知时只有第 1 页（翻页按钮禁用）。
    // 翻页条**两个页签都渲染**（已安装页签 = 本地全量、无分页 → 整条禁用占位），
    // 底部结构与未安装页签完全一致：末卡不会贴到窗口底边，页面高度也不跳。
    //
    // 页数口径：未安装 = npm 搜索全局 total 折算；已安装 = **本地条数**
    // （已装列表是本地全量、不分页 → 只有第 1 页，不能用全局 total
    // 算出「第 1 / 248 页」这种说走的计数）。
    let (pager_page, pager_pages) = st.pager();
    col = col.child(pager(weak, pager_page, pager_pages, t));
    gpui::div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .bg(gpui::rgb(t.bg))
        .child(
            gpui::div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    gpui::div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .max_w(gpui::px(PAGE_MAX_W))
                        .flex()
                        .flex_col()
                        .child(col),
                ),
        )
        .into_any_element()
}

/// 工具栏：搜索 + 排序（固定最多下载）+ 未安装/已安装 + 全部更新(n)。
fn toolbar(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let st = &chat.aux.market.pi_ext;
    let mut bar = gpui::div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP4))
        .pl(gpui::px(PAGE_LEFT))
        .pr(gpui::px(PAGE_RIGHT))
        .pt(gpui::px(TOOLBAR_PT))
        .pb(gpui::px(TOOLBAR_PB));

    if let Some(input) = st.input.clone() {
        bar = bar.child(gpui::div().w(gpui::px(340.)).flex_shrink_0().child(input));
    }

    // 排序下拉：最多下载（服务端序）/ 最近更新 / 名称（本页客户端排序）
    if let Some(guard) = st.sort_guard.clone() {
        let sort = st.sort;
        let open = st.sort_open;
        let label = match sort {
            ExtSort::Downloads => tr("market.sort_downloads").to_string(),
            ExtSort::Updated => tr("market.sort_updated").to_string(),
            ExtSort::Name => tr("market.sort_name").to_string(),
        };
        let weak_t = weak.clone();
        let weak_d = weak.clone();
        let trigger = gpui::div()
            .id("pi-ext-sort-trigger")
            .flex()
            .items_center()
            .gap(gpui::px(space::SP2))
            .h(gpui::px(crate::ui::tokens::btn::MD_H))
            .px(gpui::px(crate::ui::tokens::btn::MD_PX))
            .rounded(gpui::px(radius::CTRL))
            .border_1()
            .border_color(gpui::rgb(t.border))
            .text_size(crate::appearance::ui_size(12.))
            .text_color(gpui::rgb(t.text_muted))
            .hover(|s| s.text_color(gpui::rgb(t.text)))
            .child(label)
            .child(crate::ui::icon(
                if open { "chevron-up" } else { "chevron-down" },
                12.,
                t.text_soft,
            ))
            .into_any_element();
        bar = bar.child(crate::ui::dropdown(
            "pi-ext-sort-dd",
            &guard,
            open,
            move |_, cx| {
                let _ = weak_t.update(cx, |c, cx| {
                    let st = &mut c.aux.market.pi_ext;
                    st.sort_open = !st.sort_open;
                    cx.notify();
                });
            },
            move |_, cx| {
                let _ = weak_d.update(cx, |c, cx| {
                    let st = &mut c.aux.market.pi_ext;
                    if st.sort_open {
                        st.sort_open = false;
                        cx.notify();
                    }
                });
            },
            trigger,
            move || {
                let mut menu = gpui::div()
                    .w(gpui::px(160.))
                    .py(gpui::px(space::SP2))
                    .bg(gpui::rgb(t.bg))
                    .border_1()
                    .border_color(gpui::rgb(t.border))
                    .rounded(gpui::px(radius::CARD))
                    .shadow_lg()
                    .overflow_hidden();
                /// 勾位宽度：选中打勾 / 未选中留白，文字不左右跳。
                const ICON_GUTTER: f32 = 12.;
                let icon_gutter = ICON_GUTTER;
                for (sopt, sid, slabel) in [
                    (
                        ExtSort::Downloads,
                        "pi-ext-sort-dl",
                        tr("market.sort_downloads").to_string(),
                    ),
                    (
                        ExtSort::Updated,
                        "pi-ext-sort-up",
                        tr("market.sort_updated").to_string(),
                    ),
                    (
                        ExtSort::Name,
                        "pi-ext-sort-nm",
                        tr("market.sort_name").to_string(),
                    ),
                ] {
                    let active = sopt == sort;
                    let weak_i = weak.clone();
                    menu = menu.child(
                        gpui::div()
                            .id(sid)
                            .h(gpui::px(28.))
                            .px(gpui::px(space::SP4))
                            .flex()
                            .items_center()
                            .gap(gpui::px(space::SP3))
                            .text_size(crate::appearance::ui_size(12.))
                            .cursor_pointer()
                            .rounded(gpui::px(radius::CTRL))
                            .when(active, |d| {
                                d.bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.10)))
                                    .text_color(gpui::rgb(t.accent))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                            })
                            .when(!active, |d| d.text_color(gpui::rgb(t.text_dim)))
                            .hover(|s| s.bg(gpui::rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_i.update(cx, |c, cx| {
                                    let st = &mut c.aux.market.pi_ext;
                                    st.sort = sopt;
                                    st.sort_open = false;
                                    crate::ui::vlist::reset_scroll(LIST_ID);
                                    cx.notify();
                                });
                            })
                            .child(
                                gpui::div()
                                    .w(gpui::px(icon_gutter))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .children(
                                        active.then(|| crate::ui::icon("check", 12., t.accent)),
                                    ),
                            )
                            .child(slabel),
                    );
                }
                menu.into_any_element()
            },
        ));
    }

    // 搜索按钮（回车同效）：打字去抖自动搜之外，给一个显式入口
    let weak_search = weak.clone();
    bar = bar.child(crate::ui::button(
        "pi-ext-search",
        tr("market.search"),
        BtnSize::Md,
        BtnVariant::Secondary,
        false,
        move |_, _, cx| {
            let _ = weak_search.update(cx, |c, cx| c.pi_ext_search_now(cx));
        },
    ));

    bar = bar.child(gpui::div().flex_1());

    // 未安装 / 已安装（单选切换，0 网络）
    let mut seg = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP1))
        .p(gpui::px(space::SP1))
        .h(gpui::px(crate::ui::tokens::btn::MD_H))
        .rounded(gpui::px(radius::CARD))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .bg(gpui::rgb(t.bg_panel));
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
        seg = seg.child(
            gpui::div()
                .id(id)
                .h(gpui::px(24.))
                .px(gpui::px(crate::ui::tokens::btn::SM_PX))
                .flex()
                .items_center()
                .rounded(gpui::px(radius::CTRL))
                .text_size(crate::appearance::ui_size(11.))
                .when(active, |d| {
                    d.bg(gpui::rgb(t.bg))
                        .text_color(gpui::rgb(t.text))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!active, |d| d.text_color(gpui::rgb(t.text_muted)))
                .cursor_pointer()
                .hover(|s| s.text_color(gpui::rgb(t.text)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = weak_f.update(cx, |c, cx| c.pi_ext_set_filter(filter, cx));
                })
                .child(label),
        );
    }
    // 未安装/已安装 切换「安装 switch」靠右（其后无其他元素）。
    // 「全部更新」已移到**摘要行右端**（用户定稿）。
    bar = bar.child(seg);
    bar.into_any_element()
}

/// 「全部更新 (n)」按钮：放在摘要行右端（与结果数同行，右对齐）。
fn update_all_button(chat: &Chat, weak: &gpui::WeakEntity<Chat>, busy_any: bool) -> AnyElement {
    let n = chat.aux.plugin_updates.available();
    let label = if n > 0 {
        format!("{} {n}", tr("market.update_all"))
    } else {
        tr("market.update_all").to_string()
    };
    let weak_all = weak.clone();
    crate::ui::button(
        "pi-ext-update-all",
        label,
        BtnSize::Md,
        BtnVariant::Secondary,
        n == 0 || busy_any || chat.aux.plugin_updates.checking,
        move |_, _, cx| {
            let _ = weak_all.update(cx, |c, cx| c.pi_ext_update_all(cx));
        },
    )
}

/// 离线黄条 / 错误红条（有则渲染）。
fn banner(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> Option<AnyElement> {
    let st = &chat.aux.market.pi_ext;
    let shell = || {
        gpui::div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(gpui::px(space::SP3))
            .ml(gpui::px(PAGE_LEFT))
            .mr(gpui::px(PAGE_RIGHT))
            .mt(gpui::px(space::SP2))
            .px(gpui::px(space::SP4))
            .py(gpui::px(space::SP2))
            .rounded(gpui::px(radius::CTRL))
            .text_size(crate::appearance::ui_size(11.))
    };

    if let Some(err) = &st.error {
        let weak_retry = weak.clone();
        return Some(
            shell()
                .bg(gpui::rgba(crate::appearance::theme::danger_wash(t)))
                .text_color(gpui::rgb(t.danger))
                .child(crate::ui::icon("info", 12., t.danger))
                .child(crate::i18n::tf("market.load_failed", &[("e", err.clone())]))
                .child(gpui::div().flex_1())
                .child(crate::ui::button(
                    "pi-ext-retry",
                    tr("market.retry"),
                    BtnSize::Md,
                    BtnVariant::Secondary,
                    false,
                    move |_, _, cx| {
                        let _ = weak_retry.update(cx, |c, cx| c.pi_ext_retry(cx));
                    },
                ))
                .into_any_element(),
        );
    }

    if st.offline {
        let weak_retry = weak.clone();
        return Some(
            shell()
                .bg(gpui::rgb(t.bg_panel))
                .text_color(gpui::rgb(t.text_muted))
                .child(crate::ui::icon("info", 12., t.text_muted))
                .child(crate::i18n::tf(
                    "market.offline",
                    &[("n", st.entries.len().to_string())],
                ))
                .child(gpui::div().flex_1())
                .child(crate::ui::button(
                    "pi-ext-retry-2",
                    tr("market.retry"),
                    BtnSize::Md,
                    BtnVariant::Secondary,
                    false,
                    move |_, _, cx| {
                        let _ = weak_retry.update(cx, |c, cx| c.pi_ext_retry(cx));
                    },
                ))
                .into_any_element(),
        );
    }
    None
}

/// 加载中骨架（设计稿「加载中（骨架 + 分批上屏）」态）：
/// 第一批清单还没上屏时，列表**先把位置占住**（行高/卡片尺寸与真卡一致），
/// 不让页面看起来是空的、也不让首批到达时跳一下。
fn skeleton_rows(t: &'static Theme) -> AnyElement {
    let bar = |w: f32, h: f32| {
        gpui::div()
            .w(gpui::px(w))
            .h(gpui::px(h))
            .rounded(gpui::px(radius::XS))
            .bg(gpui::rgb(t.bg_panel))
    };
    let mut col = gpui::div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden();
    for _ in 0..6 {
        col = col.child(
            gpui::div()
                .w_full()
                .pr(gpui::px(ROW_PAD_R))
                .mt(crate::appearance::ui_size(space::SP4))
                .child(
                    gpui::div()
                        .w_full()
                        .h(gpui::px(card_metrics::card_h()))
                        .flex()
                        .items_start()
                        .gap(gpui::px(space::SP7))
                        .px(gpui::px(space::SP7))
                        .py(gpui::px(space::SP5))
                        .rounded(gpui::px(radius::MODAL))
                        .border_1()
                        .border_color(gpui::rgb(t.border))
                        .child(
                            bar(THUMB_W, THUMB_H)
                                .rounded(gpui::px(radius::CTRL))
                                .flex_shrink_0(),
                        )
                        .child(
                            gpui::div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(gpui::px(space::SP3))
                                .child(bar(220., 12.))
                                .child(bar(520., 10.))
                                .child(bar(380., 10.))
                                .child(bar(160., 10.)),
                        ),
                ),
        );
    }
    col.into_any_element()
}

/// 空态：加载中 → 转圈；否则空态文案。
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
    } else if st.filter == ExtFilter::Installed {
        col = col
            .child(crate::ui::icon("plug", 22., t.text_faint))
            .child(tr("market.empty_installed").to_string());
    } else {
        col = col
            .child(crate::ui::icon("search", 22., t.text_faint))
            .child(tr("market.empty").to_string());
    }
    col.into_any_element()
}

/// 单张包卡（v15）：**两段式**
/// 上段 = 缩略槽 + 名称/描述/元信息；下段 = 链接 …… 命令胶囊 + 动作。
/// 命令胶囊与动作按钮在下段**同一行、只隔 SP4(8)**（原先隔着一整个动作列，
/// 视觉上空一大块）。动作右缘恒贴卡片内容右缘 → 跨行仍然对齐。
fn pkg_card(
    row: &RowData,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    busy_any: bool,
) -> AnyElement {
    let has_meta = !row.author.is_empty() || !row.downloads.is_empty() || !row.ago.is_empty();
    let card = gpui::div()
        .id(SharedString::from(format!("pi-ext-card-{}", row.name)))
        .h(gpui::px(card_metrics::card_h()))
        .w_full()
        .mt(gpui::px(space::SP4))
        .flex()
        .items_start()
        .gap(gpui::px(space::SP7))
        .px(gpui::px(space::SP7))
        .py(gpui::px(space::SP5))
        .rounded(gpui::px(radius::MODAL))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .bg(gpui::rgb(t.bg))
        .hover(|s| s.border_color(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.42))))
        .child(thumb_grid(t))
        .child(
            gpui::div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .gap(gpui::px(space::SP2))
                // ① 名称 + 版本 + 类型（右端：已装启停开关）
                .child(
                    gpui::div()
                        .flex()
                        .flex_shrink_0()
                        // 顶部对齐：开关/版本徽标贴行顶，使「上边距 = 下边距」
                        // （底行按钮贴卡片底内边距，两者都等于卡片 py）。
                        .items_start()
                        .gap(gpui::px(space::SP4))
                        .min_w_0()
                        .child(name_link(row, t))
                        .children((!row.version.is_empty()).then(|| badge(&row.version, t)))
                        .child(gpui::div().flex_1())
                        .children(switch(row, weak)),
                )
                // ② 描述：**恰好两行**的盒（行高×2，随面板字号缩放）；
                // 右内距 = 动作簇宽 + 8 —— 说明右缘与命令胶囊右缘同线，
                // **绝不伸进按钮空间**（用户定稿）
                .children((!row.desc.is_empty()).then(|| {
                    // 右内距 = 动作簇宽 + 8（与设置-扩展页同口径）：
                    // 描述右缘 ⊥ **命令胶囊右缘**，绝不伸进按钮空间。
                    // 宽度不再限 640：描述应跟到卡片内容右缘为止（按钮列在右）。
                    let acts_w = match row.status {
                        RowStatus::NotInstalled => 40.,    // [安装]
                        RowStatus::UpdateAvailable => 72., // [垃圾][更新]
                        RowStatus::Installed => 24.,       // [垃圾]
                    };
                    gpui::div()
                        .w_full()
                        .pr(gpui::px(acts_w + 8.))
                        .text_size(crate::appearance::ui_size(12.))
                        .line_height(crate::appearance::ui_size(DESC_LINE_H))
                        .text_color(gpui::rgb(t.text_muted))
                        // 硬截行而不是固高盖裁：line_clamp 按行数 shape，
                        // 行高恰好整数行 → 不会出现半行。
                        .line_clamp(DESC_LINES)
                        // 不参与 flex 收缩：卡片高度由内容算（card_metrics），
                        // 若这个盒允许被挤，第三行会被压成半行。
                        .flex_shrink_0()
                        .h(gpui::px(card_metrics::desc_h()))
                        .child(SharedString::from(row.desc.clone()))
                        .into_any_element()
                }))
                // 弹性占位：把底行顶到卡片底内边距上，
                // 保证「上边距 = 下边距 = 卡片 py」恒成立
                .child(gpui::div().flex_1())
                // ③ 底行（v17：在「倒数第二行」的位置，即与标题/描述同列缩进）
                .child(
                    gpui::div()
                        .w_full()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(gpui::px(space::SP4))
                        .children(meta_line(row, t))
                        .when(has_meta, |d| d.child(dot(t)))
                        .child(links(row, t))
                        .child(gpui::div().flex_1())
                        .children(cmd_pill(row, t))
                        .child(actions(row, weak, t, busy_any)),
                ),
        );
    // 行右边距：给 `vlist` 常显滚动条让位（滚动条压在卡片上 = 设计稿禁止）
    gpui::div()
        .w_full()
        .pr(gpui::px(ROW_PAD_R))
        .child(card)
        .into_any_element()
}

/// 缩略槽（设计稿 v28）：88×72 格子纸底纹 + 距底 26% 的 accent 细线。
///
/// 为什么不是图片：npm 搜索不回图；v1 是版式占位，
/// v2 换 `pi.image` 时只需替换本函数内容（尺寸/圆角/裁切不变）。
fn thumb_grid(t: &'static Theme) -> AnyElement {
    const STEP: f32 = 12.;
    let line = || {
        gpui::div()
            .absolute()
            .bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.16)))
    };
    let mut b = gpui::div()
        .relative()
        .w(gpui::px(THUMB_W))
        .h(gpui::px(THUMB_H))
        .flex_shrink_0()
        .rounded(gpui::px(radius::CTRL))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .bg(gpui::rgb(t.bg_panel))
        .overflow_hidden();
    let mut x = STEP;
    while x < THUMB_W {
        b = b.child(line().left(gpui::px(x)).top_0().bottom_0().w(gpui::px(1.)));
        x += STEP;
    }
    let mut y = STEP;
    while y < THUMB_H {
        b = b.child(line().top(gpui::px(y)).left_0().right_0().h(gpui::px(1.)));
        y += STEP;
    }
    let accent_y = (THUMB_H * 0.74).round();
    b.child(
        gpui::div()
            .absolute()
            .left_0()
            .right_0()
            .top(gpui::px(accent_y))
            .h(gpui::px(1.))
            .bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.5))),
    )
    .into_any_element()
}

/// 扩展名称：可点，点开 **pi.dev 详情页**（`pi.dev/packages/<name>`）。
///
/// 为什么是 pi.dev 而不是 npm：市场页的说明页在 pi.dev（模型适用的扩展中心，
/// 有安装指引与适配说明）；npm 页面只有 README。底行的「npm」链接仍指 npm（名字清晰不混）。
/// hover 转 accent，与卡片里其他链接同一反馈。
fn name_link(row: &RowData, t: &'static Theme) -> AnyElement {
    let url = format!("https://pi.dev/packages/{}", row.name);
    gpui::div()
        .id(SharedString::from(format!("pi-ext-name-{}", row.name)))
        .min_w_0()
        .text_size(crate::appearance::ui_size(15.))
        .line_height(crate::appearance::ui_size(20.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(gpui::rgb(if row.enabled { t.text } else { t.text_muted }))
        .cursor_pointer()
        .hover(|s| s.text_color(gpui::rgb(t.accent)))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            cx.open_url(&url);
        })
        .child(SharedString::from(row.name.clone()))
        .into_any_element()
}

fn badge(text: &str, t: &'static Theme) -> AnyElement {
    gpui::div()
        .flex_shrink_0()
        .h(gpui::px(18.))
        .px(gpui::px(space::SP2))
        .flex()
        .items_center()
        .rounded(gpui::px(radius::XS))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(10.))
        .text_color(gpui::rgb(t.text_faint))
        .child(SharedString::from(format!("v{text}")))
        .into_any_element()
}

/// 已装启停开关（卡片行内档 = `switch::SM`：**24×14 / knob 8**）。
///
/// 视觉配方与 `settings::widgets::switch_base`（MD 28×16）逐条对齐——1px 轨道描边、
/// 开态 accent、关态 `bg_selected`、关态 knob `text_muted`、开态 knob `bg`；
/// 唯一差别是尺寸走 SM 档（用户定稿：卡片里的开关要小，别用 28×16）。
///
/// 为什么不直接调 `switch_base`：它把 MD 尺寸写死，而 Settings 模块受保护
/// （未经授权不改）。等授权后给 `switch_base` 加 `size: SwitchSize` 参数，
/// 这里改成传入 SM 即可删掉本地绘制。
fn switch(row: &RowData, weak: &gpui::WeakEntity<Chat>) -> Option<AnyElement> {
    use crate::ui::tokens::switch as sw;
    let ix = row.installed_ix?;
    let on = row.enabled;
    let t = T();
    let weak_sw = weak.clone();
    let track: gpui::Hsla = if on {
        gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.10)).into()
    } else {
        gpui::rgb(t.bg_panel).into()
    };
    let border: gpui::Hsla = if on {
        gpui::rgb(t.accent).into()
    } else {
        gpui::rgb(t.border).into()
    };
    Some(
        gpui::div()
            // 第一行右端（flex 撑开后由弹黄盒推到最右，右缘 = 内容右缘）
            .child(
                gpui::div()
                    .id(SharedString::from(format!("pi-ext-sw-{}", row.name)))
                    .w(gpui::px(sw::SM_W))
                    .h(gpui::px(sw::SM_H))
                    .flex_shrink_0()
                    .rounded(gpui::px(sw::SM_H / 2.))
                    .border_1()
                    .border_color(border)
                    .bg(track)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = weak_sw.update(cx, |c, cx| c.mc_toggle_package(ix, cx));
                    })
                    .child(
                        gpui::div()
                            .ml(gpui::px(if on { sw::SM_ON_ML } else { sw::SM_OFF_ML }))
                            .size(gpui::px(sw::SM_KNOB))
                            .rounded_full()
                            .bg(if on {
                                gpui::rgb(t.accent)
                            } else {
                                gpui::rgb(t.text_soft)
                            }),
                    ),
            )
            .into_any_element(),
    )
}

/// 作者 · 下载量/月 · 发布距今天数。本地已装包可能三项全缺 → None（不占位）。
fn meta_line(row: &RowData, t: &'static Theme) -> Option<AnyElement> {
    let mut meta = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP3))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(gpui::rgb(t.text_soft));
    if !row.author.is_empty() {
        meta = meta
            .child(
                gpui::div()
                    .text_color(gpui::rgb(t.text_dim))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(SharedString::from(row.author.clone())),
            )
            .child(dot(t));
    }
    if !row.downloads.is_empty() {
        meta = meta
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(space::SP1))
                    .child(crate::ui::icon("download", 11., t.text_soft))
                    .child(SharedString::from(format!("{}/mo", row.downloads))),
            )
            .child(dot(t));
    }
    if !row.ago.is_empty() {
        meta = meta.child(SharedString::from(row.ago.clone()));
    }
    let empty = row.author.is_empty() && row.downloads.is_empty() && row.ago.is_empty();
    if empty {
        None
    } else {
        Some(meta.into_any_element())
    }
}

fn dot(t: &'static Theme) -> AnyElement {
    gpui::div()
        .w(gpui::px(2.))
        .h(gpui::px(2.))
        .rounded_full()
        .bg(gpui::rgb(t.text_faint))
        .into_any_element()
}

/// 链接（npm / repo）：下段最左，点击开系统浏览器。
fn links(row: &RowData, t: &'static Theme) -> AnyElement {
    let mut links = gpui::div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(gpui::px(space::SP6))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(gpui::rgb(t.text_soft));

    let npm_url = format!("https://www.npmjs.com/package/{}", row.name);
    links = links.child(
        gpui::div()
            .id(SharedString::from(format!("pi-ext-npm-{}", row.name)))
            .flex()
            .items_center()
            .gap(gpui::px(space::SP1))
            .cursor_pointer()
            .hover(|s| s.text_color(gpui::rgb(t.accent)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                cx.open_url(&npm_url);
            })
            .child(crate::ui::icon("plug", 11., t.text_soft))
            .child("npm"),
    );

    if let Some(repo) = &row.repo {
        let url = repo.clone();
        links = links.child(
            gpui::div()
                .id(SharedString::from(format!("pi-ext-repo-{}", row.name)))
                .flex()
                .items_center()
                .gap(gpui::px(space::SP1))
                .cursor_pointer()
                .hover(|s| s.text_color(gpui::rgb(t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    cx.open_url(&url);
                })
                .child(crate::ui::icon("git-branch", 11., t.text_soft))
                .child("repo"),
        );
    }
    links.into_any_element()
}

/// 安装命令胶囊：下段右端、**紧贴动作按钮（间隔 SP4=8）**，仅未安装时显示。
fn cmd_pill(row: &RowData, t: &'static Theme) -> Option<AnyElement> {
    if row.status != RowStatus::NotInstalled {
        return None;
    }
    Some(
        gpui::div()
            .flex_shrink_0()
            .h(gpui::px(22.))
            .max_w(gpui::px(360.))
            .flex()
            .items_center()
            .gap(gpui::px(space::SP2))
            .px(gpui::px(space::SP3))
            .rounded(gpui::px(radius::CTRL))
            .border_1()
            .border_color(gpui::rgb(t.border))
            .bg(gpui::rgb(t.bg_panel))
            .child(
                gpui::div()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(gpui::rgb(t.text_faint))
                    .child("$"),
            )
            .child(
                gpui::div()
                    .min_w_0()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(gpui::rgb(t.text_dim))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(row.cmd.clone())),
            )
            .into_any_element(),
    )
}

/// 动作列（固定宽 104，永远右对齐）：卸载在最左，其次 更新 / 安装。
fn actions(
    row: &RowData,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    busy_any: bool,
) -> AnyElement {
    let name = row.name.clone();
    let mut group = gpui::div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(gpui::px(space::SP4));

    if row.busy {
        group = group.child(crate::ui::spinner(14., t.accent));
        return group.into_any_element();
    }

    // 未安装：只给【安装】
    if row.status == RowStatus::NotInstalled {
        let weak_i = weak.clone();
        let n = name.clone();
        group = group.child(crate::ui::button(
            format!("pi-ext-install-{name}"),
            tr("market.btn.install"),
            BtnSize::Md,
            BtnVariant::Secondary,
            busy_any,
            move |_, _, cx| {
                let n = n.clone();
                let _ = weak_i.update(cx, |c, cx| c.pi_ext_install(n, cx));
            },
        ));
        return group.into_any_element();
    }

    // 已安装：卸载 icon 恒在**最左**（不再有「已安装」文字键）。
    // 垃圾桶：28×28 / 普通描边（v20 用户定稿：市场页不要红色按钮），
    // hover 淡底。确认浮层（mc_ask_remove_package）仍走高危确认。
    let weak_r = weak.clone();
    let n = name.clone();
    group = group.child(
        gpui::div()
            .id(SharedString::from(format!("pi-ext-remove-{name}")))
            .size(gpui::px(crate::ui::tokens::btn::MD_H))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(gpui::px(radius::CTRL))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x59)))
            .bg(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x0f)))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::rgba(crate::appearance::theme::danger_wash(t))))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let n = n.clone();
                let _ = weak_r.update(cx, |c, cx| c.pi_ext_remove(n, cx));
            })
            .child(crate::ui::icon("icon-trash-solid", 12., t.danger)),
    );

    // 有新版：更新键；已最新：什么都不加（卸载 icon 已是唯一动作）
    if row.status == RowStatus::UpdateAvailable {
        let weak_u = weak.clone();
        let n = name.clone();
        group = group.child(crate::ui::button(
            format!("pi-ext-update-{name}"),
            tr("market.btn.update"),
            BtnSize::Md,
            BtnVariant::Secondary,
            busy_any,
            move |_, _, cx| {
                let n = n.clone();
                let _ = weak_u.update(cx, |c, cx| c.pi_ext_update(n, cx));
            },
        ));
    }
    group.into_any_element()
}

// ---------------------------------------------------------------------------
// 翻页条（官方式：← 上一页 / 页码窗口 / 下一页 →）
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum PageBtn {
    Num(usize),
    Gap,
}

/// 页码窗口（纯函数，单测友好）：≤7 页全显示；否则 首/末 + 当前前后各 1，缺口用省略号。
fn page_window(page: usize, total: usize) -> Vec<PageBtn> {
    if total <= 7 {
        return (1..=total).map(PageBtn::Num).collect();
    }
    let mut out = vec![PageBtn::Num(1)];
    let start = page.saturating_sub(1).max(2);
    let end = (page + 1).min(total - 1);
    if start > 2 {
        out.push(PageBtn::Gap);
    }
    for p in start..=end.max(start) {
        out.push(PageBtn::Num(p));
    }
    if end < total - 1 {
        out.push(PageBtn::Gap);
    }
    out.push(PageBtn::Num(total));
    out
}

fn pager(
    weak: &gpui::WeakEntity<Chat>,
    page: usize,
    total_pages: usize,
    t: &'static Theme,
) -> AnyElement {
    let mut bar = gpui::div()
        .flex_shrink_0()
        .h(gpui::px(PAGER_H))
        .flex()
        .items_center()
        .gap(gpui::px(space::SP6))
        .pl(gpui::px(PAGE_LEFT))
        .pr(gpui::px(PAGE_RIGHT))
        .border_t_1()
        .border_color(gpui::rgb(t.border));

    // ← 上一页
    //
    // 不因 loading 禁用：一页要拉 50 个包的清单（约 7 秒），若禁用就是
    // “翻到第 2 页后按钮全灰、等不到”。上一轮的回包由 `gen_ix` 作废，
    // 不需要用禁用按钮来防重入。
    let prev_ok = page > 1;
    let weak_prev = weak.clone();
    bar = bar.child(pager_nav(
        "pi-ext-prev",
        tr("market.prev"),
        prev_ok,
        t,
        move |cx| {
            let _ = weak_prev.update(cx, |c, cx| {
                let (p, _) = c.aux.market.pi_ext.pager();
                if p > 1 {
                    c.pi_ext_goto(p - 1, cx);
                }
            });
        },
    ));

    // 页码（居中）
    let mut pages = gpui::div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(gpui::px(space::SP4));
    for btn in page_window(page, total_pages.max(1)) {
        match btn {
            PageBtn::Gap => {
                pages = pages.child(
                    gpui::div()
                        .px(gpui::px(space::SP1))
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(gpui::rgb(t.text_faint))
                        .child("…"),
                );
            }
            PageBtn::Num(n) => {
                let active = n == page;
                let weak_n = weak.clone();
                pages = pages.child(
                    gpui::div()
                        .id(SharedString::from(format!("pi-ext-page-{n}")))
                        .w(gpui::px(crate::ui::tokens::btn::MD_H))
                        .h(gpui::px(crate::ui::tokens::btn::MD_H))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(gpui::px(radius::CTRL))
                        .border_1()
                        .when(active, |d| {
                            d.border_color(gpui::rgb(t.accent))
                                .bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.10)))
                                .text_color(gpui::rgb(t.accent))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!active, |d| {
                            d.border_color(gpui::rgb(t.border))
                                .text_color(gpui::rgb(t.text_muted))
                        })
                        .text_size(crate::appearance::ui_size(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui::rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_n.update(cx, |c, cx| c.pi_ext_goto(n, cx));
                        })
                        .child(SharedString::from(n.to_string())),
                );
            }
        }
    }
    bar = bar.child(pages);

    // 下一页 →
    let next_ok = page < total_pages;
    let weak_next = weak.clone();
    bar = bar.child(pager_nav(
        "pi-ext-next",
        tr("market.next"),
        next_ok,
        t,
        move |cx| {
            let _ = weak_next.update(cx, |c, cx| {
                let (p, tp) = c.aux.market.pi_ext.pager();
                if p < tp {
                    c.pi_ext_goto(p + 1, cx);
                }
            });
        },
    ));
    bar.into_any_element()
}

fn pager_nav(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    t: &'static Theme,
    on_click: impl Fn(&mut gpui::App) + 'static,
) -> AnyElement {
    // 标签**永远绘制**（不能按 = 置灰，不消失）：v19 曾把 child(label) 只挂
    // 在 enabled 分支，禁用时渲染成空盒（用户截图的「幽灵按钮」）
    let mut el = gpui::div()
        .id(id)
        .h(gpui::px(crate::ui::tokens::btn::MD_H))
        .px(gpui::px(crate::ui::tokens::btn::MD_PX))
        .flex()
        .items_center()
        .rounded(gpui::px(radius::CTRL))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(gpui::rgb(t.text_muted))
        .child(label);
    if !enabled {
        return el.opacity(0.38).into_any_element();
    }
    el = el
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgb(t.bg_hover)).text_color(gpui::rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            on_click(cx);
        });
    el.into_any_element()
}

/// 千分位（12,365）。
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 「2 天前 / 今天」文案（`days_since` 纯函数出天数）。
fn ago_label(iso: &str, now_unix: i64) -> String {
    match days_since(iso, now_unix) {
        Some(0) => tr("market.today").to_string(),
        Some(d) if d > 0 => crate::i18n::tf("market.days_ago", &[("n", d.to_string())]),
        _ => String::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_window_shapes() {
        assert_eq!(
            page_window(1, 3),
            vec![PageBtn::Num(1), PageBtn::Num(2), PageBtn::Num(3)]
        );
        // 首页：1 2 … 20
        assert_eq!(
            page_window(1, 20),
            vec![
                PageBtn::Num(1),
                PageBtn::Num(2),
                PageBtn::Gap,
                PageBtn::Num(20)
            ]
        );
        // 中间页：1 … 9 10 11 … 20
        assert_eq!(
            page_window(10, 20),
            vec![
                PageBtn::Num(1),
                PageBtn::Gap,
                PageBtn::Num(9),
                PageBtn::Num(10),
                PageBtn::Num(11),
                PageBtn::Gap,
                PageBtn::Num(20)
            ]
        );
        // 末页
        assert_eq!(
            page_window(20, 20),
            vec![
                PageBtn::Num(1),
                PageBtn::Gap,
                PageBtn::Num(19),
                PageBtn::Num(20)
            ]
        );
    }

    #[test]
    fn thousands_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(12365), "12,365");
        assert_eq!(thousands(1234567), "1,234,567");
    }

    #[test]
    fn pager_uses_local_count_on_installed_tab() {
        let mut st = PiExtState::default();
        // 未安装：全局 total 折算，并截在 MAX_PAGES
        st.total = 12_365;
        st.page = 2;
        assert_eq!(st.pager(), (2, MAX_PAGES));
        // 已安装：本地全量、不分页 → 恒定 (1, 1)
        st.filter = ExtFilter::Installed;
        assert_eq!(st.pager(), (1, 1));
    }

    #[test]
    fn total_pages_caps_at_max() {
        let mut st = PiExtState::default();
        st.total = 12_365;
        assert_eq!(st.total_pages(), MAX_PAGES);
        st.total = PAGE_SIZE * 3;
        assert_eq!(st.total_pages(), 3);
        st.total = 0;
        assert_eq!(st.total_pages(), 1);
    }
}
