//! 021 文件树操作栏 + 右键菜单的文件动作：切换显示变更 / 搜索文件弹窗 /
//! 新建文件·目录 / 改名 / 删除至回收站（trash crate）/ 复制路径 /
//! @ 插入会话输入框 / 资源管理器定位。

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

/// 文件搜索命中上限（pi-web file-index 打分截断同量级；500×500 弹窗
/// 内滚动翻不完 200 行之外的内容已够用）。
const FILE_SEARCH_LIMIT: usize = 200;

/// 文件清单缓存条目（key = cwd；021 细节定稿：60s TTL，最多 10 份——
/// 超限整表清空，at_index 同策略）。
struct ListingEntry {
    files: std::sync::Arc<Vec<String>>,
    built: std::time::Instant,
}

static LISTING_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, ListingEntry>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 带 60s×10 缓存的文件清单（git ls-files / BFS 回退）。命中 TTL 内
/// 直接复用——弹窗里逐字过滤不再每键重扫盘；TTL 过期或换 cwd 才真扫。
fn cached_listing(cwd: &Path) -> std::sync::Arc<Vec<String>> {
    const TTL: std::time::Duration = std::time::Duration::from_secs(60);
    const MAX_PROJECTS: usize = 10;
    let key = cwd.to_string_lossy().to_string();
    if let Ok(cache) = LISTING_CACHE.lock() {
        if let Some(e) = cache.get(&key) {
            if e.built.elapsed() < TTL {
                return e.files.clone();
            }
        }
    }
    let files = std::sync::Arc::new(crate::services::file_index::load_listing(cwd).files);
    if let Ok(mut cache) = LISTING_CACHE.lock() {
        if cache.len() >= MAX_PROJECTS {
            cache.clear();
        }
        cache.insert(key, ListingEntry { files: files.clone(), built: std::time::Instant::now() });
    }
    files
}

impl Chat {
    // -------------------------------------------------------------------
    // 操作栏
    // -------------------------------------------------------------------

    /// 「切换显示变更」：开 = 只列 git 变更文件（flatten changes_only，
    /// 全量下钻无视展开集合）。持久化走 UiState.tree_changes_only。
    pub(crate) fn toggle_changes_only(&mut self, cx: &mut Context<Self>) {
        self.changes_only = !self.changes_only;
        self.rebuild_tree();
        self.persist_ui();
        cx.notify();
    }

