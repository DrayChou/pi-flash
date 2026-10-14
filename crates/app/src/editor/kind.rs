//! FileTypeHandler 注册表（editor 架构核心，2026-10-12 拍板）：**按后缀名
//! 注册 handler，未注册一律走 [`TextKind`] 默认实例**。类型相关决策全部收
//! 进 [`FileKind`] 实现，调用点（打开路由/编辑器配置/主体渲染/工具栏/右键
//! 菜单）零 if-else——新增文件类型 = 声明一个 kind 静态量 + 注册表登记一
//! 行，框架不动。
//!
//! 职责边界：
//! - **handler 管**：CodeEditor 配置（高亮语言/软换行/图片粘贴钩子；None
//!   = 无编辑器类型）、主体渲染算法、工具栏按钮（默认仅搜索；md 追加预览
//!   eye）、右键菜单文件动作块（默认骨架 = 资源管理器/终端 + 改名/删除）、
//!   是否有源码/预览双态（file.view_mode op 门控）
//! - **共享基建不管类型**：文本缓冲/脏位/自动保存/外部改动检测/冲突横幅
//!   （tabs.rs）、面包屑/搜索面板/焦点（view.rs 宿主）——对一切已打开
//!   tab 统一生效
//!
//! 与旧实现的分野：txt 与 code 是两个 kind（不再靠 ts_language 回退
//! "text" 混在一起）；html/htm 与 xml/vue 共用一个 CodeKind（ts_lang
//! "html"）——「在浏览器中打开」是**路径判据**（[`browser_openable`]）
//! 而不是一个专门 kind，编辑器右键菜单与文件树菜单（021）共用同一处
//! 判断；`csv` 等未注册后缀自然落 TextKind（将来表格视图 = 纯增量注册
//! CsvKind）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use gpui::{AnyElement, Context};

use crate::Chat;

/// CodeEditor 配置（由 [`FileKind::editor_config`] 产出，view.rs 消费）。
#[derive(Clone, Copy)]
pub(crate) struct EditorConfig {
    /// gpui-component tree-sitter 语言名
    pub ts_lang: &'static str,
    /// 软换行（2026-10-14 用户定夺：所有 kind 恒开——6bc1f2f 口径「预览和
    /// 编辑状态都要回行」；此前 md 关是与 WYSIWYG 连坐误关。唯一的例外态：
    /// WYSIWYG 装饰挂上期间折行必须关（折叠与折行结构互斥，见
    /// markdown::sync_live_decorations——耦合点收在那一个函数里）
    pub soft_wrap: bool,
    /// 剪贴板图片粘贴落盘钩子（025，仅 md）
    pub image_paste: bool,
}

/// 导航操作栏右侧按钮（023：默认仅搜索；md 追加预览切换）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ToolButton {
    /// 源码/渲染双态切换（md；fv-eye）
    Eye,
    /// 内置搜索替换弹层（fv-search）
    Search,
}

/// 右键菜单文件动作块条目（编辑四项复制/粘贴/剪切/全选全类型共有，不在
/// 本表——由 psp_overlays 的编辑块固定渲染）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MenuAction {
    /// 预览：双态类型源码态专属，点击切回渲染态（edm-preview）
    PreviewToggle,
    /// 在资源管理器中打开（edm-explorer）
    Explorer,
    /// 在终端中打开所在目录（edm-term）
    Terminal,
    /// 分隔线
    Sep,
    /// 改名（edm-rename）
    Rename,
    /// 删除到回收站（edm-del）
    Delete,
}

/// 默认文件动作块：资源管理器/终端 + 分隔 + 改名/删除（023 菜单骨架）。
const DEFAULT_FILE_ACTIONS: &[MenuAction] = &[
    MenuAction::Explorer,
    MenuAction::Terminal,
    MenuAction::Sep,
    MenuAction::Rename,
    MenuAction::Delete,
];

/// 主体渲染上下文：壳（view.rs file_editor_body）解析好后传给 handler，
/// 渲染真值（脏→编辑器值）等类型相关解析由各 handler 自取。
pub(crate) struct BodyCtx<'a> {
    pub chat: &'a mut Chat,
    pub weak: &'a gpui::WeakEntity<Chat>,
    pub path: &'a Path,
    /// 当前视图态（FileTab.md_source：true = 源码态）；仅双态类型消费
    pub source_mode: bool,
}

