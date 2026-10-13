//! 文件 tab 生命周期：打开路由（二进制/超限拒绝、已开复用）、
//! 保存/自动保存（023，Zed after-timeout 同构）、外部改动检测与冲突 banner、
//! tab 激活（MRU 置顶）/关闭/无条件丢弃。
//! （P2 归并：actions_panels 的 tab 侧。）

use crate::*;

/// 自动保存静默期（023；Zed autosave after_timeout 同构）。
const AUTOSAVE_DEBOUNCE_MS: u64 = 1000;

impl Chat {
    /// v54.4：文件打开路由——统一打开为内容区 tab，主体渲染与编辑器配置
    /// 由 kind handler 决定（未注册类型走默认文本 kind）。html/htm 直接
    /// 开源码（2026-10-12 二改拍板，不再旁路系统浏览器；浏览器入口保留在
    /// 右键菜单首位文件动作）。
    ///
    /// 023 改版：编辑器实体懒创建（InputState::new 要 window，本调用链
    /// 没有——渲染帧在 content.rs 里补），二进制/超限文件提示不打开。
    pub(crate) fn open_file_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // 消息文本/远程控制可能传来相对路径：统一按 cwd 绝对化（已开复用
        // 的 same_path 比较与磁盘读取都基于绝对形态）
        let path = if path.is_absolute() { path } else { self.ws.cwd.join(path) };
        const MAX: u64 = 10 * 1024 * 1024;
        if std::fs::metadata(&path).map(|m| m.len() > MAX).unwrap_or(false) {
            self.set_status(
                format!("{}：{}", crate::i18n::tr("file_view.too_large"), path.display()),
                cx,
            );
            return;
        }
        // 已打开则直接切过去（Zed 行为）：不重读磁盘、不整文重灌——那会
        // 复位滚动/光标，并在大文件上冻结数百毫秒。外部改动由 fs watcher
        // 负责：cwd 内文件走 cwd 的递归 watch，cwd 外文件由
        // retain_file_watches 按所在目录单挂监听，两边信号都经
        // check_external_file_changes 按 disk_sig 复查后自动重载/标冲突。
        if let Some(ix) = self
            .ws.panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, &path)))
        {
            self.activate_panel_tab(ix, cx);
            self.set_content_view(ContentView::File);
            self.ws.pending_focus_file = Some(path.clone());
            cx.notify();
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                // 带上完整路径：消息里的路径点击失败（相对解析/拼写/盘符），
                // 没有路径无法定位是哪个文件没打开
                self.set_status(
                    format!("{} {}: {e}", crate::i18n::tr("file_view.read_failed"), path.display()),
                    cx,
                );
                return;
            }
        };
        if bytes.contains(&0) {
            self.set_status(
                format!("{}：{}", crate::i18n::tr("file_view.binary"), path.display()),
                cx,
            );
            return;
        }
        let content = String::from_utf8_lossy(&bytes).to_string();
        let sig = file_sig(&path);
        let mut ft = FileTab::from_disk(content);
        ft.disk_sig = sig;
        self.ws.file_cache.insert(path.clone(), ft);
        self.ws.pending_focus_file = Some(path.clone());
        self.ws.panel_tabs.push(PanelTab::File(path));
        // cwd 外文件：按所在目录补一条单文件监听（否则磁盘改了不同步）
        self.retain_file_watches();
        let ix = self.ws.panel_tabs.len() - 1;
        self.activate_panel_tab(ix, cx);
        self.set_content_view(ContentView::File);
        cx.notify();
    }

    /// 关文件 tab：有未保存修改（或挂着外部改动冲突）先弹确认，否则直接
    /// 关。zed parity：已从磁盘消失（Deleted）且无未保存修改的缓冲不算
    /// 脏——直接关，不弹「未保存的修改」；脏的照旧弹（保存并关闭=重建文件）。
    pub(crate) fn close_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let dirty = self
            .ws.file_cache
            .get(path)
            .map(|f| match f.conflict {
                Some(crate::FileConflict::Deleted) => f.dirty,
                _ => f.dirty || f.conflict.is_some(),
            })
            .unwrap_or(false);
        if dirty {
            self.dlg.dialog = Some(crate::Dialog::FileDirty { path: path.to_path_buf() });
            cx.notify();
            return;
        }
        self.discard_file_tab(path, cx);
    }

    /// 无条件丢弃文件 tab（确认弹窗三个按钮的公共尾）。
    pub(crate) fn discard_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self
            .ws.panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, path)))
        else {
            return;
        };
        let was_active = self.ws.active_panel_tab == Some(ix);
        self.close_panel_tab(ix, cx);
        self.ws.file_cache.remove(path);
        // 关掉后可能没人再需要这个目录的监听：卸掉
        self.retain_file_watches();
        if was_active && self.layout.content_view == ContentView::File {
            // 激活位可能落到终端 tab 上——优先指去最近的文件 tab；
            // 一个文件 tab 都不剩则内容区回退（有终端回浏览区，否则回会话）
            let any_file = self
                .ws.panel_tabs
                .iter()
                .position(|t| matches!(t, PanelTab::File(_)));
            match any_file {
                Some(fix)
                    if !matches!(
                        self.ws.panel_tabs.get(self.ws.active_panel_tab.unwrap_or(usize::MAX)),
                        Some(PanelTab::File(_))
                    ) =>
                {
                    self.activate_panel_tab(fix, cx);
                }
                None => {
                    let v = if self.ws.panel_tabs.is_empty() {
                        ContentView::Chat
                    } else {
                        self.layout.browse_last
                    };
                    self.set_content_view(v);
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// 保存文件 tab（Ctrl+S / 关闭确认「保存并关闭」共用）：编辑器在则取
    /// 编辑器值，写盘成功后刷新磁盘真值缓存与外部改动基准。
    pub(crate) fn save_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let err = self.write_file_back(path, cx);
        if let Some(e) = err {
            self.set_status(format!("{}: {e}", crate::i18n::tr("file_view.save_failed")), cx);
            return;
        }
        self.set_status(crate::i18n::tr("status.saved").to_string(), cx);
        cx.notify();
    }

    /// 写盘 + 刷新磁盘真值/基准/脏位。Err = 写盘失败的错误串（调用方决定
    /// 提示方式：手动保存 toast，自动保存静默）。
    fn write_file_back(&mut self, path: &Path, cx: &mut Context<Self>) -> Option<String> {
        let text = match self.ws.file_cache.get(path).and_then(|f| f.editor.as_ref()) {
            Some(ed) => ed.read(cx).value().to_string(),
            None => match self.ws.file_cache.get(path) {
                Some(f) => f.content.clone(),
                None => return None,
            },
        };
        // zed fs.save 同款：先建父目录再写——已删文件保存即重建（目录被
        // 一并删除时也能恢复）；目录健在时 create_dir_all 是空操作
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Some(e.to_string());
            }
        }
        if let Err(e) = std::fs::write(path, &text) {
            return Some(e.to_string());
        }
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.content = text;
            ft.dirty = false;
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
        None
    }

    /// 023 自动保存（Zed after-timeout 同构）：编辑停顿约 1 秒写盘。每次
    /// 脏变起一个定时器；静默期内的后续编辑各自再起定时器，先到的发现
    /// 「最后编辑 < 1s」就让位退出，最晚的那个落盘。冲突挂着不自动写
    /// （等用户在横幅裁决），写盘静默（不抢状态栏）。
    /// （2026-10-10 用户定夺：恒开，无开关——默认行为即唯一行为）
    pub(crate) fn autosave_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(AUTOSAVE_DEBOUNCE_MS))
                    .await;
                let proceed = this.update(cx, |c, _| {
                    c.ws.file_cache.get(&path).is_some_and(|f| {
                        f.dirty
                            && f.conflict.is_none()
                            && f.last_edit
                                .is_some_and(|t| t.elapsed().as_millis() as u64 >= AUTOSAVE_DEBOUNCE_MS)
                    })
                });
                if !proceed.unwrap_or(false) {
                    return; // 已保存/被丢弃/冲突/仍在输入（后续定时器接管）
                }
                let _ = this.update(cx, |c, cx| {
                    let _ = c.write_file_back(&path, cx);
                });
                return;
            }
        })
        .detach();
    }

    /// Ctrl+S（FileSave action，"Input" 上下文绑定）：保存当前文件 tab。
    pub(crate) fn save_active_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.active_file_path() {
            self.save_file(&path, cx);
        }
    }

    /// 冲突 banner「重新加载」：磁盘为准，丢弃本地未保存修改。
    pub(crate) fn file_reload_from_disk(&mut self, path: &Path, cx: &mut Context<Self>) {
        let bytes = match std::fs::read(path) {
            Ok(b) if !b.contains(&0) => b,
            _ => {
                self.set_status(crate::i18n::tr("conflict.deleted_on_disk").to_string(), cx);
                return;
            }
        };
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.content = String::from_utf8_lossy(&bytes).to_string();
            ft.dirty = false;
            ft.reload_pending = true;
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
    }

    /// 冲突 banner「忽略」：保留本地缓冲；以当前磁盘态为新基准，磁盘再变
    /// 才会再次提示。
    pub(crate) fn file_ignore_conflict(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
    }

    /// 023 外部改动检测（对齐 Zed）：fs 监听泵的每个合批信号跑一遍。
    /// 无未保存修改 → 自动重载（reload_pending 由渲染帧灌进编辑器）；
    /// 有修改 → 标冲突，等用户在 banner 上裁决。自己的保存已把基准刷到
    /// 写后元数据，本轮信号比较为空转。
    pub(crate) fn check_external_file_changes(&mut self, cx: &mut Context<Self>) {
        self.ws.ext_probe.0 += 1;
        let mut changed = false;
        let paths: Vec<PathBuf> = self
            .ws.panel_tabs
            .iter()
            .filter_map(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        for path in paths {
            let Some(ft) = self.ws.file_cache.get_mut(&path) else {
                continue;
            };
            match std::fs::metadata(&path) {
                Err(_) => {
                    // 文件已从磁盘消失：标 Deleted（zed DiskState::Deleted 同构
                    // ——缓冲保留可继续编辑、tab 删除线、保存即重建；不弹横幅）。
                    // 已在 Deleted 态不重复置位（watcher 泵 150ms 一轮，避免
                    // 每轮空 notify）
                    if ft.conflict != Some(crate::FileConflict::Deleted) {
                        ft.conflict = Some(crate::FileConflict::Deleted);
                        changed = true;
                    }
                }
                Ok(md) => {
                    let sig = md.modified().ok().map(|m| (m, md.len()));
                    if sig.is_none() || sig == ft.disk_sig {
                        continue;
                    }
                    match std::fs::read(&path) {
                        Ok(b) if !b.contains(&0) => {
                            self.ws.ext_probe.1 += 1;
                            if !ft.dirty {
                                ft.content = String::from_utf8_lossy(&b).to_string();
                                ft.reload_pending = true;
                                ft.conflict = None;
                            } else if ft.conflict != Some(crate::FileConflict::Changed) {
                                // Deleted→Changed 覆盖：文件被外部重建而本地
                                // 仍有未保存修改（zed：has_conflict 弹裁决）
                                ft.conflict = Some(crate::FileConflict::Changed);
                            }
                            ft.disk_sig = sig;
                            changed = true;
                        }
                        _ => {
                            // 二进制化/读不出来：视同消失（同样只置一次）
                            if ft.conflict != Some(crate::FileConflict::Deleted) {
                                ft.conflict = Some(crate::FileConflict::Deleted);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// 当前激活的文件 tab 路径。
    pub(crate) fn active_file_path(&self) -> Option<PathBuf> {
        self.ws.active_panel_tab
            .and_then(|ix| self.ws.panel_tabs.get(ix))
            .and_then(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
    }

    /// 激活内容区 tab 并挪到标签流首位（023 小功能：选中标签永远在最左，
    /// zed 没有）。Term/File 都是轻量句柄（终端实体在 terminals 按 id 索引），
    /// 移动无副作用。
    pub(crate) fn activate_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.ws.panel_tabs.len() {
            return;
        }
        if ix != 0 {
            let tab = self.ws.panel_tabs.remove(ix);
            self.ws.panel_tabs.insert(0, tab);
        }
        self.ws.active_panel_tab = Some(0);
        // 切到终端 tab：同步 active_terminal（按键/自动化都读它；否则双终端
        // 切 tab 后输入会送进后台那个终端）
        if let Some(tix) = self.shown_terminal_index() {
            self.ws.active_terminal = Some(tix);
        }
        cx.notify();
    }

    pub(crate) fn close_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.ws.panel_tabs.len() {
            return;
        }
        let removed = self.ws.panel_tabs.remove(ix);
        if let PanelTab::Term(id) = &removed {
            if let Some(tix) = self.ws.terminals.iter().position(|t| t.id == *id) {
                let _ = self.ws.terminals[tix]
                    .pty
                    .send(alacritty_terminal::event_loop::Msg::Shutdown);
                self.ws.terminals.remove(tix);
                // 该终端的右键菜单若开着，随 tab 一起收（挂载处也校验，双保险）
                if self.dlg.term_menu.is_some_and(|(tid, _, _)| tid == *id) {
                    self.dlg.term_menu = None;
                }
                // 终端可开多个（同项目不限量）：下标随移除平移/夹紧，
                // 否则 active_terminal 会指错终端或越界
                self.ws.active_terminal = match self.ws.active_terminal {
                    Some(a) if a == tix => {
                        if self.ws.terminals.is_empty() {
                            None
                        } else {
                            Some(tix.min(self.ws.terminals.len() - 1))
                        }
                    }
                    Some(a) if a > tix => Some(a - 1),
                    other => other,
                };
            }
        }
        self.ws.active_panel_tab = match self.ws.active_panel_tab {
            Some(a) if a >= self.ws.panel_tabs.len() => {
                if self.ws.panel_tabs.is_empty() {
                    None
                } else {
                    Some(a.saturating_sub(1))
                }
            }
            other => other,
        };
        // 关 tab 后激活的可能就是另一个终端 tab：对齐一次（与激活路径同源）
        if let Some(tix) = self.shown_terminal_index() {
            self.ws.active_terminal = Some(tix);
        }
        cx.notify();
    }
}
