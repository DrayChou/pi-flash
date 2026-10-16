//! i18n (pi-flash-04k): three languages — zh-CN (source strings, identity),
//! zh-TW, en. `t()` maps a source literal through the table; `tf()` renders
//! `{key}` templates. The language index is runtime-switchable and persisted
//! in the workspace memory file (pi-web i18n parity: user preference).

use std::sync::atomic::{AtomicUsize, Ordering};

/// 0 zh-CN · 1 zh-TW · 2 en
static LANG_IX: AtomicUsize = AtomicUsize::new(0);

pub const LANG_LABELS: [&str; 3] = ["简体中文", "繁體中文", "English"];

pub fn set_lang(ix: usize) {
    let ix = ix.min(2);
    LANG_IX.store(ix, Ordering::Relaxed);
    // gpui-component（rust_i18n）词表同步：搜索条按钮文案/tooltip 用组件
    // 自己的 locales/ui.yml（繁体落组件的 zh-HK 桶）
    gpui_component::set_locale(match ix {
        1 => "zh-HK",
        2 => "en",
        _ => "zh-CN",
    });
}

pub fn lang_ix() -> usize {
    LANG_IX.load(Ordering::Relaxed)
}


// ===========================================================================
// 键 = 语义标识 `<域>.<主语>[.<限定>]`，**不再拿中文当键**（2026-10-12 迁移）。
// 旧设计里 zh-CN 源串同时是显示值和查表键，两处必须逐字节相等却没有编译期
// 检查——润色一个字、或新增文案忘了配译文，繁/英界面都会静默回落成中文。
// 语义键让三种语言的文案在表里各自独立：改文案不再动键。
//
// 行内四列：`(key, zh-CN, zh-TW, en)`。**繁/英留空 = 尚未翻译**��此时 tr()
// 回落 zh-CN（与迁移前一致）；漏译清单由 src/i18n_audit.rs 盯着（只减不增）。
//
// 新增文案流程：① i18n_audit 挡住「加了 tr() 却没加表行」；② 表行繁/英空着
// 会被挂进 BACKLOG，补齐后从 BACKLOG 删掉。
// ===========================================================================

/// 关于页信息块：这两个常量现在是**键**，文案在 TABLE 同一行（改文案不必动
/// 键）。about.rs 直接 `tr(ABOUT_*)`。
pub const ABOUT_DESC: &str = "about.tagline";
pub const ABOUT_NOTE: &str = "about.note";

/// (source zh-CN, zh-TW, en)