/// 文件类型 handler。对象安全（&'static dyn 进注册表）。
pub(crate) trait FileKind: Sync {
    /// 注册表调试/测试用名（非 test 构建无消费方，标识保留）
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// CodeEditor 配置；None = 该类型无编辑器（图片查看器等）
    fn editor_config(&self) -> Option<EditorConfig> {
        None
    }

    /// 源码/预览双态（eye 按钮 + edm-preview 菜单项 + file.view_mode op
    /// 的门控）
    fn supports_view_mode(&self) -> bool {
        false
    }

    /// 导航操作栏右侧按钮（023：默认仅搜索）
    fn toolbar(&self) -> &'static [ToolButton] {
        &[ToolButton::Search]
    }

    /// 右键菜单文件动作块（含分隔线，渲染器按序出条目）。
    /// `source_mode`：双态类型据此决定是否带 PreviewToggle。
    fn menu_file_actions(&self, _source_mode: bool) -> &'static [MenuAction] {
        DEFAULT_FILE_ACTIONS
    }

    /// 主体渲染算法（内容区编辑展示区）。
    fn render_body(&self, cx: &mut Context<Chat>, body: BodyCtx<'_>) -> AnyElement;
}

// ---------------------------------------------------------------------------
// 具体类型
// ---------------------------------------------------------------------------

/// 纯文本（默认 kind，注册表未命中一律落它）：ts "text" 纯色、软换行开。
struct TextKind;

/// 代码文件：tree-sitter 高亮语言为实例数据；后缀 → 语言映射即注册条目，
/// 未注册语言（rb/sql 因 vendored 裁剪）不进表 = 自然走 TextKind。
struct CodeKind {
    ts_lang: &'static str,
}

/// Markdown：双态（默认源码态 WYSIWYG，024 二改拍板；eye 切渲染预览）、
/// 图片粘贴插图钩子（025）。软换行开（stock 路径；仅 WYSIWYG 装饰挂上
/// 期间由 sync_live_decorations 关——折叠与折行互斥）。
struct MarkdownKind;

/// 图片：gpui img() 查看器，无编辑器、只读。
struct ImageKind {
    format: gpui::ImageFormat,
}

impl FileKind for TextKind {
    fn name(&self) -> &'static str {
        "text"
    }
    fn editor_config(&self) -> Option<EditorConfig> {
        Some(EditorConfig { ts_lang: "text", soft_wrap: true, image_paste: false })
    }
    fn render_body(&self, cx: &mut Context<Chat>, body: BodyCtx<'_>) -> AnyElement {
        super::view::render_editor_surface(body.chat, body.weak, body.path, cx)
    }
}

impl FileKind for CodeKind {
    fn name(&self) -> &'static str {
        "code"
    }
    fn editor_config(&self) -> Option<EditorConfig> {
        Some(EditorConfig { ts_lang: self.ts_lang, soft_wrap: true, image_paste: false })
    }
    fn render_body(&self, cx: &mut Context<Chat>, body: BodyCtx<'_>) -> AnyElement {
        super::view::render_editor_surface(body.chat, body.weak, body.path, cx)
    }
}

impl FileKind for MarkdownKind {
    fn name(&self) -> &'static str {
        "markdown"
    }
    fn editor_config(&self) -> Option<EditorConfig> {
        Some(EditorConfig { ts_lang: "markdown", soft_wrap: true, image_paste: true })
    }
    fn supports_view_mode(&self) -> bool {
        true
    }
    fn toolbar(&self) -> &'static [ToolButton] {
        &[ToolButton::Eye, ToolButton::Search]
    }
    fn menu_file_actions(&self, source_mode: bool) -> &'static [MenuAction] {
        const SOURCE: &[MenuAction] = &[
            MenuAction::PreviewToggle,
            MenuAction::Explorer,
            MenuAction::Terminal,
            MenuAction::Sep,
            MenuAction::Rename,
            MenuAction::Delete,
        ];
        if source_mode {
            SOURCE
        } else {
            DEFAULT_FILE_ACTIONS
        }
    }
    fn render_body(&self, cx: &mut Context<Chat>, body: BodyCtx<'_>) -> AnyElement {
        // 源码态装饰幂等对账（两态都要跑：切 preview 时摘 provider）
        super::markdown::sync_live_decorations(body.chat, body.path, body.source_mode, cx);
        if body.source_mode {
            super::view::render_editor_surface(body.chat, body.weak, body.path, cx)
        } else {
            super::markdown::render_preview_body(body.chat, body.weak, body.path, cx)
        }
    }
}

