//! 应用壳的界面状态类型簇（P3 自 main.rs 拆出）：弹窗/面板/内容区枚举、
//! psp 分组与悬浮卡、composer 菜单与 @ 索引状态、pill 弹层三件套、
//! 内容区 tab（PanelTab/FileTab）。全量 pub(crate)：crate 内自由可达。

use gpui::*;
use crate::*;

/// 扩展页后台 CLI op（040）：转圈落点不同——Install 在安装按钮，
/// Remove 在对应包的垃圾桶上；op 泵收到完成消息即清零。
pub(crate) enum PkgOp {
    Install,
    Remove(String),
}

#[derive(Debug, Clone)]
pub(crate) enum Dialog {
    /// 042：新增 Provider 清单弹窗（两组：自定义 / API KEY；订阅组缓行）
    ProviderPicker { input: gpui::Entity<TextInput>, scroll: gpui::ScrollHandle },
    GitDiff { path: PathBuf, patch: String },
    SessionSearch { input: gpui::Entity<TextInput> },
    /// 004 projectManager 打开项目菜单：搜索框 + 打开文件夹 + 最近 30 天
    /// 项目列表（列表数据在 `project_hits`，扫描完异步回填）。`fresh` =
    /// 新会话页来源：选项目落**全新草稿**（new_session_in，不恢复
    /// last_open）；psp 来源保持切项目恢复上次会话的既定行为。`scroll`
    /// 挂列表自绘滚动条（须跨帧复用，vlist.rs「血案」注释）。
    ProjectPicker {
        input: gpui::Entity<TextInput>,
        fresh: bool,
        scroll: gpui::ScrollHandle,
    },
    /// composer 缩略图点击大图预览：直接持渲染源（Arc 指针拷贝，无索引
    /// 失效问题）
    ImagePreview { image: std::sync::Arc<gpui::Image> },
    /// topbar ⋯ 菜单：系统提示词 / 工具定义（窗体 = 设置弹窗那套大卡片，
    /// 见 top_panels / ui::overlay::big_card）
    SessionInfo { kind: TopPanel },
    /// 023 fileView：关闭带未保存修改的文件 tab 前确认。
    /// 持 path 不持 ix——弹窗存活期间的增删不会让索引漂移。
    FileDirty { path: PathBuf },
    /// 060 远程控制：扫码弹窗（二维码 + 绑定码）。topbar 手机图标与设置
    /// 「远程控制」页签入口已隐藏（2026-10-10 定），自动化 WX_QR_OPEN 仍可打开。
    /// 不持数据 —— 内容全从 `chat.remote.qr` 现读，泵更新即重绘。
    WxQr,
    /// 021 文件树操作栏「搜索文件」：500×500 居中窗，按文件名过滤当前项目
    /// 清单（load_listing 现扫，命中在 `file_search_hits`）。`scroll` 挂
    /// 结果列表自绘滚动条（跨帧复用，vlist.rs「血案」注释）。
    FileSearch {
        input: gpui::Entity<TextInput>,
        scroll: gpui::ScrollHandle,
    },
}

/// ⋯ 菜单的两个面：系统提示词原文 / 已加载工具的声明。各自画在设置弹窗
/// 同款窗体里（`Dialog::SessionInfo`），数据来自 transcript 重放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopPanel {
    System,
    Tools,
}

/// functionPanel active view (statusbar 三 tab): mutually exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DockPanel {
    Sessions,
    Files,
    Git,
}

impl DockPanel {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            DockPanel::Sessions => "sessions",
            DockPanel::Files => "files",
            DockPanel::Git => "git",
        }
    }
}

/// 内容区视图状态机 (v54): chat 默认；terminal / markdown 预览以 topbar tab 打开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentView {
    Chat,
    Term,
    /// 文件查看 tab（PanelTab::File；md/html 渲染、其余源码）
    File,
    /// 081 更新日志 tab（PanelTab::Changelog；markdown 渲染）
    Changelog,
}

/// psp 列表方式（⋯ 菜单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListMode {
    Grouped,
    Flat,
}

/// psp 排序方式（⋯ 菜单；手动=初始序，真拖拽待实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortMode {
    Time,
    Manual,
}