// (key, zh-CN, zh-TW, en)
const TABLE: &[(&str, &str, &str, &str)] = &[
    // chrome / sidebar
    // compaction card（pi-web CompactionMessageView parity）
    ("compaction.done", "会话已压缩", "會話已壓縮", "Conversation compacted"),
    ("compaction.preset_locked", "运行中不能更换工具预设", "執行中不能更換工具預設", "Cannot change the tool preset while the agent is running"),
    ("compaction.summary_intro", "此处之前的会话历史已压缩为以下摘要：", "此處之前的會話歷史已壓縮為以下摘要：", "The conversation history before this point was compacted into the following summary:"),
    ("compaction.no_summary", "（无摘要）", "（無摘要）", "(no summary)"),
    ("compaction.file_context", "文件上下文：{details}", "檔案脈絡：{details}", "File context: {details}"),
    ("compaction.reads", "{n} 读取", "{n} 讀取", "{n} read"),
    ("compaction.writes", "{n} 修改", "{n} 修改", "{n} modified"),
    ("compaction.modified_files", "修改文件", "修改檔案", "Modified files"),
    ("compaction.read_files", "读取文件", "讀取檔案", "Read files"),
    ("app.name", "pi-flash", "pi-flash", "pi-flash"),
    ("action.new", "新建", "新建", "New"),
    ("session.new", "新会话", "新會話", "New session"),
    // 012 新会话页（newSession）
    ("new_session.greeting", "Hi，打算让我做点什么？", "Hi，打算讓我做點什麼？", "Hi, what shall we work on?"),
    ("session.open_project", "打开项目", "開啟專案", "Open project"),
    ("session.project_switch_hint", "切换后仅显示该项目的会话，并恢复上次打开的会话", "切換後僅顯示該項目的會話，並恢復上次打開的會話", "Show only this project's sessions and restore the last open one"),
    ("session.full_history", "完整历史", "完整歷史", "Full history"),
    ("branch.title", "分支", "分支", "Branches"),
    ("branch.main", "主分支", "主分支", "main"),
    ("git.none", "no git", "no git", "no git"),
    ("session.no_branches", "暂无分支", "暫無分支", "No branches"),
    ("branch.node_hint", "点击节点：从该用户消息处创建分支新会话", "點擊節點：從該用戶消息處創建分支新會話", "Click a node to fork a new session from that user message"),
    ("session.fork", "新分支", "新分支", "Fork"),
    ("session.copied", "已复制", "已複製", "Copied"),
    ("session.creating", "创建中…", "建立中…", "Creating…"),
    ("tool.file_browser", "文件浏览器", "文件瀏覽器", "Files"),
    ("action.compact", "压缩", "壓縮", "Compact"),
    ("action.send", "发送", "發送", "Send"),
    ("composer.placeholder", "消息...输入 / 使用命令，输入 @ 查找文件", "訊息...輸入 / 使用命令，輸入 @ 查找文件", "Message... / for commands, @ for files"),
    ("session.no_active_session", "无活动会话", "無活動會話", "No active session"),
    ("status.disconnected", "未连接", "未連接", "Disconnected"),
    ("session.generate_title", "生成标题", "生成標題", "Generate title"),
    // dialogs
    // 004 打开项目菜单（projectManager）
    ("project.search_placeholder", "搜索项目…", "搜尋項目…", "Search projects…"),
    ("action.open_folder", "打开文件夹", "開啟資料夾", "Open folder"),
    ("project.idle_30d", "最近 30 天没有打开过的项目", "最近 30 天沒有開啟過的項目", "No projects opened in the last 30 days"),
    ("project.no_match", "没有匹配的项目", "沒有符合的項目", "No matching projects"),
    ("action.rename_session", "重命名会话", "重命名會話", "Rename session"),
    ("action.session_name", "session name", "session name", "session name"),
    ("action.cancel", "取消", "取消", "Cancel"),
    ("action.save", "保存", "儲存", "Save"),
    ("action.update", "更新", "更新", "Update"),
    ("action.delete", "删除", "刪除", "Delete"),
    ("action.no", "否", "否", "No"),
    ("action.yes", "是", "是", "Yes"),
    ("action.submit", "提交", "提交", "Submit"),
    // 问卷卡（ask_card.rs，ask_user_question 整卷 tab 卡）
    ("ask.question", "问题", "問題", "Question"),
    ("ask.confirm", "确 认", "確 認", "Confirm"),
    ("ask.cancel_all", "取消整卷", "取消整卷", "Cancel all"),
    ("ask.next", "下一题", "下一題", "Next"),
    ("ask.advance_submit", "跳题/提交", "跳題/提交", "Skip/Submit"),
    ("ask.direct_pick", "直选", "直選", "Pick"),
    ("ask.other_placeholder", "自定义答案…", "自訂答案…", "Custom answer…"),
    ("ask.no_preview", "（无预览）", "（無預覽）", "(no preview)"),
    ("model.select_placeholder", "select model", "select model", "select model"),
    ("model.select_title", "选择模型", "選擇模型", "Select model"),
    ("action.filter_models", "过滤模型...", "過濾模型...", "filter models..."),
    ("action.select_project", "选择项目", "選擇項目", "Select project"),
    ("action.no_models_match", "no models match", "no models match", "no models match"),
    // 系统提示词面板 token 分析块（7 大类）
    ("tokens.total", "总计", "總計", "Total"),
    ("tokens.global_prompt", "全局提示词", "全域提示詞", "Global prompt"),
    ("tokens.system_tools", "系统工具", "系統工具", "System tools"),
    ("tokens.project_prompt", "项目提示词", "專案提示詞", "Project prompt"),
    // settings tabs
    ("settings.tab.models", "模型", "模型", "Models"),
    ("settings.tab.skills", "技能", "技能", "Skills"),
    ("settings.tab.extensions", "扩展", "擴展", "Extensions"),
    ("settings.tab.tools", "工具", "工具", "Tools"),
    ("settings.tab.general", "通用", "通用", "General"),
    // models tab
    ("models.enabled_models", "已启用模型", "已啟用模型", "Enabled models"),
    ("models.enable_all", "全部启用", "全部啟用", "Enable all"),
    ("models.disable_all", "全部停用", "全部停用", "Disable all"),
    ("models.current", "当前", "當前", "current"),
    ("models.api_key", "API Key", "API Key", "API Key"),
    ("models.api_key_configured", "API Key 已配置", "API Key 已配置", "API key configured"),
    ("models.api_key_required", "API Key 不能为空", "API Key 不能為空", "API key must not be empty"),
    ("models.signed_in_with_oauth", "OAuth 已登录", "OAuth 已登入", "Signed in with OAuth"),
    ("models.credentials", "凭据", "憑證", "Credentials"),
    ("models.credentials_hint", "登录凭据存储于 ~/.pi/agent/auth.json（与 pi 共用）", "登入憑證儲存於 ~/.pi/agent/auth.json（與 pi 共用）", "Credentials are stored in ~/.pi/agent/auth.json (shared with pi)"),
    ("action.show", "显示", "顯示", "Show"),
    ("action.hide", "隐藏", "隱藏", "Hide"),
    ("models.api_key_hint", "密钥写入 ~/.pi/agent/auth.json（与 pi 共用）；新 provider 的模型需重启 pi-flash 后出现在列表", "密鑰寫入 ~/.pi/agent/auth.json（與 pi 共用）；新 provider 的模型需重啟 pi-flash 後出現在列表", "Key is written to ~/.pi/agent/auth.json (shared with pi); a new provider's models appear after restarting pi-flash"),
    ("models.not_configured", "未配置", "未配置", "Not configured"),
    ("models.cannot_disable_last", "不能停用最后一个启用的模型", "不能停用最後一個啟用的模型", "Cannot disable the last enabled model"),
    ("settings.write_failed", "写入 settings.json 失败: {e}", "寫入 settings.json 失敗: {e}", "Failed to write settings.json: {e}"),
    ("models.failed_to_save", "保存失败: {e}", "儲存失敗: {e}", "Failed to save: {e}"),
    ("models.project_override_dotpi", "项目级 .pi/settings.json 覆盖了 enabledModels，面板只读", "項目級 .pi/settings.json 覆蓋了 enabledModels，面板只讀", "Project .pi/settings.json overrides enabledModels; panel is read-only"),
    ("models.project_override", "项目级 settings.json 覆盖了 enabledModels，此面板只读", "項目級 settings.json 覆蓋了 enabledModels，此面板只讀", "Project settings.json overrides enabledModels; this panel is read-only"),
    ("models.no_matching_models", "没有匹配的模型", "沒有匹配的模型", "No matching models"),
    // skills tab
    ("skills.no_skills_found", "没有找到技能", "沒有找到技能", "No skills found"),
    ("skills.empty_hint", "没有找到技能（扫描项目 .pi/skills、.agents/skills 与全局目录）", "沒有找到技能（掃描項目 .pi/skills、.agents/skills 與全局目錄）", "No skills found (scans project .pi/skills, .agents/skills and global dirs)"),
    ("skills.visible_to_model", "对模型可见", "對模型可見", "Visible to model"),
    ("skills.hidden_hint", "已隐藏（仍可手动调用）", "已隱藏（仍可手動調用）", "Hidden (still invocable manually)"),
    ("skills.write_failed", "写入 SKILL.md 失败: {e}", "寫入 SKILL.md 失敗: {e}", "Failed to write SKILL.md: {e}"),
    // extensions tab（040）
    ("extensions.install", "安装", "安裝", "Install"),
    ("extensions.installing", "安装中", "安裝中", "Installing…"),
    ("extensions.install_hint", "从 https://pi.dev/packages 复制安装命令，做全局安装", "從 https://pi.dev/packages 複製安裝命令，做全域安裝", "Copy an install command from https://pi.dev/packages and install globally"),
    ("extensions.global_defaults", "配置全局默认扩展", "配置全域預設擴展", "Global default extensions"),
    ("about.description", "说明", "說明", "Description"),
    ("extensions.path", "路径", "路徑", "Path"),
    ("extensions.source", "来源", "來源", "Source"),
    ("scope.global", "全局", "全局", "Global"),
    ("scope.project", "项目", "項目", "Project"),
    ("scope.workspace", "工作区", "工作區", "Workspace"),
    ("scope.builtin", "内置", "內置", "Built-in"),
    ("status.enabled", "已启用", "已啟用", "Enabled"),
    ("status.disabled", "已停用", "已停用", "Disabled"),
    ("status.loaded", "已加载", "已加載", "Loaded"),
    ("extensions.not_configured", "没有已配置的扩展", "沒有已配置的擴展", "No configured extensions"),
    ("extensions.not_installed", "没有已安装的扩展", "沒有已安裝的擴展", "No extensions installed"),
    ("action.remove", "移除", "移除", "Remove"),
    ("extensions.fetch_models", "获取模型", "獲取模型", "Fetch models"),
    ("extensions.fetching", "获取中…", "獲取中…", "Fetching…"),
    ("extensions.remove_key_confirm", "删除 {name} 的 API Key，并不再显示？", "刪除 {name} 的 API Key，並不再顯示？", "Delete the {name} API key and hide this provider?"),
    ("extensions.fetch_key_rejected", "获取失败：key 被拒（HTTP {s}）", "獲取失敗：key 被拒（HTTP {s}）", "Fetch failed: key rejected (HTTP {s})"),
    ("extensions.fetch_failed", "获取失败：{e}", "獲取失敗：{e}", "Fetch failed: {e}"),
    ("action.confirm", "确认", "確認", "Confirm"),
    ("extensions.uninstall", "确认卸载", "確認卸載", "Uninstall"),
    ("extensions.uninstall_confirm", "卸载扩展 {name}？", "卸載擴展 {name}？", "Uninstall extension {name}?"),
    ("extensions.source_placeholder", "请输入扩展来源，或粘贴 pi install 安装命令", "請輸入擴展來源，或貼上 pi install 安裝命令", "Enter a source or paste a pi install command"),
    ("extensions.installed", "已安装 {source}", "已安裝 {source}", "Installed {source}"),
    ("extensions.removed", "已移除 {source}", "已移除 {source}", "Removed {source}"),
    ("extensions.pi_failed", "pi {} 失败: {}", "pi {} 失敗: {}", "pi {} failed: {}"),
    ("extensions.remove_failed", "删除失败: {e}", "刪除失敗: {e}", "Failed to delete: {e}"),
    ("extensions.remove_failed_msg", "删除失败：{msg}", "刪除失敗：{msg}", "Failed to delete: {msg}"),
    ("extensions.ok", "知道了", "知道了", "OK"),
    // tools tab
    ("tools.tool_selection", "工具选择", "工具選擇", "Tool selection"),
    ("tools.all", "全部", "全部", "All"),
    ("tools.default", "默认", "預設", "Default"),
    ("tools.read_only", "只读", "只讀", "Read-only"),
    ("tools.none", "无", "無", "None"),
    ("tools.pi_default_hint", "不覆盖，pi 默认（全部内置工具）", "不覆蓋，pi 預設（全部內置工具）", "No override; pi default (all built-in tools)"),
    ("tools.read_bash_edit_write", "read, bash, edit, write", "read, bash, edit, write", "read, bash, edit, write"),
    ("tools.read_grep_find_ls", "read, grep, find, ls", "read, grep, find, ls", "read, grep, find, ls"),
    ("tools.disable_all_tools", "禁用所有工具", "禁用所有工具", "Disable all tools"),
    ("tools.unset_hint", "未设置（pi 默认解析全部工具）", "未設置（pi 預設解析全部工具）", "Unset (pi resolves every tool)"),
    ("tools.no_tools", "[]（无工具）", "[]（無工具）", "[] (no tools)"),
    ("tools.default_tools_hint", "写入 ~/.pi/agent/settings.json 的 defaultTools；新会话生效（与 pi CLI --tools 一致）", "寫入 ~/.pi/agent/settings.json 的 defaultTools；新會話生效（與 pi CLI --tools 一致）", "Writes defaultTools to ~/.pi/agent/settings.json; applies to new sessions (same as pi CLI --tools)"),
    // 会话统计行
    ("stats.output", "输出", "輸出", "Output"),
    // general tab（v60 界面页）
    ("settings.tab.appearance", "界面", "介面", "Appearance"),
    ("settings.theme", "主题", "主題", "Themes"),
    ("settings.fonts", "字体", "字體", "Fonts"),
    ("settings.language", "语言", "語言", "Language"),
    ("settings.chat_font", "会话字体", "會話字體", "Chat font"),
    ("settings.panel_font", "面板字体", "面板字體", "Panel font"),
    ("settings.file_font", "文档字体", "文档字體", "File font"),
    ("size.small", "小", "小", "Small"),
    ("size.medium", "中", "中", "Medium"),
    ("size.large", "大", "大", "Large"),
    ("size.extra_large", "特大", "特大", "Extra large"),
    ("settings.search_fonts", "搜索字体…", "搜尋字型…", "Search fonts…"),
    ("settings.no_matching_fonts", "无匹配字体", "無符合字型", "No matching fonts"),
    ("settings.notification_sound", "提示音", "提示音", "Notification sound"),
    ("settings.sound_hint", "agent 运行结束播放系统提示音", "agent 執行結束播放系統提示音", "Play a system sound when the agent run finishes"),
    ("settings.load_time_window", "加载时间窗口", "載入時間範圍", "Load time window"),
    ("settings.load_window_hint", "加载最近几天内活跃的会话", "載入最近幾天內活躍的會話", "Load sessions active within the window"),
    ("settings.session_display_count", "会话显示数", "會話顯示數", "Session display count"),
    ("settings.auto_collapse_time", "自动折叠时间", "自動摺疊時間", "Auto-collapse time"),
    ("settings.auto_collapse_hint", "启动时，超时未活动的项目默认折叠", "啟動時，超時未活動的專案預設摺疊", "Collapse projects with no recent activity at startup"),
    ("settings.session_count_hint", "各项目默认显示的会话数量", "各項目預設顯示的會話數量", "Sessions shown per project by default"),
    ("settings.show_more", "显示更多", "顯示更多", "Show more"),
    // session list / relative time templates
    ("session.messages", "{} 条消息", "{} 條消息", "{} messages"),
    // 033 导航面板总结栏
    ("nav.turn_summary", "{n}条消息：用户{u}条 助手{a}条 工具调用{t}次", "{n}條訊息：用户 {u} 條 助理{a}條 工具呼叫{t}次", "{n}messages:{u}user {a} assistant,{t}tool calls"),
    ("time.s_ago", "{secs}秒前", "{secs}秒前", "{secs}s ago"),
    ("time.m_ago", "{n}分钟前", "{n}分鐘前", "{n}m ago"),
    ("time.h_ago", "{n}小时前", "{n}小時前", "{n}h ago"),
    ("time.d_ago", "{n}天前", "{n}天前", "{n}d ago"),
    // misc
    ("value.none", "（无）", "（無）", "(none)"),
    ("auth.sign_out", "退出登录", "退出登入", "Sign out"),
    // 界面页显示开关（自「其他」页挪入，无小节标题——用户定夺）
    ("settings.show_thinking", "展示思考", "展示思考", "Show thinking"),
    ("settings.show_thinking_hint", "思考默认展开或收起", "思考預設展開或收起", "thinking blocks expand by default or collapse to one line"),
    // 081 自动更新（其他页开关 + 新会话页版本号位 + 更新日志 tab）
    ("about.auto_update", "自动更新", "自動更新", "Auto-update"),
    ("settings.auto_update_hint", "允许Pi-Flash自动更新", "允許Pi-Flash自動更新", "Let Pi-Flash update itself automatically"),
    ("about.downloading_v", "下载 {v} 版本中…", "下載 {v} 版本中…", "Downloading v{v}…"),
    ("about.extracting_v", "解压安装中…", "解壓安裝中…", "Extracting & installing…"),
    ("update.ready_click", "下载完成，点击重启更新", "下載完成，點擊重啟更新", "Download complete — click to restart and update"),
    ("about.changelog", "更新日志", "更新日誌", "Changelog"),
    ("about.fetching_changelog", "正在获取更新日志…", "正在獲取更新日誌…", "Fetching changelog…"),
    ("about.no_changelog_available", "暂无更新日志", "暫無更新日誌", "No changelog available"),
    ("about.github_unreachable", "暂时无法连接 GitHub", "暫時無法連線 GitHub", "Can't reach GitHub right now"),
    ("about.about", "关于", "關於", "About"),
    ("about.author", "作者", "作者", "Author"),
    ("about.author_name", "不正常赛博捅讯社", "不正常賽博捅訊社", "不正常赛博捅讯社"),
    ("about.version", "PF版本", "PF 版本", "Version"),
    ("about.kernel", "内核版本", "內核版本", "Kernel"),
    ("about.tagline", "Windows上Pi的极速Shell + 文件管理编辑", "Windows上Pi的極速Shell + 檔案管理編輯", "A lightning-fast Pi Shell for Windows + file management & editing"),
    ("about.note", "Pi-Flash将保持轻巧专注速度体验", "Pi-Flash將保持輕巧專注速度體驗", "The Pi-Flash kernel stays lean and focused on speed"),
    // 文件树 git 标识开关（设置-界面）
    ("settings.file_tree_git_markers", "文件树 Git 标识", "檔案樹 Git 標識", "File tree Git markers"),
    // Markdown 所见即所得开关（设置-界面；024 默认关——回车崩溃待修）
    ("settings.md_live_preview", "Markdown 所见即所得", "Markdown 所見即所得", "Markdown live preview"),
    ("settings.md_live_preview_hint", "编辑 md 时隐藏语法标记，光标进入即显现（实验性）", "編輯 md 時隱藏語法標記，游標進入即顯現（實驗性）", "Hide markdown syntax marks while editing; revealed at the cursor (experimental)"),
    ("settings.git_markers_hint", "文件树显示 git 修改徽标与目录变更点", "檔案樹顯示 git 修改徽標與目錄變更點", "Show git modification badges and directory change dots in the file tree"),
    // 023 fileView（标签栏 + 菜单 / 导航栏 / 冲突横幅 / 确认弹窗）
    ("file_view.pick_in_tree", "在左侧文件树中选择一个文件", "在左側檔案樹中選擇一個檔案", "Pick a file in the tree on the left"),
    ("file_view.open_file", "打开文件…", "開啟檔案…", "Open file…"),
    ("file_view.new_file", "新建文件", "新增檔案", "New file"),
    ("file_view.name_label", "文件名（可含子目录）", "檔案名（可含子目錄）", "File name (subdirs allowed)"),
    ("file_view.create_hint", "在项目根下创建；Enter 确认，Esc 取消", "在專案根下建立；Enter 確認，Esc 取消", "Created under the project root; Enter to confirm, Esc to cancel"),
    ("file_view.too_large", "文件超过 10MB，不打开", "檔案超過 10MB，不開啟", "File exceeds 10MB, not opening"),
    ("file_view.read_failed", "读取失败", "讀取失敗", "Read failed"),
    // 无法打开灰屏（二进制占位 tab，仿 Zed Could not open file）
    ("file_view.unsupported_type", "无法打开 {ext} 类型文件", "無法開啟 {ext} 類型的檔案", "Cannot open {ext} files"),
    ("file_view.binary_unsupported", "二进制文件不受支持", "不支援二進位檔案", "Binary files are not supported"),
    ("file_view.open_in_default_app", "在默认应用中打开", "以預設應用程式開啟", "Open in Default App"),
    ("file_view.save_failed", "保存失败", "儲存失敗", "Save failed"),
    // 051-apikey管理
    ("conflict.configured_vault", "已配置 · 系统凭据库", "已設定 · 系統憑證庫", "Configured · OS vault"),
    ("conflict.configured_file", "已配置 · 文件存储", "已設定 · 檔案儲存", "Configured · file storage"),
    ("conflict.vault_unavailable", "系统凭据库不可用，已降级为文件存储", "系統憑證庫不可用，已降級為檔案儲存", "Credential vault unavailable — fell back to file storage"),
    ("conflict.create_failed", "创建失败", "建立失敗", "Create failed"),
    ("conflict.path_escapes_root", "路径越出项目根", "路徑越出專案根", "Path escapes the project root"),
    ("status.saved", "已保存", "已儲存", "Saved"),
    ("conflict.modified_on_disk", "文件已在磁盘上被修改（本地有未保存修改）", "檔案已在磁碟上被修改（本地有未儲存修改）", "File changed on disk (you have unsaved edits)"),
    ("conflict.deleted_on_disk", "文件已从磁盘消失", "檔案已從磁碟消失", "File disappeared from disk"),
    ("conflict.reload", "重新加载", "重新載入", "Reload"),
    ("conflict.keep_my_version", "保留我的版本", "保留我的版本", "Keep my version"),
    ("conflict.keep_buffer", "保留缓冲", "保留緩衝", "Keep buffer"),
    ("conflict.unsaved_count", "未保存的修改", "未儲存的修改", "Unsaved changes"),
    ("conflict.unsaved_list", "有未保存的修改：", "有未儲存的修改：", "Unsaved changes:"),
    ("conflict.unsaved_n_files", "有未保存的修改：{n} 个文件", "有未儲存的修改：{n} 個檔案", "Unsaved changes in {n} files"),
    ("conflict.close_without_saving", "不保存关闭", "不儲存關閉", "Close without saving"),
    ("conflict.save_and_close", "保存并关闭", "儲存並關閉", "Save and close"),
    // topbar 标签右键菜单（zed tab_bar 菜单移植；无 Close Clean，会话
    // 首标签恒不可关——菜单里没有 Close、Close Left 恒灰）
    ("tab.close", "关闭", "關閉", "Close"),
    ("tab.close_others", "关闭其他标签", "關閉其他標籤", "Close Others"),
    ("tab.close_left", "关闭左侧标签", "關閉左側標籤", "Close Left"),
    ("tab.close_right", "关闭右侧标签", "關閉右側標籤", "Close Right"),
    ("tab.close_all", "关闭全部标签", "關閉全部標籤", "Close All"),
    ("conflict.no_files", "（无文件）", "（無檔案）", "(no files)"),
    ("conflict.initializing_editor", "编辑器初始化中…", "編輯器初始化中…", "Initializing editor…"),
    ("conflict.file_closed", "文件已关闭", "檔案已關閉", "File closed"),
    // 021 文件树操作栏 + 右键菜单
    ("file_tree.reveal_in_explorer", "在资源管理器中打开", "在檔案資源管理器中開啟", "Open in Explorer"),
    ("file_tree.open_in_terminal", "在终端中打开", "在終端機中開啟", "Open in terminal"),
    ("file_tree.open_in_browser", "在浏览器中打开", "在瀏覽器中開啟", "Open in browser"),
    ("file_tree.copy_path", "复制路径", "複製路徑", "Copy path"),
    ("action.copy", "复制", "複製", "Copy"),
    ("action.paste", "粘贴", "貼上", "Paste"),
    ("action.cut", "剪切", "剪下", "Cut"),
    ("action.select_all", "全选", "全選", "Select All"),
    // 031 输入面板右键菜单（用户定案：redo 文案「恢复」）
    ("action.undo", "撤销", "撤銷", "Undo"),
    ("action.redo", "恢复", "恢復", "Redo"),
    ("file_view.preview", "预览", "預覽", "Preview"),
    ("file_tree.insert_at_mention", "@到会话输入框", "@到會話輸入框", "Mention in composer"),
    ("file_tree.rename", "改名", "改名", "Rename"),
    ("file_tree.new_folder", "新建目录", "新增資料夾", "New folder"),
    ("action.rename", "重命名", "重新命名", "Rename"),
    ("field.name", "名称", "名稱", "Name"),
    ("action.create", "创建", "建立", "Create"),
    ("tree.name_required", "名称不能为空", "名稱不能為空", "Name cannot be empty"),
    ("tree.name_has_separator", "名称不能包含路径分隔符", "名稱不能包含路徑分隔符", "Name cannot contain path separators"),
    ("file_tree.already_exists", "已存在", "已存在", "Already exists"),
    ("tree.exists_named", "「{name}」已存在，请换一个名称", "「{name}」已存在，請換一個名稱", "\"{name}\" already exists at this location, please choose a different name"),
    ("tree.name_has_whitespace", "名称首尾包含空白字符", "名稱首尾包含空白字元", "Name contains leading or trailing whitespace"),
    ("tree.name_illegal_segment", "名称不能为空或包含 .. 等非法段", "名稱不能為空或包含 .. 等非法段", "Name cannot be empty or contain invalid segments like \"..\""),
    ("tree.delete_confirm", "删除 {name}？（移入回收站）", "刪除 {name}？（移入回收站）", "Delete {name}? (moves to the recycle bin)"),
    ("tree.delete_confirm_dirty", "删除 {name}？{n} 个打开文件的未保存更改将丢失（移入回收站）", "刪除 {name}？{n} 個開啟檔案的未保存變更將遺失（移至資源回收筒）", "Delete {name}? Unsaved changes in {n} open file(s) will be lost (moves to the recycle bin)"),
    ("file_tree.search_files", "搜索文件…", "搜尋檔案…", "Search files…"),
    ("file_tree.search_hint", "输入文件名关键词搜索当前项目", "輸入檔名關鍵詞搜尋當前專案", "Type a file-name keyword to search the current project"),
    ("file_tree.scanning_files", "正在扫描文件…", "正在掃描檔案…", "Scanning files…"),
    ("file_tree.files", "{n} 个文件", "{n} 個檔案", "{n} files"),
    ("file_tree.no_changes", "暂无变更", "暫無變更", "No changes"),
    ("file_tree.show_changes_only", "只看变更", "只看變更", "Show changes only"),
    ("file_tree.maximize", "最大化文件树", "最大化文件樹", "Maximize file tree"),
    ("file_tree.switch_project", "切换项目", "切換專案", "Switch project"),
    ("tree.delete_root_denied", "不能删除项目根目录", "不能刪除專案根目錄", "Cannot delete the project root"),
    ("file_tree.rename_failed", "重命名失败: {e}", "重新命名失敗: {e}", "Rename failed: {e}"),
    ("file_tree.move_failed", "移动失败: {e}", "移動失敗: {e}", "Move failed: {e}"),
    // 021 Pin 收藏夹
    ("file_tree.pin", "Pin", "Pin", "Pin"),
    ("file_tree.unpin", "取消固定", "取消釘選", "Unpin"),
    ("file_tree.pin_favorites", "Pin 收藏夹", "Pin 收藏夾", "Pin favorites"),
    ("file_tree.pin_remove", "移除", "移除", "Remove"),
    // 会话功能栏（session::session_fn_bar，原 topbar ⋯ 菜单三入口）
    ("session.open_terminal", "打开终端", "開啟終端機", "Open terminal"),
    ("session.system_prompt", "系统提示词", "系統提示詞", "System prompt"),
    ("session.session_tools", "会话工具集", "會話工具集", "Session tools"),
    // top panel：系统提示词（pi-web system.*）
    ("system_prompt.tool_definitions", "工具定义", "工具定義", "Tool definitions"),
    ("system_prompt.empty_disabled", "系统提示词为空（工具已禁用）", "系統提示詞為空（工具已停用）", "System prompt is empty (tools are disabled)"),
    ("system_prompt.loading", "系统提示词加载中…", "系統提示詞載入中…", "Loading system prompt…"),
    // top panel：工具定义（pi-web tools.*）
    ("tool_defs.not_loaded", "工具定义尚未加载", "工具定義尚未載入", "Tool definitions have not loaded yet"),
    ("tool_defs.no_active_tools", "没有启用的工具", "沒有啟用的工具", "No active tools"),
    ("tool_defs.description", "描述", "描述", "Description"),
    ("tool_defs.params", "参数", "參數", "Parameters"),
    ("tool_defs.parameters", "{count} 个参数", "{count} 個參數", "{count} parameters"),
    ("tool_defs.tool_declarations", "调用声明", "調用聲明", "Tool declarations"),
    ("tool_defs.items", "{n} 条", "{n} 條", "{n} items"),
    ("tool_defs.no_parameters", "无参数", "無參數", "No parameters"),
    ("tool_defs.required", "必填", "必填", "Required"),
    ("tool_defs.optional", "可选", "選填", "Optional"),
    ("tool_defs.allowed", "可选值", "允許值", "Allowed"),
    ("tool_defs.default", "默认值", "預設值", "Default"),
    ("tool_defs.model", "模型: {v}", "模型: {v}", "Model: {v}"),
    ("tool_defs.thinking", "思考: {v}", "思考: {v}", "Thinking: {v}"),
    ("tool_defs.max_turns", "最大轮数: {v}", "最大輪數: {v}", "Max turns: {v}"),
    // editor pill menus
    ("thinking.use_pi_default", "使用 pi 默认设置", "使用 pi 預設設置", "Use pi default"),
    ("thinking.off", "关闭推理", "關閉推理", "Reasoning off"),
    ("thinking.minimal_reasoning", "最低限度推理", "最低程度推理", "Minimal reasoning"),
    ("thinking.low_reasoning", "低强度推理", "低強度推理", "Low reasoning"),
    ("thinking.medium_reasoning", "中等强度推理", "中等強度推理", "Medium reasoning"),
    ("thinking.high_reasoning", "高强度推理", "高強度推理", "High reasoning"),
    ("thinking.extra_high_reasoning", "超高强度推理", "超高強度推理", "Extra-high reasoning"),
    ("thinking.max_reasoning", "最高强度推理", "最高強度推理", "Max reasoning"),
    ("tools.from_default_tools", "取自 settings.json 的 defaultTools", "取自 settings.json 的 defaultTools", "From settings.json defaultTools"),
    ("thinking.chat_only", "仅聊天", "僅聊天", "Chat only"),
    ("tools.preset_readonly_count", "4 个只读内置工具", "4 個只讀內置工具", "4 read-only built-in tools"),
    ("tools.preset_full_count", "4 个内置工具", "4 個內置工具", "4 built-in tools"),
    ("thinking.all_built_in_tools", "全部内置工具", "全部內置工具", "All built-in tools"),
    // 034/040 会话扩展清单（2026-10 改版：档名 full+、勾选即生效）
    ("extensions.full", "full+", "full+", "full+"),
    ("tools.preset_full_desc", "full + 自定义扩展", "full + 自訂擴展", "full + custom extensions"),
    ("extensions.choose_extensions", "选择扩展", "選擇擴展", "Choose extensions"),
    ("extensions.builtin_all_desc", "全部内置工具（不装任何扩展）", "全部內置工具（不裝任何擴展）", "All built-in tools (no extensions loaded)"),
    ("extensions.list_next_turn", "扩展清单将在下一轮生效", "擴展清單將在下一輪生效", "The extension list takes effect on the next turn"),
    ("extensions.list_save_failed", "扩展清单保存失败: {e}", "擴展清單儲存失敗: {e}", "Failed to save the extension list: {e}"),
    // 034 扩展按钮 + 勾选菜单
    ("extensions.only_full_preset", "扩展只能在 full+ 档勾选", "擴展只能在 full+ 檔勾選", "Extensions can only be picked in the full+ preset"),
    // 040 说明 token 预览（延迟加载）
    ("tokens.prompt_size", "说明大小", "說明大小", "Prompt size"),
    ("tokens.sum", "合计：", "合計：", "Total:"),
    ("nav.work_details", "工作详情 · {m} 条消息 · {t} 次工具调用", "", ""),
    ("mcp.enabled_count", "已开启 {n}/{total}", "", ""),
    ("models.registry_read_failed", "读取 Provider 注册表失败：{e}", "", ""),
    ("models.available_count", "可用模型 {enabled}/{total}", "", ""),
    ("models.json_read_failed", "无法读取 models.json，为避免覆盖已禁用保存：{e}", "", ""),
    ("skills.failed_to_toggle_count", "{total} 个技能中有 {count} 个未能更改", "", ""),
    ("psp.delete_project_confirm", "删除项目 {name} 及其所有会话？", "", ""),
    ("status.running_tools_with_extra", "正在运行 {names}（另有 {count} 个）...", "", ""),
    ("messages.no_output", "（无输出）", "", ""),
    // 031 @ 文件检索 + ! shell 命令
    ("composer.files", "文件", "檔案", "Files"),
    ("composer.slash_commands", "斜杠命令", "斜槓命令", "Slash commands"),
    ("composer.no_matching_files", "没有匹配的文件", "沒有符合的檔案", "No matching files"),
    ("composer.shell_local_only", "Shell · 输出仅本地（!!）", "Shell · 輸出僅本機（!!）", "Shell · output stays local (!!)"),
    ("composer.shell_to_model", "Shell · 输出发给模型", "Shell · 輸出傳送給模型", "Shell · output sent to model"),
    ("composer.running_hint", "执行中 · Esc 中止", "執行中 · Esc 中止", "Running · Esc to abort"),
    ("composer.shell_busy", "会话忙，无法执行 shell 命令", "會話忙碌，無法執行 shell 命令", "Session is busy — can't run a shell command"),
    ("composer.shell_empty", "命令为空：! 后面接 shell 命令", "命令為空：! 後面接 shell 命令", "Empty command — type a shell command after !"),
    ("status.compacting", "压缩中…", "壓縮中…", "Compacting…"),
    ("status.auto_retry", "出错，{secs}s 后自动重试（第 {attempt}/{max} 次）", "出錯，{secs}s 後自動重試（第 {attempt}/{max} 次）", "Error — retrying in {secs}s (attempt {attempt}/{max})"),
    ("composer.compacting_wait", "上下文压缩中，请稍等……", "上下文壓縮中，請稍等……", "Compacting context, please wait…"),
    ("composer.generating_title", "生成标题…", "生成標題…", "Generating title…"),
    ("composer.title_set", "已生成标题: {title}", "已生成標題: {title}", "Title set: {title}"),
    ("composer.title_failed", "标题生成失败: {e}", "標題生成失敗: {e}", "Title generation failed: {e}"),
    // exit banner
    ("exit.process_exited", "Process exited with code {code_text}", "Process exited with code {code_text}", "Process exited with code {code_text}"),
    ("exit.unknown", "unknown", "unknown", "unknown"),
    // git badges etc stay ASCII
    // ---- 以下为存量漏译：繁/英待补，tr() 回落中文 ----
    ("actions.edit", "编辑", "", ""),
    ("branch.detour_note", "*此对话曾短暂探索另一分支后返回，摘要如下：*", "", ""),
    ("composer.compacting_wait2", "上下文压缩中，请等待……", "", ""),
    ("composer.hint_esc", "ESC打断模型", "", ""),
    ("composer.hint_shortcuts", "/skill，@文件，!shell，Shift回车换行", "", ""),
    ("custom_models.add", "添加自定义 Provider", "", ""),
    ("custom_models.base_url_hint", "留空 Base URL 使用 pi 内置端点；API Key 支持 ENV 变量", "", ""),
    ("custom_models.configure", "配置 Provider", "", ""),
    ("custom_models.endpoint_hint", "OpenAI / Anthropic 兼容端点；留空 Base URL 使用 pi 内置端点", "", ""),
    ("custom_models.exists", "已存在名为「{name}」的 Provider", "", ""),
    ("custom_models.field_context", "上下文窗口（tokens）", "", ""),
    ("custom_models.field_id", "ID", "", ""),
    ("custom_models.field_name", "Provider 名称", "", ""),
    ("custom_models.key_hint", "ENV_VAR_NAME、!shell-command 或明文 key", "", ""),
    ("custom_models.model_id_required", "模型 ID 不能为空", "", ""),
    ("custom_models.name_invalid", "Provider 名称只能使用字母、数字、_ 和 -", "", ""),
    ("custom_models.name_required", "Provider 名称不能为空", "", ""),
    ("custom_models.new_model", "新模型", "", ""),
    ("custom_models.reasoning_badge", "推理模型（T 徽标）", "", ""),
    ("dock.mcp", "MCP", "", ""),
    ("dock.other", "其他", "", ""),
    ("extensions.source_example", "例：pi install npm:pi-web-access", "", ""),
    ("file_tree.search", "搜索文件", "", ""),
    ("file_view.image_read_failed", "图片读取失败", "", ""),
    ("git.clean", "工作区干净", "", ""),
    ("git.commit_placeholder", "提交信息（提交已暂存更改）", "", ""),
    ("git.no_commits", "暂无提交", "", ""),
    ("git.pushed", "推送完成", "", ""),
    ("git.pushing", "推送中…", "", ""),
    ("git.stage_all", "Stage All", "", ""),
    ("git.staged", "提交已暂存", "", ""),
    ("mcp.closed", "已关闭", "", ""),
    ("mcp.failed_count", "{count} 个服务器未能更改", "", ""),
    ("mcp.field_name", "MCP名称", "", ""),
    ("mcp.form_placeholder", "粘贴 JSON / url / 命令行 / pi mcp add...", "", ""),
    ("mcp.form_title", "MCP配置", "", ""),
    ("mcp.name_invalid", "服务器名称只能使用字母、数字、_ 和 -", "", ""),
    ("mcp.next_turn", "MCP 清单将在下一轮生效", "", ""),
    ("mcp.none_configured", "没有已配置的 MCP 服务器", "", ""),
    ("mcp.save_failed", "MCP 保存失败: {e}", "", ""),
    ("mcp.saved_next_session", "已保存，对新会话生效", "", ""),
    ("mcp.select", "选择 MCP", "", ""),
    ("menu.no_command_found", "未找到扩展、提示词或技能命令", "", ""),
    ("menu.tab_enter_insert", "Tab / Enter 插入", "", ""),
    ("messages.before", "之前", "", ""),
    ("messages.extended_hidden", "扩展消息（不显示）", "", ""),
    ("messages.generating_args", "正在生成参数...", "", ""),
    ("messages.too_large", "消息内容过大，已按纯文本显示", "", ""),
    ("messages.truncated", "回复因达到模型输出长度上限而被截断。发送一条后续消息以继续。", "", ""),
    ("model.add_provider", "添加 Provider", "", ""),
    ("model.count", "{n} models", "", ""),
    ("model.keep_one", "至少需要保留一个启用的模型", "", ""),
    ("model.key_not_in_vault", "密钥不在系统凭据库，请重新保存", "", ""),
    ("model.loading_registry", "正在读取 Provider 注册表…", "", ""),
    ("model.no_provider_match", "没有匹配的 Provider", "", ""),
    ("model.oauth_unavailable", "OAuth 登录暂未接入，订阅服务商暂时无法在此配置", "", ""),
    ("model.search_provider", "搜索 Provider...", "", ""),
    ("plugins.copy_install", "复制安装命令，做全局安装", "", ""),
    ("plugins.from", "从", "", ""),
    ("plugins.state", "状态", "", ""),
    ("psp.confirm_delete", "确认删除", "", ""),
    ("psp.delete_project", "删除项目及所有会话", "", ""),
    ("psp.list_mode", "列表方式", "", ""),
    ("psp.maximize_session_list", "最大化会话列表", "最大化會話列表", "Maximize session list"),
    ("psp.messages_count", "{n} 条消息", "", ""),
    ("psp.new_session", "新建会话", "新建會話", "New session"),
    ("psp.new_session_in_project", "在此项目新建会话", "在此專案新建會話", "New session in project"),
    ("psp.no_sessions", "无会话", "", ""),
    ("psp.open_in_file_browser", "在文件浏览器中打开", "", ""),
    ("psp.open_project", "打开项目", "開啟專案", "Open project"),
    ("psp.project_deleted", "项目已删除（{n} 个会话）", "", ""),
    ("psp.project_groups", "项目分组列表", "", ""),
    ("psp.recent_sessions", "最近会话列表", "", ""),
    ("psp.search", "搜索", "搜尋", "Search"),
    ("psp.search_sessions", "搜索会话", "搜尋會話", "Search sessions"),
    ("psp.session_label", "会话", "", ""),
    ("psp.sort", "排序", "排序", "Sort"),
    ("psp.sort_by_time", "按更新时间排序", "", ""),
    ("psp.sort_manual", "手动排序", "", ""),
    ("psp.sort_mode", "排序方式", "", ""),
    ("psp.split_tip", "拖拽调整上下比例（双击复位 50%）", "拖曳調整上下比例（雙擊復位 50%）", "Drag to resize (double-click to reset)"),
    ("psp.switch_to_flat", "切换为最近会话列表", "切換為最近會話列表", "Switch to recent sessions"),
    ("psp.switch_to_grouped", "切换为项目分组列表", "切換為專案分組列表", "Switch to project groups"),
    ("remote.qr_confirm", "已扫码，请在手机上确认…", "", ""),
    ("remote.qr_desc", "用手机微信扫码，把当前聊天绑定到这台机器的 pi-flash。", "", ""),
    ("remote.qr_hint", "用手机微信「扫一扫」对屏扫码；约 2 分钟后过期。", "", ""),
    ("remote.qr_loading", "正在获取二维码…", "", ""),
    ("remote.qr_title", "扫码绑定", "", ""),
    ("remote.select_provider", "选择模型供应商", "", ""),
    ("remote.select_thinking", "选择思考档位", "", ""),
    ("remote.switch_session", "切换会话", "", ""),
    ("remote.switch_workspace", "切换工作区", "", ""),
    ("session_search.hint", "输入关键词，搜索当前项目的会话内容", "", ""),
    ("session_search.no_match", "没有匹配结果", "", ""),
    ("session_search.partial", "部分结果", "", ""),
    ("session_search.placeholder", "搜索会话内容…", "", ""),
    ("session_search.searching", "搜索中…", "", ""),
    ("settings.data_dir", "工作区数据目录", "", ""),
    ("settings.memory_dir", "会话记忆 / 布局状态存放位置", "", ""),
    ("settings.restore_hint", "启动时恢复上次激活的项目与会话", "", ""),
    ("settings.restore_title", "启动恢复", "", ""),
    // 082 设置「快捷键」页
    ("shortcuts.group_file_tree", "文件树", "檔案樹", "File tree"),
    ("shortcuts.group_global", "全局", "全域", "Global"),
    ("shortcuts.rename_hint", "选中文件或目录时", "選中檔案或目錄時", "When a file or folder is selected"),
    ("shortcuts.view_left", "向左切换视图", "向左切換視圖", "Switch view left"),
    ("shortcuts.view_left_hint", "切换到当前标签左侧视图", "切換到當前標籤左側視圖", "Switch to the view left of the current tab"),
    ("shortcuts.view_right", "向右切换视图", "向右切換視圖", "Switch view right"),
    ("shortcuts.view_right_hint", "切换到当前标签右侧视图", "切換到當前標籤右側視圖", "Switch to the view right of the current tab"),
    ("stats.cache_hit_rate", "缓存命中率", "快取命中率", "Cache hit rate"),
    ("stats.cache_read", "缓存读", "", ""),
    ("stats.cache_write", "缓存写", "", ""),
    ("stats.context_capacity", "上下文容量", "上下文容量", "Context usage"),
    ("stats.context_tokens", "上下文 token", "", ""),
    ("stats.cost", "费用", "", ""),
    ("stats.duration", "用时", "", ""),
    ("stats.input", "输入", "", ""),
    ("stats.messages", "消息", "訊息", "Messages"),
    ("stats.no_usage", "会话尚未产生用量。", "", ""),
    ("stats.share", "使用比例", "", ""),
    ("stats.tokens_total", "Token 累计", "", ""),
    ("stats.total_cost", "累计费用", "", ""),
    ("status.running_tool", "正在运行 {name}...", "", ""),
    ("status.running_tools", "正在运行 {names}...", "", ""),
    ("status.send_failed", "发送失败", "", ""),
    ("status.waiting_model", "正在等待模型...", "", ""),
    ("terminal.none", "暂无终端会话", "", ""),
    ("titlebar.expand_sidebar", "展开侧栏", "", ""),
    ("titlebar.toggle_sidebar", "收起 / 展开侧栏", "", ""),
    ("tools.preset_rebound", "工具预设: {k} (会话进程已重绑)", "", ""),
];