impl FileKind for ImageKind {
    fn name(&self) -> &'static str {
        "image"
    }
    fn render_body(&self, _cx: &mut Context<Chat>, body: BodyCtx<'_>) -> AnyElement {
        super::view::render_image_surface(body.path, self.format)
    }
}

// ---------------------------------------------------------------------------
// 注册表：后缀名 → handler（未命中 → TextKind）
// ---------------------------------------------------------------------------

static TEXT: TextKind = TextKind;
static MARKDOWN: MarkdownKind = MarkdownKind;
static IMG_PNG: ImageKind = ImageKind { format: gpui::ImageFormat::Png };
static IMG_JPEG: ImageKind = ImageKind { format: gpui::ImageFormat::Jpeg };
static IMG_GIF: ImageKind = ImageKind { format: gpui::ImageFormat::Gif };
static IMG_WEBP: ImageKind = ImageKind { format: gpui::ImageFormat::Webp };
static IMG_BMP: ImageKind = ImageKind { format: gpui::ImageFormat::Bmp };
static IMG_SVG: ImageKind = ImageKind { format: gpui::ImageFormat::Svg };

static RUST: CodeKind = CodeKind { ts_lang: "rust" };
static PYTHON: CodeKind = CodeKind { ts_lang: "python" };
static JAVASCRIPT: CodeKind = CodeKind { ts_lang: "javascript" };
static TYPESCRIPT: CodeKind = CodeKind { ts_lang: "typescript" };
static TSX: CodeKind = CodeKind { ts_lang: "tsx" };
static GO: CodeKind = CodeKind { ts_lang: "go" };
static JSON: CodeKind = CodeKind { ts_lang: "json" };
/// 树状标记语言共用实例（html/htm/xml/vue → ts_lang "html"）：后三者的高亮
/// 语言随 html，且都不享浏览器菜单（那是 [`browser_openable`] 的判据）
static MARKUP: CodeKind = CodeKind { ts_lang: "html" };
static CSS: CodeKind = CodeKind { ts_lang: "css" };
static TOML: CodeKind = CodeKind { ts_lang: "toml" };
static YAML: CodeKind = CodeKind { ts_lang: "yaml" };
static C: CodeKind = CodeKind { ts_lang: "c" };
static CPP: CodeKind = CodeKind { ts_lang: "cpp" };
static CSHARP: CodeKind = CodeKind { ts_lang: "csharp" };
static JAVA: CodeKind = CodeKind { ts_lang: "java" };
static SWIFT: CodeKind = CodeKind { ts_lang: "swift" };
static ZIG: CodeKind = CodeKind { ts_lang: "zig" };
static BASH: CodeKind = CodeKind { ts_lang: "bash" };
static CMAKE: CodeKind = CodeKind { ts_lang: "cmake" };
static PROTO: CodeKind = CodeKind { ts_lang: "proto" };
static DIFF: CodeKind = CodeKind { ts_lang: "diff" };
static ELIXIR: CodeKind = CodeKind { ts_lang: "elixir" };
static GRAPHQL: CodeKind = CodeKind { ts_lang: "graphql" };
static SCALA: CodeKind = CodeKind { ts_lang: "scala" };
static MAKE: CodeKind = CodeKind { ts_lang: "make" };

