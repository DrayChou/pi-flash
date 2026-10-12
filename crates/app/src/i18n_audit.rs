// i18n 漏译守卫（`tr("…")` 的参数既是显示值、也是查表键）。
//
// 背景：i18n.rs 的 `TABLE` 首列就是中文源串，`tr("…")` 传字面量时**既是
// 值**（zh-CN 下 `0 => s` 原样显示）**又是键**（繁/英下 `find(|e| e.0 == s)`）。
// 两处必须逐字节相等，而 Rust 对此零检查——润色一个字、或新增文案忘了配
// 译文，繁/英界面都会**静默回落成中文**（用户实测两次：展示思考的描述、
// 整条列表项只剩标点那次同源）。
//
// 本测试做源码级审计（读 `src/**/*.rs` 与 `src/i18n.rs` 的 TABLE 三元组）：
// 1. **新字面量查不到表 → 失败**并逐条列出（真正的回归防线：以后新增 UI
//    忘了配繁/英，CI 直接红）；
// 2. `BACKLOG` 里的**存量**漏译不失败，只打印剩余条数——补完一条就从
//    BACKLOG 删一条，列表只减不增；
// 3. 表里有、但源码再无字面量调用的条目（常量/变量传参等）只打印，不判定。
//
// 判定用 **TABLE 的键集合**，不能用「`set_lang(2)` 后 `tr(s)==s`」的行为法：
// 表里 11 条 EN 译文与中文原串相同（如 `no git`/`session name`/`API Key`），
// 行为法会把它们误报成漏译。

#[cfg(test)]
mod audit {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::path::PathBuf;

    /// 存量漏译清单（zh-CN 显示值）。补齐繁/英后请把该条从 TABLE 补上、再从
    /// 这里删除——测试会校验「清单里的每一条都仍然是漏译」，防止清单变成死账。
    const BACKLOG: &[&str] = &[
    "/skill，!shell命令，Shift回车换行",
    "ENV_VAR_NAME、!shell-command 或明文 key",
    "ESC打断模型",
    "ID",
    "MCP",
    "MCP 保存失败: {e}",
    "MCP 清单将在下一轮生效",
    "MCP名称",
    "MCP配置",
    "OAuth 登录暂未接入，订阅服务商暂时无法在此配置",
    "Provider 名称",
    "Provider 名称不能为空",
    "Provider 名称只能使用字母、数字、_ 和 -",
    "Stage All",
    "Tab / Enter 插入",
    "Token 累计",
    "{count} 个服务器未能更改",
    "{n} models",
    "{n} 条消息",
    "上下文",
    "上下文 token",
    "上下文压缩中，请等待……",
    "上下文窗口（tokens）",
    "之前",
    "会话",
    "会话尚未产生用量。",
    "会话记忆 / 布局状态存放位置",
    "使用比例",
    "例：pi install npm:pi-web-access",
    "其他",
    "切换会话",
    "切换工作区",
    "列表方式",
    "删除项目及所有会话",
    "发送失败",
    "启动恢复",
    "启动时恢复上次激活的项目与会话",
    "回复因达到模型输出长度上限而被截断。发送一条后续消息以继续。",
    "图片读取失败",
    "在文件浏览器中打开",
    "密钥不在系统凭据库，请重新保存",
    "展开侧栏",
    "工作区干净",
    "工作区数据目录",
    "工具预设: {k} (会话进程已重绑)",
    "已保存，对新会话生效",
    "已关闭",
    "已存在名为「{name}」的 Provider",
    "已改名",
    "手动排序",
    "扩展消息（不显示）",
    "按更新时间排序",
    "排序方式",
    "推理模型（T 徽标）",
    "推送中…",
    "推送完成",
    "提交信息（提交已暂存更改）",
    "提交已暂存",
    "搜索 Provider...",
    "搜索中…",
    "搜索会话内容…",
    "搜索文件",
    "收起 / 展开侧栏",
    "新模型",
    "无会话",
    "暂无提交",
    "暂无终端会话",
    "最近会话列表",
    "服务器名称只能使用字母、数字、_ 和 -",
    "未找到扩展、提示词或技能命令",
    "模型 ID 不能为空",
    "正在生成参数...",
    "正在等待模型...",
    "正在读取 Provider 注册表…",
    "正在运行 {names}...",
    "正在运行 {name}...",
    "没有匹配的 Provider",
    "没有匹配结果",
    "没有已配置的 MCP 服务器",
    "消息内容过大，已按纯文本显示",
    "添加 Provider",
    "添加自定义 Provider",
    "状态",
    "用时",
    "确认删除",
    "粘贴 JSON / url / 命令行 / pi mcp add...",
    "累计费用",
    "缓存写",
    "缓存命中率",
    "缓存读",
    "编辑",
    "至少需要保留一个启用的模型",
    "费用",
    "输入",
    "输入关键词，搜索当前项目的会话内容",
    "选择 MCP",
    "选择思考档位",
    "选择模型供应商",
    "部分结果",
    "配置 Provider",
    "项目分组列表",
    "项目已删除（{n} 个会话）",
    "（无输出）",
    ];