/// One project group of the psp (sessions sorted mtime desc at build).
#[derive(Debug, Clone)]
pub(crate) struct ProjectGroup {
    pub name: String,
    pub path: PathBuf,
    pub sessions: Vec<SessionInfo>,
}

/// 004 打开项目菜单的一行：项目名（目录名）+ 路径。
#[derive(Debug, Clone)]
pub(crate) struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
}

/// 会话 hover 详情卡状态（300ms 离行宽限 + 进卡取消隐藏）。
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

/// psp 菜单（⋯ 排序两级 / 项目菜单）；(x, y) 为事件坐标锚点。
#[derive(Debug, Clone)]
pub(crate) enum PspMenu {
    /// sub: 0=列表方式, 1=排序方式
    Sort { sub: Option<u8>, x: f32, y: f32 },
    Project { path: PathBuf, x: f32, y: f32 },
}

/// 021 文件树右键菜单（目录/文件两套条目）；(x, y) 为事件坐标锚点。
#[derive(Debug, Clone)]
pub(crate) enum TreeMenu {
    Dir { path: PathBuf, x: f32, y: f32 },
    File { path: PathBuf, x: f32, y: f32 },
}

/// 021 行内新建/改名（Zed project_panel `EditState` 移植）：面板同一时刻
/// 至多一个行内编辑器——Enter 提交 / Esc 取消 / 点击别处（面板 mouse-down
/// 或焦点迁走）按 Zed 语义收场。`is_dir` 可被提交时的尾随 `/` 升级（仅
/// 新建，Zed filename_indicates_dir 同款）。
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

#[derive(Debug, Clone)]
pub(crate) struct AttachedImage {
    pub(crate) data_b64: String,
    pub(crate) mime: String,
    /// 缩略图渲染源（附加时预构建）：img() 按 Image id 缓存解码，避免
    /// 逐帧重哈希/重解码大图
    pub(crate) thumb: Option<std::sync::Arc<gpui::Image>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MenuKind {
    Slash,
    At,
}

#[derive(Debug, Clone)]
pub(crate) struct MenuItem {
    pub(crate) insert: String,
    pub(crate) desc: String,
    /// @ 菜单条目的目录形态（决定插入文本 `@dir/` 不闭合 + 图标）
    pub(crate) is_dir: bool,
}

/// @ 文件索引的每-cwd 缓存条目（pi-web file-index route.ts cache parity：
/// 后台构建、每 cwd 一份；TTL/容量可配置——services::workspace 的
/// `at_index_ttl_secs`（默认 60s）/ `at_index_max_projects`（默认 10））。
/// `files` = git ls-files 原始清单；`entries` = 派生条目（目录 + 文件，
/// 浅层优先）——菜单打分输入。
#[derive(Clone, Default)]
pub(crate) struct AtIndexState {
    pub(crate) built: Option<std::time::Instant>,
    pub(crate) files: std::sync::Arc<Vec<String>>,
    pub(crate) entries: std::sync::Arc<Vec<crate::services::at_file::FileEntry>>,
    pub(crate) building: bool,
}

/// @ 菜单过滤记忆化条目：entries 的 Arc 身份 + 查询词 + 上次过滤结果。
/// 索引重建整体换新 Arc → `Arc::ptr_eq` 失配自然失效，无需手动清。
pub(crate) type AtFilterCache = (
    std::sync::Arc<Vec<crate::services::at_file::FileEntry>>,
    String,
    Vec<crate::services::at_file::FileEntry>,
);

/// 042：provider 通路探查状态（保存 key 自动一次 + 详情页【重新探查】）。

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PillMenu {
    Thinking,
    Tools,
}

/// inputpanel 弹窗的统一定位锚点（用户定稿）：弹窗与触发按钮**水平居中**、
/// 底缘距按钮**顶缘 5px**（不遮按钮）。坐标来自按钮 paint 期的 canvas 捕获。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PillAnchor {
    pub center_x: gpui::Pixels,
    pub top: gpui::Pixels,
}

/// 三个胶囊按钮的 bounds 槽位：canvas 在 paint 期每帧覆盖写入，弹窗
/// 打开时读取。Rc<RefCell> 是为了绕开 paint 闭包拿不到实体借用的限制。
#[derive(Clone, Default)]
pub(crate) struct PillBtns {
    pub tools: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub thinking: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub ext: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// MCP 按钮（043：扩展按钮右边的勾选菜单入口）
    pub mcp: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// 模型按钮（042：模型菜单锚定）
    pub model: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
}

