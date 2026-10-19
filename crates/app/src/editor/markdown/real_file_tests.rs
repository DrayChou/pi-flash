/// 真实文章回归（025 P1）：12 个引用全展开 + 磁盘能找到图片文件。
#[test]
fn real_article_all_refs_expand_and_resolve() {
    let p = std::path::Path::new(
        r"D:\my_obsidian\gitee_vault\mynotes\20观宏知微\文章\1 每日博文\一块RTX5070跑Qwen3.8-next-flash 150B飚速90TS！算力自由降临了？.md",
    );
    let Ok(src) = std::fs::read_to_string(p) else {
        eprintln!("skip: 真实文章不在本机");
        return;
    };
    let refs = src.matches("![[").count() + src.matches("![](").count();
    let blocks = super::doc_blocks(&src);
    let imgs: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match b {
            super::MdBlock::Image { url, .. } => Some(url.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(imgs.len(), refs, "引用应全展开: {imgs:?}");
    // 每个 url：md 目录直查 或 assets/<子目录>/同名命中（render 出图前提）
    let base = p.parent().unwrap();
    for u in &imgs {
        let name = std::path::Path::new(u).file_name().unwrap();
        let direct = base.join(u);
        let hit = direct.exists()
            || std::fs::read_dir(base.join("assets"))
                .map(|rd| rd.flatten().any(|e| e.path().join(name).is_file()))
                .unwrap_or(false);
        assert!(hit, "找不到图片文件: {u}");
    }
    eprintln!("全部 {refs} 个引用展开且磁盘命中");
}