fn registry() -> &'static HashMap<&'static str, &'static dyn FileKind> {
    static REG: OnceLock<HashMap<&'static str, &'static dyn FileKind>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut m: HashMap<&'static str, &'static dyn FileKind> = HashMap::new();
        let mut put = |exts: &[&'static str], k: &'static dyn FileKind| {
            for e in exts {
                m.insert(*e, k);
            }
        };
        put(&["md", "markdown"], &MARKDOWN);
        put(&["png"], &IMG_PNG);
        put(&["jpg", "jpeg"], &IMG_JPEG);
        put(&["gif"], &IMG_GIF);
        put(&["webp"], &IMG_WEBP);
        put(&["bmp"], &IMG_BMP);
        put(&["svg"], &IMG_SVG);
        put(&["rs"], &RUST);
        put(&["py", "pyw"], &PYTHON);
        put(&["js", "jsx", "mjs", "cjs"], &JAVASCRIPT);
        put(&["ts", "mts", "cts"], &TYPESCRIPT);
        put(&["tsx"], &TSX);
        put(&["go"], &GO);
        put(&["json", "jsonc"], &JSON);
        // html/htm/xml/vue：树状标记语言共用一个 CodeKind（源码进编辑器，
        // tree-sitter html 高亮；html 二改拍板起不再旁路系统浏览器）。
        // 「在浏览器中打开」不是 kind 差异 → [`browser_openable`] 路径判据
        put(&["html", "htm", "xml", "vue"], &MARKUP);
        put(&["css", "scss"], &CSS);
        put(&["toml"], &TOML);
        put(&["yaml", "yml"], &YAML);
        put(&["c", "h"], &C);
        put(&["cpp", "cc", "cxx", "hpp", "hxx"], &CPP);
        put(&["cs"], &CSHARP);
        put(&["java"], &JAVA);
        put(&["swift"], &SWIFT);
        put(&["zig"], &ZIG);
        put(&["sh", "bash", "zsh"], &BASH);
        put(&["cmake"], &CMAKE);
        put(&["proto"], &PROTO);
        put(&["diff", "patch"], &DIFF);
        put(&["ex"], &ELIXIR);
        put(&["graphql", "gql"], &GRAPHQL);
        put(&["scala"], &SCALA);
        put(&["mk"], &MAKE);
        m
    })
}

/// 后缀名查表（大小写不敏感）；未注册/无后缀 → TextKind 默认实例。
pub(crate) fn kind_of(path: &Path) -> &'static dyn FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref().and_then(|e| registry().get(e)) {
        Some(&k) => k,
        None => &TEXT,
    }
}