    fn src_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }

    /// i18n.rs 的 TABLE 首列（= 全部合法键）。只认标准三元组行 `("k", "a", "b"),`。
    fn table_keys(i18n_src: &str) -> BTreeSet<String> {
        let Some(body) = i18n_src.split_once("const TABLE") else {
            panic!("i18n.rs 里找不到 TABLE");
        };
        let mut keys = BTreeSet::new();
        let mut started = false;
        for line in body.1.lines() {
            if !started {
                if line.contains("&[") {
                    started = true;
                }
                continue;
            }
            let t = line.trim();
            if t.starts_with("];") {
                break;
            }
            if let Some(k) = first_string_literal(t) {
                keys.insert(k);
            }
        }
        assert!(started, "i18n.rs 的 TABLE 声明格式变了");
        keys
    }

    /// 取一行里第一个 Rust 字符串字面量的**内容**（转义原样保留，两侧同源可比）。
    fn first_string_literal(s: &str) -> Option<String> {
        let start = s.find('"')?;
        let b = s.as_bytes();
        let mut out = String::new();
        let mut i = start + 1;
        while i < b.len() {
            match b[i] {
                b'\\' if i + 1 < b.len() => {
                    out.push('\\');
                    out.push(b[i + 1] as char);
                    i += 2;
                }
                b'"' => return Some(out),
                _ => {
                    // 保持 UTF-8 完整：按字符推进
                    let ch = s[i..].chars().next()?;
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        None
    }

    /// 扫源码，取出所有 `tr("…")` / `tf("…")` 的**字面量**参数（常量/变量传参
    /// 天然不在其中——它们由 TABLE 里的常量键兜底，见 ABOUT_*）。
    fn literal_calls(root: &Path) -> BTreeSet<String> {
        let mut files = Vec::new();
        walk(root, &mut files);
        let mut found = BTreeSet::new();
        for f in files {
            // i18n.rs 自身跳过：TABLE 条目与 tr 行为测试里的字面量是「元数据」，
            // 不是调用点（含故意写成 "not in table" 的负例）。
            // 本文件也要跳过：扫描器源码里的 `["tr(", "tf("]` 会被当成调用点。
            match f.file_name().and_then(|s| s.to_str()) {
                Some("i18n.rs") | Some("i18n_audit.rs") => continue,
                _ => {}
            }
            for line in read(&f).lines() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with("///") || t.starts_with('*') {
                    continue;
                }
                for call in ["tr(", "tf("] {
                    let mut from = 0usize;
                    while let Some(at) = line[from..].find(call) {
                        let abs = from + at;
                        from = abs + call.len();
                        // 标识符的一部分（如 `xtr(`）不算调用
                        let prev = line[..abs].chars().next_back();
                        if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        if let Some(lit) = first_string_literal(&line[from..]) {
                            if !lit.is_empty() {
                                found.insert(lit);
                            }
                        }
                    }
                }
            }
        }
        found
    }

    #[test]
    fn no_new_untranslated_literals() {
        let root = src_root();
        let keys = table_keys(&read(&root.join("i18n.rs")));
        assert!(keys.len() > 100, "TABLE 解析异常，只读到 {} 个键", keys.len());
        let calls = literal_calls(&root);

        let missing: Vec<&String> = calls.iter().filter(|c| !keys.contains(*c)).collect();

        let stale: Vec<&&str> = BACKLOG
            .iter()
            .filter(|b| !missing.iter().any(|m| m.as_str() == **b))
            .collect();

        if !missing.is_empty() {
            // 「本次新增」= 存量清单之外的那些；清单已被删条目时不能相减（溢出）
            let fresh = missing
                .iter()
                .filter(|m| !BACKLOG.iter().any(|b| *m.as_str() == **b))
                .count();
            eprintln!(
                "\n⛔ i18n 漏译（存量 {} 条 + 新增 {} 条）：\n{}\n",
                missing.len() - fresh,
                fresh,
                missing
                    .iter()
                    .map(|s| format!("    \"{s}\","))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            eprintln!(
                "补法：在 crates/app/src/i18n.rs 的 TABLE 里加一行 \
                 (\"<上面的中文>\", \"<繁>\", \"<英>\"), 补完把 BACKLOG 里对应条目删掉。\n"
            );
        }

        assert!(
            stale.is_empty(),
            "BACKLOG 有 {} 条已不再漏译（补齐了或调用点已删），请把它们从 BACKLOG 里删掉：{stale:?}",
            stale.len()
        );
        assert!(
            missing.len() <= BACKLOG.len(),
            "⛔ 新增了 {} 条未配译文的 UI 文案（上面已列出）。\
             规则：`tr(\"中文\")` 的参数同时是显示值和 TABLE 的键，两者必须逐字节一致。",
            missing.len().saturating_sub(BACKLOG.len())
        );
        eprintln!(
            "i18n 审计：{} 个调用点字面量，{} 条存量漏译（BACKLOG {} 条）",
            calls.len(),
            missing.len(),
            BACKLOG.len()
        );
    }}
