//! 会话池 Chat 方法域：new/open/delete/switch/subscribe_runtime + 会话改名。
//! （P2 归并：actions_sessions + actions_runtime + actions_rename 池侧；
//! SessionRuntime 本体在 session/runtime.rs。）

use crate::*;

impl Chat {
    pub(crate) fn subscribe_runtime(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(rt, |chat, rt, ev: &session::runtime::SessionEvent, cx| {
            use session::runtime::SessionEvent;
            let is_active = rt.read(cx).key == chat.active_key;
            match ev {
                SessionEvent::Changed => {
                    // recents (003-session管理): message-level activity on
                    // this runtime is "recently updated" — promote its file
                    // in the recent-activity list (persist throttled)
                    if let Some(f) = rt.read(cx).file.clone() {
                        pi_link::recents::touch_recent(&f);
                    }
                    if !is_active {
                        // v54 未读绿点：非活跃会话在跑/在流式 → 记未读，
                        // 切换到它时清除
                        let (file, running) = {
                            let r = rt.read(cx);
                            (
                                r.file.clone(),
                                r.agent_running
                                    || r.state.as_ref().is_some_and(|s| s.is_streaming),
                            )
                        };
                        if let Some(f) = file {
                            if running && !chat.unread.contains(&f) {
                                chat.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::TurnSettled => {
                    // 轮次终点（2026-10-10 规则修订）：agent 结束时**窗口没
                    // 焦点、或本会话已转后台**（用户切到同一窗口的别的对话）
                    // 就给点——三色分类：外部错误/崩溃 → 红；内部中断
                    // （length/aborted）→ 黄；正常完成 → 青蓝。在场看见不给。
                    // is_active = 该 runtime key vs 当前活跃 key，结算时刻
                    // 现算即前台/后台判定
                    if !chat.window_active || !is_active {
                        let (file, turn_error, turn_interrupted) = {
                            let r = rt.read(cx);
                            (r.file.clone(), r.turn_error, r.turn_interrupted)
                        };
                        if let Some(f) = file {
                            // 通知点只表达**最近一轮**的结局：先摘掉上一轮的
                            // 档位再挂新点——否则旧红点（优先级最高）会盖住
                            // 新一轮的完成通知，反之亦然
                            chat.unread.remove(&f);
                            chat.turn_errors.remove(&f);
                            chat.turn_warnings.remove(&f);
                            if turn_error {
                                chat.turn_errors.insert(f);
                            } else if turn_interrupted {
                                chat.turn_warnings.insert(f);
                            } else {
                                chat.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ListDirty => {
                    chat.refresh_sessions();
                    cx.notify();
                }
                SessionEvent::PendingTab(preview) => {
                    // 伪标签生命周期（draft 首条消息乐观上侧栏）：挂/刷新按
                    // key 幂等，撤按 key 摘除。真标签由 FileBound 落地。
                    let key = rt.read(cx).key.clone();
                    match preview {
                        Some(p) => {
                            let p = p.clone();
                            let cwd = rt.read(cx).cwd.to_string_lossy().to_string();
                            if let Some(e) =
                                chat.pending_tabs.iter_mut().find(|e| e.key == key)
                            {
                                e.preview = p;
                            } else {
                                chat.pending_tabs.push(PendingTab {
                                    key,
                                    preview: p,
                                    cwd,
                                });
                            }
                        }
                        None => {
                            chat.pending_tabs.retain(|e| e.key != key);
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ExtUi(req) => {
                    // 微信端与桌面端**并行**拿到同一请求（060 §4.1）：两边都要
                    // 渲染一份文本，但只有一方能应答 —— 桌面 ext_respond 会
                    // clear_pending，pi 侧也只 resolve 一次（§8 档 2 不重复消费）
                    chat.remote.on_ext_ui(req);
                    if is_active {
                        let req = req.clone();
                        chat.on_ext_ui(req, cx);
                    } else {
                        // parked session asks for permission: queue it, the
                        // badge surfaces it; popped when user switches there
                        rt.update(cx, |r, _| r.ext_queue.push(req.clone()));
                        cx.notify();
                    }
                }
                SessionEvent::Assistant(ev) => {
                    // 只回推**活跃会话**的输出（与入站投递口径一致，避免串会话）
                    if is_active {
                        for text in chat.remote.on_assistant(&ev) {
                            chat.remote.send(&text);
                        }
                    }
                }
                SessionEvent::Models(models) => {
                    // 进程答案写项目槽（pi-web /api/models parity：每个 runtime 只写
                    // 自己 cwd 的条目），同时**并进全局态并回写自有缓存**
                    // （010-启动.md §2/§4.1：包内联 provider 只有进程答得到，
                    // 存下来下次冷启动就有真名字）。
                    let cwd_key = rt.read(cx).cwd.to_string_lossy().to_string();
                    chat.models_by_cwd.insert(cwd_key, models.clone());
                    if crate::startup::merge_models(&mut chat.globals, &models) {
                        let _ = crate::startup::write_cache(
                            &chat.globals.models,
                            &chat.globals.commands,
                        );
                    }
                    chat.mc_state =
                        models_config::compute_state(chat.mc_patterns.as_ref(), &chat.mc_refs());
                    cx.notify();
                }
                SessionEvent::Commands(commands) => {
                    // 扩展命令（包内注册，磁盘没有数据文件）：并进全局清单 + 回写缓存。
                    // `skill:` 前缀不入全局（skill 走磁盘派生，见 startup::merge_commands）。
                    if crate::startup::merge_commands(&mut chat.globals, &commands) {
                        let _ = crate::startup::write_cache(
                            &chat.globals.models,
                            &chat.globals.commands,
                        );
                    }
                    cx.notify();
                }
                SessionEvent::FileBound(path) => {
                    chat.on_file_bound(&rt, path.clone(), cx);
                }
                SessionEvent::RenameReady(prefill) => {
                    if is_active {
                        if let Some(f) = rt.read(cx).file.clone() {
                            chat.start_rename(f, prefill.clone(), cx);
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Attention switch: flush the editor mirrors into the outgoing
    /// session, load the incoming one's inputPanel state. Zero process
    /// operations, zero IO — this is the pi-web "switch" and it is instant.
    pub(crate) fn switch_to(
        &mut self,
        rt: gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = self.runtimes.get(&self.active_key).cloned() {
            if old != rt {
                let (input, images, history) = (
                    std::mem::take(&mut self.input),
                    std::mem::take(&mut self.pending_images),
                    std::mem::take(&mut self.history),
                );
                old.update(cx, |r, _| {
                    r.input = input;
                    r.pending_images = images;
                    r.history = history;
                });
            }
        }
        let (input, images, history, key, file) =
            rt.update(cx, |r, _| {
                (
                    r.input.clone(),
                    r.pending_images.clone(),
                    r.history.clone(),
                    r.key.clone(),
                    r.file.clone(),
                )
            });
        self.input = input;
        self.pending_images = images;
        self.history = history;
        self.history_ix = None;
        self.active_key = key;
        self.active_file = file.clone();
        // 注意力切到会话 ⇒ 内容区必须回到 chat 前台：否则在终端/文件编辑
        // 视图里点会话列表行（或「新会话」）后，内容区还停在 Term/File，
        // 看着像「切了会话但窗口没切过来」。browse_last 不受影响，
        // statusbar 的 files tab 仍能回到终端/文件（set_content_view 只在
        // Term/File 时更新它）。
        if self.content_view != crate::ContentView::Chat {
            self.set_content_view(crate::ContentView::Chat);
        }
        if let Some(f) = &file {
            set_last_open(&self.cwd.to_string_lossy(), &f.to_string_lossy());
            // v54: 切入会话清除未读/出错红点
            self.unread.remove(f);
            self.turn_errors.remove(f);
            self.turn_warnings.remove(f);
        }
        self.pill_menu = None;
        self.menu_ix = 0;
        // 「点外/Esc 收起」不能跨会话泄漏：否则上个会话收起过菜单，切过来
        // 后 / @ 永远弹不出（active_menu 第一行就短路）
        self.menu_dismissed = false;
        // surface queued permission requests of the incoming session
        let queued = rt.update(cx, |r, _| {
            let q = std::mem::take(&mut r.ext_queue);
            r.touch();
            q
        });
        if let Some(first) = queued.first() {
            let first = first.clone();
            cx.notify();
            self.on_ext_ui(first, cx);
            for r in queued.into_iter().skip(1) {
                rt.update(cx, |r2, _| r2.ext_queue.push(r));
            }
        }
        cx.notify();
    }
}

impl Chat {
    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        // lazy draft (pi-web parity): the pi process spawns on the first
        // prompt, not now; running sessions are untouched
        let key = format!("draft-{}", self.draft_seq);
        self.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key.clone(), self.cwd.clone(), None);
            r.default_model = services::workspace::default_model_pref();
            r.status = tr("session.new").to_string();
            r
        });
        self.runtimes.insert(key, rt.clone());
        self.subscribe_runtime(&rt, cx);
        clear_last_open(&self.cwd.to_string_lossy());
        self.renaming = None;
        self.rename_input = None;
        self.switch_to(rt, cx);
    }

    /// Draft persisted its first prompt: pi bound this process to a fresh
    /// session file (pi-web promoteNewSession parity). Migrate the pool key
    /// draft-N → path so the sidebar entry and the pool share one identity,
    /// otherwise reopening the session would spawn a duplicate runtime.
    pub(crate) fn on_file_bound(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let path_key = path.to_string_lossy().to_string();
        let old_key = rt.read(cx).key.clone();
        if old_key == path_key {
            return;
        }
        // 伪标签到此退役：磁盘扫描现在找得到真文件，真标签接管
        self.pending_tabs.retain(|e| e.key != old_key);
        rt.update(cx, |r, _| r.key = path_key.clone());
        // 会话级插件清单跟着身份走（草稿 key → 会话文件路径）：确认时按草稿
        // key 落盘，这里把它搬到真实会话键上，重启后按会话恢复（034）
        if let Some(store) = pi_link::session_ext::store_path() {
            let sources = pi_link::session_ext::read_for(&store, &old_key);
            if !sources.is_empty() {
                let _ = pi_link::session_ext::write_for(&store, &path_key, &sources);
            }
            let _ = pi_link::session_ext::remove_for(&store, &old_key);
        }
        if let Some(entity) = self.runtimes.remove(&old_key) {
            self.runtimes.insert(path_key.clone(), entity);
        }
        if self.active_key == old_key {
            self.active_key = path_key.clone();
        }
        self.active_file = Some(path.clone());
        // recents (003-session管理): a fresh session file just landed
        pi_link::recents::touch_recent(&path);
        set_last_open(&rt.read(cx).cwd.to_string_lossy(), &path_key);
        self.refresh_sessions();
        cx.notify();
    }

    pub(crate) fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
        // recents (003-session管理): opening is activity — promote to the
        // top of the recent-activity list (persist throttled)
        pi_link::recents::touch_recent(&path);
        // cwd lookup: current project first, then all psp groups (v54 跨项目)
        let cwd = self
            .sessions
            .iter()
            .find(|s| s.path == path)
            .map(|s| PathBuf::from(s.cwd.clone()))
            .or_else(|| {
                self.projects
                    .iter()
                    .find(|g| g.sessions.iter().any(|s| s.path == path))
                    .map(|g| g.path.clone())
            })
            .unwrap_or_else(|| self.cwd.clone());
        let key = path.to_string_lossy().to_string();
        // pool hit: switch attention — instant, all messages in place
        if let Some(rt) = self.runtimes.get(&key).cloned() {
            self.cwd = cwd;
            self.branch = read_branch(&self.cwd);
            self.renaming = None;
            self.rename_input = None;
            self.confirm_delete = None;
            if rename {
                rt.update(cx, |r, _| r.pending_rename = true);
            }
            self.switch_to(rt, cx);
            return;
        }
        // new runtime: tail render first (cache or disk), process spawns in
        // the background — the conversation is on screen before node exists
        let tail = match self.session_tail_cache.get(&path).cloned() {
            Some(msgs) => msgs,
            None => {
                let msgs = msgs_from_tail(read_tail_messages(&path, 256 * 1024, 100));
                if self.session_tail_cache.len() >= crate::services::workspace::TAIL_PRELOAD * 2 {
                    self.session_tail_cache.clear();
                }
                self.session_tail_cache.insert(path.clone(), msgs.clone());
                msgs
            }
        };
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key, cwd.clone(), Some(path.clone()));
            r.default_model = services::workspace::default_model_pref();
            // leaf-chain integrity probe for the get_messages guard
            r.disk_msg_count = pi_link::sessions::count_message_entries(&path) as usize;
            r.messages = tail;
            r.pager.reload(r.messages.len());
            r.status = "resuming".into();
            r.pending_rename = rename;
            r
        });
        self.runtimes.insert(rt.read(cx).key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        if !same_ws(&cwd.to_string_lossy(), &self.cwd.to_string_lossy()) {
            self.cwd = cwd;
            self.branch = read_branch(&self.cwd);
            self.expanded_dirs.clear();
            // 文件树：根项目行默认展开
            self.expanded_dirs.insert(self.cwd.clone());
            self.refresh_sessions();
            self.load_project_files();
            // 换工作区：git 状态/树缓存/fs watcher 全部跟着 cwd 重挂
            //（此前这里漏 refresh_git，git 面板会显示上一个项目的状态）
            self.refresh_git();
            self.attach_fs_watch();
        }
        set_last_open(&self.cwd.to_string_lossy(), &path.to_string_lossy());
        self.renaming = None;
        self.rename_input = None;
        self.confirm_delete = None;
        self.switch_to(rt.clone(), cx);
        let rt_spawn = rt;
        cx.spawn(async move |_this, cx| {
            let _ = rt_spawn.update(cx, |r, cx| {
                if r.agent.session.is_none() {
                    if let Some(rx) = r.spawn() {
                        let epoch = r.agent.epoch;
                        session::runtime::SessionRuntime::attach_pump(&rt_spawn, rx, epoch, cx);
                    }
                }
                r.rpc(&Command::GetMessages);
                r.refresh_anchors();
                r.refresh_state();
            });
        })
        .detach();
    }

    /// pi-web DELETE /api/sessions/{id}: unlink the session file; a live
    /// (active) session is aborted + shut down first and the shell resets
    /// to a fresh draft with the same cwd.
    pub(crate) fn delete_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.confirm_delete = None;
        let key = path.to_string_lossy().to_string();
        // 会话没了，它的自定义插件清单也清掉（034）
        if let Some(store) = pi_link::session_ext::store_path() {
            let _ = pi_link::session_ext::remove_for(&store, &key);
        }
        let was_active = self.runtimes.get(&self.active_key)
            .map(|rt| rt.read(cx).file.as_deref() == Some(path.as_path()))
            .unwrap_or(false);
        // tear down the runtime (abort + process kill; dropping the entity
        // clears the file handle on Windows before unlink)
        if let Some(rt) = self.runtimes.remove(&key) {
            rt.update(cx, |r, _| {
                r.rpc(&Command::Abort);
                r.shutdown_process();
            });
        }
        self.unread.remove(&path);
        self.turn_errors.remove(&path);
        self.turn_warnings.remove(&path);
        // recents (003-session管理): the file is gone, drop the entry
        pi_link::recents::remove_recent(&path);
        if was_active {
            self.active_file = None;
            self.new_session(cx);
        }
        match std::fs::remove_file(&path) {
            Ok(_) => {
                self.sessions.retain(|s| s.path != path);
                self.session_tail_cache.remove(&path);
                // refresh_sessions 只重扫当前 cwd，其余项目组沿用各组缓存——
                // 非当前项目的会话也要在这里摘掉，否则列表一直显示已删会话
                for g in &mut self.projects {
                    g.sessions.retain(|s| s.path != path);
                }
                self.set_status("session deleted".into(), cx);
            }
            Err(e) => {
                self.set_status(crate::i18n::tf("extensions.remove_failed", &[("e", e.to_string())]), cx);
            }
        }
        self.refresh_sessions();
        cx.notify();
    }
}

impl Chat {
    pub(crate) fn start_rename(&mut self, path: PathBuf, prefill: String, cx: &mut Context<Self>) {
        let weak_ok = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .select_all_on_focus()
                .placeholder(tr("action.session_name"))
        });
        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| c.apply_rename(v.trim().to_string(), cx));
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.renaming = None;
                    c.rename_input = None;
                    cx.notify();
                });
            }));
        });
        self.renaming = Some(path);
        self.rename_input = Some(input);
        cx.notify();
    }