/// 「在浏览器中打开」适用判据（html/htm）：编辑器右键菜单（edm-browser，
/// 文件动作块首位）与文件树菜单（ftm-file-browser，021）共用同一处判断，
/// 两处扩展名规则不再各写一份（漂移过：树菜单认 html 而编辑器菜单不动）。
/// 判据是路径而非 kind——html/htm 与 xml/vue 同用一个 CodeKind，xml 不开
/// 浏览器。
pub(crate) fn browser_openable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn kind(ext: &str) -> &'static dyn FileKind {
        kind_of(&PathBuf::from(format!("x.{ext}")))
    }

    /// 后缀 → kind 路由（原 ts_language 映射表的注册表化断言）。
    #[test]
    fn ext_routing() {
        assert_eq!(kind("rs").editor_config().unwrap().ts_lang, "rust");
        assert_eq!(kind("pyw").editor_config().unwrap().ts_lang, "python");
        assert_eq!(kind("tsx").editor_config().unwrap().ts_lang, "tsx");
        assert_eq!(kind("toml").editor_config().unwrap().ts_lang, "toml");
        assert_eq!(kind("cpp").editor_config().unwrap().ts_lang, "cpp");
        assert_eq!(kind("hpp").editor_config().unwrap().ts_lang, "cpp");
        assert_eq!(kind("yml").editor_config().unwrap().ts_lang, "yaml");
        // 大小写不敏感（后缀统一 lower 进表）
        assert_eq!(kind("MD").name(), "markdown");
        assert_eq!(kind("RS").editor_config().unwrap().ts_lang, "rust");
        // 无后缀 / 未注册 → 默认 TextKind（csv 走 default，行为同今天）
        assert_eq!(kind_of(&PathBuf::from("README")).name(), "text");
        assert_eq!(kind("csv").name(), "text");
        assert_eq!(kind("unknownext").name(), "text");
        // vendored 裁剪的语言（rb/sql）保持回退纯色文本
        assert_eq!(kind("rb").name(), "text");
        assert_eq!(kind("sql").name(), "text");
    }

    /// html：源码进编辑器（2026-10-12 二改拍板），且**并入代码类处理器**——
    /// 与 xml/vue 同一个 CodeKind（ts_lang "html"）。浏览器入口不再是 kind
    /// 差异，而是路径判据（`browser_openable`，见下个测试）。
    #[test]
    fn html_opens_as_source() {
        assert_eq!(kind("html").name(), "code");
        assert_eq!(kind("htm").name(), "code");
        assert_eq!(kind("xml").name(), "code");
        let cfg = kind("html").editor_config().unwrap();
        assert_eq!(cfg.ts_lang, "html");
        assert!(cfg.soft_wrap);
        assert!(!kind("html").supports_view_mode());
        assert_eq!(kind("html").toolbar(), &[ToolButton::Search]);
        // 文件动作块 = 默认骨架（浏览器项由 browser_openable 打头渲染）
        assert_eq!(kind("html").menu_file_actions(true), DEFAULT_FILE_ACTIONS);
    }

    /// 浏览器打开判据：只认 html/htm（大小写不敏感）；xml 同 kind 但不开。
    #[test]
    fn browser_openable_only_pages() {
        assert!(browser_openable(Path::new("a.html")));
        assert!(browser_openable(Path::new("A.HTM")));
        assert!(!browser_openable(Path::new("a.xml")));
        assert!(!browser_openable(Path::new("a.md")));
        assert!(!browser_openable(Path::new("README")));
    }

    /// txt 与 code 分离：两者都是编辑器型，但高亮与身份不同。
    #[test]
    fn text_vs_code_distinct() {
        assert_eq!(kind("txt").name(), "text");
        assert_eq!(kind("rs").name(), "code");
        assert_eq!(kind("txt").editor_config().unwrap().ts_lang, "text");
    }

    /// 编辑器配置：全部 kind 软换行开（md 的关折行只存在于 WYSIWYG 装饰
    /// 激活期间，由 sync_live_decorations 运行时收口）；md 带图片粘贴钩子；
    /// 图片无编辑器。
    #[test]
    fn editor_configs() {
        let md = kind("md").editor_config().unwrap();
        assert_eq!(md.ts_lang, "markdown");
        assert!(md.soft_wrap);
        assert!(md.image_paste);
        let txt = kind("txt").editor_config().unwrap();
        assert!(txt.soft_wrap);
        assert!(!txt.image_paste);
        assert!(kind("png").editor_config().is_none());
    }

    /// 工具栏：默认只有搜索；仅 markdown 追加预览切换（023/024）。
    #[test]
    fn toolbars() {
        let default_btns = kind("txt").toolbar();
        assert_eq!(default_btns, &[ToolButton::Search]);
        assert_eq!(kind("rs").toolbar(), &[ToolButton::Search]);
        assert_eq!(kind("md").toolbar(), &[ToolButton::Eye, ToolButton::Search]);
    }

    /// 右键菜单文件动作块：默认骨架；md 源码态带预览项（浏览器项是路径
    /// 判据，不在本表）。
    #[test]
    fn menus() {
        assert_eq!(
            kind("txt").menu_file_actions(true),
            DEFAULT_FILE_ACTIONS
        );
        let md_src = kind("md").menu_file_actions(true);
        assert_eq!(md_src[0], MenuAction::PreviewToggle);
        assert_eq!(kind("md").menu_file_actions(false), DEFAULT_FILE_ACTIONS);
        assert_eq!(kind("html").menu_file_actions(true), DEFAULT_FILE_ACTIONS);
    }

    /// 双态：仅 markdown 支持 view_mode（file.view_mode op 门控）。
    #[test]
    fn view_mode_support() {
        assert!(kind("md").supports_view_mode());
        assert!(!kind("txt").supports_view_mode());
        assert!(!kind("rs").supports_view_mode());
        assert!(!kind("png").supports_view_mode());
    }

    /// 图片格式实例：后缀 → gpui ImageFormat。
    #[test]
    fn image_formats() {
        assert_eq!(kind("png").name(), "image");
        assert_eq!(kind("jpeg").name(), "image");
        assert_eq!(kind("webp").name(), "image");
    }
}