/// Translate a **semantic key** for the active language.
///
/// - 键不存在 → 原样返回（未知字符串穿透，新 UI 优雅降级）
/// - **译文为空 → 回落 zh-CN**（存量漏译的行为与迁移前一致：繁/英界面显示
///   中文，而不是空白、也不是裸键名。漏译清单由 src/i18n_audit.rs 盯着）
pub fn tr<'a>(s: &'a str) -> &'a str {
    let row = TABLE.iter().find(|e| e.0 == s);
    match lang_ix() {
        0 => row.map(|e| e.1).unwrap_or(s),
        ix => row
            .map(|e| match ix {
                1 => e.2,
                _ => e.3,
            })
            .filter(|t| !t.is_empty())
            .or_else(|| row.map(|e| e.1))
            .unwrap_or(s),
    }
}

/// Render a `{key}` template in the active language.
pub fn tf(template: &str, pairs: &[(&str, String)]) -> String {
    let mut out = tr(template).to_string();
    for (k, v) in pairs {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tests mutate the process-global language; serialize them
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn identity_for_source_language() {
        let _g = LOCK.lock();
        set_lang(0);
        assert_eq!(tr("action.save"), "保存");
        assert_eq!(tr("something.new"), "something.new");
    }

    #[test]
    fn translations_for_other_languages() {
        let _g = LOCK.lock();
        set_lang(1);
        assert_eq!(tr("action.save"), "儲存");
        assert_eq!(tr("session.fork"), "新分支");
        assert_eq!(tr("session.creating"), "建立中…");
        assert_eq!(tr("new_session.greeting"), "Hi，打算讓我做點什麼？");
        assert_eq!(tr("project.search_placeholder"), "搜尋項目…");
        assert_eq!(tr("action.open_folder"), "開啟資料夾");
        set_lang(2);
        assert_eq!(tr("action.save"), "Save");
        assert_eq!(tr("session.fork"), "Fork");
        assert_eq!(tr("session.creating"), "Creating…");
        assert_eq!(tr("new_session.greeting"), "Hi, what shall we work on?");
        assert_eq!(tr("project.search_placeholder"), "Search projects…");
        assert_eq!(tr("action.open_folder"), "Open folder");
        assert_eq!(tr("not.in.table"), "not.in.table");
        set_lang(0);
    }

    /// 存量漏译：繁/英单元格为空 → 回落 zh-CN（**不能**空白、不能回落裸键名）
    #[test]
    fn untranslated_falls_back_to_zh() {
        let _g = LOCK.lock();
        set_lang(2);
        assert_eq!(tr("git.stage_all"), "Stage All");
        set_lang(1);
        assert_eq!(tr("git.stage_all"), "Stage All");
        set_lang(0);
    }

    /// 键与文案解耦：改译文不动键（迁移的核心收益）
    #[test]
    fn key_is_independent_of_copy() {
        let _g = LOCK.lock();
        set_lang(0);
        assert_eq!(tr("settings.show_thinking"), "展示思考");
        set_lang(2);
        assert_eq!(tr("settings.show_thinking"), "Show thinking");
        set_lang(0);
    }

    #[test]
    fn template_replacement() {
        let _g = LOCK.lock();
        set_lang(2);
        assert_eq!(
            tf("extensions.installed", &[("source", "npm:x".to_string())]),
            "Installed npm:x"
        );
        set_lang(0);
        assert_eq!(
            tf("extensions.installed", &[("source", "npm:x".to_string())]),
            "已安装 npm:x"
        );
    }
}