    /// Rename commit path that never reads the input entity (called from
    /// the input's own submit callback where the entity is borrowed).
    pub(crate) fn apply_rename(&mut self, name: String, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::SetSessionName { name });
        // sidebar label comes from the session file's `session_info` entry;
        // reload it when pi confirms the write (set_session_name response —
        // an immediate re-read here would race the file flush)
        self.refresh_state(cx);
        self.renaming = None;
        self.rename_input = None;
        cx.notify();
    }

}

// ---------------------------------------------------------------------------
// 会话池方法：刷新/切换/活跃态/上下文/标题（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn refresh_state(&self, cx: &mut gpui::App) {
        self.rt().update(cx, |r, _| r.refresh_state());
    }

    pub(crate) fn refresh_sessions(&mut self) {
        let cwd = self.cwd.to_string_lossy().to_string();
        let full = list_sessions_for_cwd(&cwd, 500)
            .into_iter()
            .filter(|s| same_ws(&s.cwd, &cwd))
            .collect::<Vec<_>>();
        // v60: 加载语义 = 时间窗口（设置.加载时间窗口）——当前项目只保留
        // 窗口内活跃的会话（mtime 近似清单 last_active）+ 当前激活会话
        // 兜底（保证改名/打开的旧会话始终可见）；显示量由 psp 分页控制
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(load_window_days() * 86_400);
        let mut sessions: Vec<SessionInfo> = full
            .iter()
            .filter(|s| s.modified >= cutoff)
            .cloned()
            .collect();
        if let Some(active) = &self.active_file {
            if !sessions.iter().any(|s| &s.path == active) {
                if let Some(info) = full.iter().find(|s| &s.path == active) {
                    sessions.push(info.clone());
                }
            }
        }
        self.sessions = sessions;
        // sync the current project's group, then re-pin project order
        let all: Vec<SessionInfo> = self
            .projects
            .iter()
            .flat_map(|g| {
                if same_ws(&g.path.to_string_lossy(), &cwd) {
                    self.sessions.clone()
                } else {
                    g.sessions.clone()
                }
            })
            .collect();
        self.rebuild_projects(all);
    }

    pub(crate) fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.cwd = cwd;
        // mark as the globally-last active workspace for startup restore
        set_last_workspace(&self.cwd.to_string_lossy());
        self.branch = read_branch(&self.cwd);
        self.refresh_sessions();
        self.dialog = None;
        self.expanded_dirs.clear();
        // 文件树：根项目行默认展开
        self.expanded_dirs.insert(self.cwd.clone());
        self.refresh_git();
        self.load_project_files();
        self.attach_fs_watch();
        // cwd 变了：cwd 外单文件监听面跟着重算（旧 cwd 下的文件现在要单挂）
        self.retain_file_watches();
        if let Some(p) = get_last_open(&self.cwd.to_string_lossy()) {
            let path = PathBuf::from(&p);
            if path.exists() {
                self.open_session(path, false, cx);
                cx.notify();
                return;
            }
        }
        self.new_session(cx);
        cx.notify();
    }

    pub(crate) fn rt(&self) -> gpui::Entity<session::runtime::SessionRuntime> {
        self.runtimes
            .get(&self.active_key)
            .expect("active runtime exists")
            .clone()
    }

    /// 「其他」页提示音开关 → 全池广播。轮末提示音归 runtime 职责，每个常驻
    /// runtime 各持一份副本，所以新开关必须扇出到**所有** runtime 而不只是活跃
    /// 那个——后台会话同样会跑 agent、同样需要提示。
    pub(crate) fn broadcast_sound_on(&self, on: bool, cx: &mut Context<Self>) {
        for (_, rt) in &self.runtimes {
            rt.update(cx, |r, _| r.sound_on = on);
        }
    }

    /// 当前项目的上下文（010-启动.md §5）：命中启动装载的集合就直接返回，
    /// 未命中（新打开的项目）用同一个装载函数现算一次并纳入集合。
    pub(crate) fn project_ctx_now(&mut self) -> startup::ProjectCtx {
        let key = self.cwd.to_string_lossy().to_string();
        if let Some(ctx) = self.project_ctx.get(&key) {
            return ctx.clone();
        }
        let ctx = startup::load_project(&self.cwd.clone());
        self.project_ctx.insert(key, ctx.clone());
        ctx
    }

    /// 当前会话标题（topbar 会话视图展示；≤30 字截断）。优先 pi 会话名，
    /// 回落首条用户消息。
    pub(crate) fn session_title(&self, cx: &gpui::App) -> String {
        let file = self
            .runtimes
            .get(&self.active_key)
            .and_then(|rt| rt.read(cx).file.clone());
        let raw = file
            .and_then(|f| {
                self.projects
                    .iter()
                    .flat_map(|g| &g.sessions)
                    .find(|s| same_path(&s.path, &f))
                    .and_then(|s| {
                        s.name
                            .clone()
                            .filter(|n| !n.trim().is_empty())
                            .or_else(|| {
                                Some(s.preview.clone()).filter(|p| !p.trim().is_empty())
                            })
                    })
            })
            .unwrap_or_else(|| tr("session.new").to_string());
        // 首行 + ≤30 字
        let first_line = raw.lines().next().unwrap_or("").trim().to_string();
        let mut out: String = first_line.chars().take(30).collect();
        if first_line.chars().count() > 30 {
            out.push('…');
        }
        out
    }

}