impl PillBtns {
    /// 从槽位取锚点；未就绪则退回点击坐标（首帧竞态兜底）。
    pub(crate) fn anchor(
        slot: &std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
        at: gpui::Point<gpui::Pixels>,
    ) -> PillAnchor {
        slot.borrow()
            .map(|b| PillAnchor { center_x: b.center().x, top: b.origin.y })
            .unwrap_or(PillAnchor { center_x: at.x, top: at.y })
    }

    /// 抓按钮 bounds 的隐形覆盖层：absolute inset_0 的 canvas，prepaint 期
    /// 把整颗按钮的 bounds 写进槽位（弹窗统一定位的测量端）。
    pub(crate) fn tracker(
        slot: &std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    ) -> gpui::AnyElement {
        let capture = slot.clone();
        gpui::canvas(
            move |b, _, _| *capture.borrow_mut() = Some(b),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
}

/// One content-area tab: a terminal session, a file viewer, or the changelog.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PanelTab {
    Term(usize),
    File(PathBuf),
    /// 081 更新日志（新版本首启自动打开；无外部资源，纯渲染）
    Changelog,
}

impl PanelTab {
    /// 该 tab 对应的内容区视图（切换/关闭回退共用）。
    pub(crate) fn view(&self) -> ContentView {
        match self {
            PanelTab::Term(_) => ContentView::Term,
            PanelTab::File(_) => ContentView::File,
            PanelTab::Changelog => ContentView::Changelog,
        }
    }
}

/// 外部改动冲突类型（023）：文件在磁盘上被改/删，而本缓冲区有未保存修改。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileConflict {
    /// 磁盘内容已变；banner 提供重新加载
    Changed,
    /// 文件已从磁盘消失（zed DiskState::Deleted 同构，2026-10-11 改版）：
    /// 缓冲保留可继续编辑、tab 标题删除线、不弹横幅；手动保存即重建
    /// （含父目录），自动保存对它保持沉默（防误重建）
    Deleted,
}
// 023 文件编辑展示页：所有文件都进 CodeEditor（Zed parity——Zed 里不分
// 文件大小、一律可编辑）。曾按 50K 行上限加过「超限转只读虚拟化列表」的
// 特殊分支，已删除：特殊路径既不是 Zed 行为，也让大文件不可编辑。
/// 文件 tab 缓冲区状态（023 文件编辑展示页）。
///
/// `content` 是磁盘真值缓存（打开/保存/重载时更新）；`editor` 懒创建——
/// `InputState::new` 要 `&mut Window`，而 `open_file_tab` 的调用链没有，
/// 推迟到渲染帧（content.rs file_view）里补。
pub(crate) struct FileTab {
    pub(crate) content: String,
    pub(crate) editor: Option<gpui::Entity<gpui_component::input::InputState>>,
    /// 编辑器值 != content（订阅 InputEvent::Change 时比较，set_value 也发
    /// Change 事件，盲标会假脏）
    pub(crate) dirty: bool,
    /// md 默认渲染预览；eye 切源码编辑（拍板 2026-10-07）
    pub(crate) md_source: bool,
    pub(crate) conflict: Option<FileConflict>,
    /// 外部改动检测基准 (mtime, len)；打开/保存/确认时刷新
    pub(crate) disk_sig: Option<(std::time::SystemTime, u64)>,
    /// 磁盘内容已换新（自动重载路径），待渲染帧灌进 editor
    pub(crate) reload_pending: bool,
    /// 最后一次用户编辑时刻（自动保存静默期判定）
    pub(crate) last_edit: Option<std::time::Instant>,
}


impl FileTab {
    pub(crate) fn from_disk(content: String) -> Self {
        Self {
            content,
            editor: None,
            dirty: false,
            // 024 用户拍板（2026-10-10 二改）：md 默认打开 = 源码态（WYSIWYG
            // 感官完整：折叠/样式/容器 reveal/原子跳词），eye 切回 preview
            // 渲染页（两者均完好）。
            md_source: true,
            conflict: None,
            disk_sig: None,
            reload_pending: false,
            last_edit: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum McProbe {
    Running,
    Done,
}