    /// 「搜索文件」：500×500 居中弹窗（同 Dialog 体系，焦点/ESC/外点
    /// 关闭全走 dialog_shell）。再点一次操作栏按钮 = 关（toggle 语义）。
    pub(crate) fn open_file_search(&mut self, cx: &mut Context<Self>) {
        if matches!(self.dialog, Some(Dialog::FileSearch { .. })) {
            self.dialog = None;
            cx.notify();
            return;
        }
        let weak = cx.weak_entity();
        let weak_esc = weak.clone();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("搜索文件…"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    let _ = weak.update(cx, |c, cx| c.file_search_changed(q, cx));
                }))
                .on_escape(Box::new(move |cx: &mut App| {
                    // 关弹窗会 drop 派发事件的输入框实体——defer 出帧再关
                    // （provider_picker_dialog 同款 dispatch-lease 规避）
                    let weak = weak_esc.clone();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |c, cx| {
                            c.dialog = None;
                            cx.notify();
                        });
                    });
                }))
        });
        self.file_search_hits.clear();
        self.file_search_needle.clear();
        self.file_search_gen += 1;
        self.dialog = Some(Dialog::FileSearch {
            input,
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
    }

    /// 查询词变化：防抖 250ms 后台扫盘（load_listing = git ls-files，非
    /// git 目录 BFS 回退），代际丢陈旧响应（search_changed 同款）。
    pub(crate) fn file_search_changed(&mut self, q: &str, cx: &mut Context<Self>) {
        self.file_search_gen += 1;
        let epoch = self.file_search_gen;
        let needle = q.trim().to_string();
        self.file_search_needle = needle.clone();
        self.file_search_hits.clear();
        if needle.is_empty() {
            self.file_search_scanning = false;
            cx.notify();
            return;
        }
        self.file_search_scanning = true;
        let cwd = self.cwd.clone();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let cancelled = weak
                .update(cx, |c, _| c.file_search_gen != epoch)
                .unwrap_or(true);
            if cancelled {
                return;
            }
            let hits = cx
                .background_spawn(async move {
                    let files = cached_listing(&cwd);
                    filter_file_names(&files, &needle)
                })
                .await;
            let _ = weak.update(cx, |c, cx| {
                if c.file_search_gen != epoch {
                    return;
                }
                c.file_search_scanning = false;
                c.file_search_hits = hits;
                cx.notify();
            });
        })
        .detach();
    }

    /// 命中行点击：打开文件 tab（html/htm 自动路由浏览器，open_file_tab
    /// 既有语义；10MB/二进制闸同样生效）。
    pub(crate) fn open_file_search_hit(&mut self, rel: &str, cx: &mut Context<Self>) {
        self.dialog = None;
        self.open_file_tab(self.cwd.join(rel), cx);
    }

    // -------------------------------------------------------------------
    // 右键菜单：新建 / 改名 / 删除 / 复制路径 / @ 插入
    // -------------------------------------------------------------------

    /// 「新建文件/新建目录」弹窗：在 `dir` 下按输入名创建（名字可含子目录）。
    pub(crate) fn open_file_new(&mut self, dir: PathBuf, is_dir: bool, cx: &mut Context<Self>) {
        let weak_submit = cx.weak_entity();
        let weak_esc = weak_submit.clone();
        let dir_submit = dir.clone();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("文件名（可含子目录）")));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let weak = weak_submit.clone();
                let dir = dir_submit.clone();
                let v = v.trim().to_string();
                // 提交会 drop 派发事件的输入框（dialog 换型）——defer 出帧
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| c.file_new_commit(&dir, is_dir, v, cx));
                });
            }));
            ti.set_on_escape(Box::new(move |cx: &mut App| {
                let weak = weak_esc.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dialog = None;
                        cx.notify();
                    });
                });
            }));
        });
        self.dialog = Some(Dialog::FileNew { dir, is_dir, input });
        cx.notify();
    }

    /// 新建提交：名字裁边后逐段校验（禁 `..` / 空段 / 绝对路径），存在
    /// 即报错不覆盖；成功后沿途目录展开 + 刷新 git（未跟踪文件立即可见）。
    pub(crate) fn file_new_commit(
        &mut self,
        dir: &Path,
        is_dir: bool,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let name = name.trim().trim_matches(['/', '\\']).to_string();
        if name.is_empty() {
            self.set_status(tr("名称不能为空").to_string(), cx);
            return;
        }
        let segs: Vec<&str> = name.split(['/', '\\']).collect();
        if name.starts_with('/') || name.contains(':') || segs.iter().any(|s| s.is_empty() || *s == "..") {
            self.set_status(tr("路径越出项目根").to_string(), cx);
            return;
        }
        let target = dir.join(&name);
        if !target.starts_with(&self.cwd) {
            self.set_status(tr("路径越出项目根").to_string(), cx);
            return;
        }
        if target.exists() {
            self.set_status(format!("{}：{}", tr("已存在"), target.display()), cx);
            return;
        }
        let r = if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())
        } else {
            std::fs::create_dir_all(target.parent().unwrap_or(dir))
                .and_then(|_| std::fs::write(&target, b""))
                .map_err(|e| e.to_string())
        };
        match r {
            Ok(_) => {
                // 沿途目录展开，让新条目立即可见（树与变更过滤两态都认）
                let mut anc = target.parent().map(|p| p.to_path_buf());
                while let Some(p) = anc {
                    if same_path(&p, &self.cwd) {
                        break;
                    }
                    self.expanded_dirs.insert(p.clone());
                    anc = p.parent().map(|q| q.to_path_buf());
                }
                self.dialog = None;
                self.refresh_git();
                self.set_status(format!("{}：{}", tr("已创建"), name), cx);
                cx.notify();
            }
            Err(e) => {
                self.set_status(format!("{}: {e}", tr("创建失败")), cx);
            }
        }
    }

    /// 「改名」弹窗：输入框预填原名并全选。
    pub(crate) fn open_file_rename(&mut self, path: PathBuf, is_dir: bool, cx: &mut Context<Self>) {
        let prefill =
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let weak_submit = cx.weak_entity();
        let weak_esc = weak_submit.clone();
        let path_submit = path.clone();
        let input = cx.new(|cx| TextInput::new(cx).select_all_on_focus().placeholder(tr("名称")));
        input.update(cx, |ti, cxi| ti.set_value(prefill, cxi));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let weak = weak_submit.clone();
                let path = path_submit.clone();
                let v = v.trim().to_string();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| c.file_rename_commit(&path, is_dir, v, cx));
                });
            }));
            ti.set_on_escape(Box::new(move |cx: &mut App| {
                let weak = weak_esc.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dialog = None;
                        cx.notify();
                    });
                });
            }));
        });
        self.dialog = Some(Dialog::FileRename { path, is_dir, input });
        cx.notify();
    }

    /// 改名提交：仅同级改名（禁路径分隔符），同名（字面）静默关弹窗。
    /// 成功后打开的文件 tab / 缓冲 / 待聚焦 / 展开集合（目录）按前缀重映射。
    pub(crate) fn file_rename_commit(
        &mut self,
        path: &Path,
        is_dir: bool,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.set_status(tr("名称不能为空").to_string(), cx);
            return;
        }
        if name.contains('/') || name.contains('\\') || name == ".." {
            self.set_status(tr("名称不能包含路径分隔符").to_string(), cx);
            return;
        }
        let Some(parent) = path.parent() else { return };
        let target = parent.join(&name);
        if target == path {
            // 字面同名：视为取消（大小写改名走 fs::rename 的原生支持）
            self.dialog = None;
            cx.notify();
            return;
        }
        if target.exists() {
            self.set_status(format!("{}：{}", tr("已存在"), target.display()), cx);
            return;
        }
        if let Err(e) = std::fs::rename(path, &target) {
            self.set_status(crate::i18n::tf("重命名失败: {e}", &[("e", e.to_string())]), cx);
            return;
        }
        // 打开面重映射：tab 与缓冲持旧路径的话跟上（目录改名 = 整棵前缀）
        let remap = |p: &PathBuf| -> PathBuf {
            let hit = if is_dir {
                same_path(p, path) || p.starts_with(path)
            } else {
                same_path(p, path)
            };
            if hit {
                let rel = p.strip_prefix(path).unwrap_or(p);
                target.join(rel)
            } else {
                p.clone()
            }
        };
        for t in &mut self.panel_tabs {
            if let PanelTab::File(p) = t {
                *p = remap(p);
            }
        }
        self.file_cache = self.file_cache.drain().map(|(k, v)| (remap(&k), v)).collect();
        if let Some(f) = self.pending_focus_file.as_mut() {
            *f = remap(f);
        }
        if is_dir {
            self.expanded_dirs = self
                .expanded_dirs
                .iter()
                .map(remap)
                .collect();
        }
        self.dialog = None;
        self.refresh_git();
        self.set_status(tr("已改名").to_string(), cx);
        cx.notify();
    }

    /// 「删除」第一步：挂统一确认浮层（overlay::confirm，居中）。
    pub(crate) fn confirm_tree_delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tree_menu = None;
        self.editor_menu = None;
        self.tree_confirm_del = Some(path);
        cx.notify();
    }

    /// 确认后真删：trash crate 送回收站（Windows 回收站 / macOS 废纸篓，
    /// 可恢复）。打开中的 tab 丢弃、展开集合清枝，git 随刷。
    pub(crate) fn tree_delete_commit(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.tree_confirm_del.take() else { return };
        // 兜底护栏：项目根不可删（操作栏右键菜单已豁免该入口，这里防
        // 其他调用路径误投递）
        if same_path(&path, &self.cwd) {
            self.set_status(tr("不能删除项目根目录").to_string(), cx);
            cx.notify();
            return;
        }
        match trash::delete(&path) {
            Ok(_) => {
                let stale: Vec<PathBuf> = self
                    .panel_tabs
                    .iter()
                    .filter_map(|t| match t {
                        PanelTab::File(p) if same_path(p, &path) || p.starts_with(&path) => {
                            Some(p.clone())
                        }
                        _ => None,
                    })
                    .collect();
                for p in stale {
                    self.discard_file_tab(&p, cx);
                }
                // 兕底：file_cache 可能残留无 tab 的 FileTab（历史关闭路径
                // 只摘 tab 没清缓存），按路径一并清，防编辑器还渲染已删文件
                self.file_cache
                    .retain(|k, _| !(same_path(k, &path) || k.starts_with(&path)));
                self.expanded_dirs.retain(|d| !(same_path(d, &path) || d.starts_with(&path)));
                self.refresh_git();
                self.set_status(tr("已移入回收站").to_string(), cx);
                cx.notify();
            }
            Err(e) => {
                // 持久弹层：状态栏 toast 一闪容易漏看，用户回报也拿不到
                // 具体错误
                self.tree_del_error = Some(e.to_string());
                self.set_status(crate::i18n::tf("删除失败: {e}", &[("e", e.to_string())]), cx);
                cx.notify();
            }
        }
    }

    /// 「复制路径」：绝对路径进剪贴板（gpui 原生 ClipboardItem）。
    pub(crate) fn tree_copy_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tree_menu = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            path.to_string_lossy().to_string(),
        ));
        self.set_status(tr("已复制").to_string(), cx);
        cx.notify();
    }

    /// 「@到会话输入框」：相对 cwd 路径经 build_at_insert_text（含空格
    /// 自动引号、目录不闭合供钻取）追加到输入框末尾，光标落在插入之后；
    /// 内容区切回会话让插入可见（在终端/文件视图时）。
    pub(crate) fn tree_at_insert(&mut self, path: PathBuf, is_dir: bool, cx: &mut Context<Self>) {
        self.tree_menu = None;
        let rel = path
            .strip_prefix(&self.cwd)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let (text, off) = crate::services::at_file::build_at_insert_text(&rel, is_dir);
        let mut base = std::mem::take(&mut self.input);
        if !base.is_empty() && !base.ends_with(' ') && !base.ends_with('\n') {
            base.push(' ');
        }
        let cursor = base.len() + off;
        base.push_str(&text);
        self.set_input_with_cursor(base, cursor, cx);
        self.set_content_view(ContentView::Chat);
    }
}

/// 「在资源管理器中打开」的文件语义（与 open_in_explorer 的目录语义分
/// 开）：文件 = 打开所在目录并选中（explorer /select, / macOS open -R），
/// 目录 = 直接进入。
pub(crate) fn reveal_in_explorer(path: &Path) {
    #[cfg(windows)]
    {
        if path.is_dir() {
            let _ = std::process::Command::new("explorer").arg(path).spawn();
        } else {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if path.is_dir() {
            let _ = std::process::Command::new("open").arg(path).spawn();
        } else {
            let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
        }
    }
}

/// 文件名子串过滤 + 排序：名字前缀命中 > 名字包含 >（带 `/` 的查询）
/// 整路径包含；同档路径短者先。上限 [`FILE_SEARCH_LIMIT`]。
fn filter_file_names(files: &[String], needle: &str) -> Vec<String> {
    let n = needle.to_lowercase();
    let mut scored: Vec<(u8, String)> = files
        .iter()
        .filter(|f| {
            let name = f.rsplit(['/']).next().unwrap_or(f);
            if name.to_lowercase().contains(&n) {
                true
            } else if n.contains('/') {
                f.to_lowercase().contains(&n)
            } else {
                false
            }
        })
        .map(|f| {
            let name = f.rsplit(['/']).next().unwrap_or(f);
            let rank = if name.to_lowercase().starts_with(&n) { 0u8 } else { 1u8 };
            (rank, f.clone())
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(&b.1))
    });
    scored.truncate(FILE_SEARCH_LIMIT);
    scored.into_iter().map(|(_, f)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::filter_file_names;

    #[test]
    fn file_name_filter_ranks_and_caps() {
        let files: Vec<String> = vec![
            "src/lib.rs".into(),
            "src/main.rs".into(),
            "docs/main.md".into(),
            "target/debug/main.exe".into(),
        ];
        // 名字前缀命中排前；同档路径短者先（main.exe 同为前缀命中）
        let hits = filter_file_names(&files, "main");
        assert_eq!(
            hits,
            vec![
                "src/main.rs".to_string(),
                "docs/main.md".to_string(),
                "target/debug/main.exe".to_string()
            ]
        );
        // 带 / 的查询走整路径匹配
        let hits = filter_file_names(&files, "src/main");
        assert_eq!(hits, vec!["src/main.rs".to_string()]);
        // 大小写不敏感
        let hits = filter_file_names(&files, "LIB");
        assert_eq!(hits, vec!["src/lib.rs".to_string()]);
    }
}
