use crate::db::Db;
use crate::models::*;
use crate::paths::{novel_meta_path, novel_tree_path};
use anyhow::Result;
use chrono::Utc;
use std::fs;

pub fn ensure_sample(db: &Db) -> Result<()> {
    if !db.list_novels()?.is_empty() {
        return Ok(());
    }

    let id = "sample-demo-novel".to_string();
    let now = Utc::now().to_rfc3339();
    let novel = NovelProject {
        id: id.clone(),
        title: "雾港回声（示例）".into(),
        synopsis: "一座被潮汐淹没的港口，少年带着旧地图寻找失踪的灯塔看守人。".into(),
        cover_path: None,
        knowledge_ids: vec![],
        knowledge_strategy: "参考知识库中的叙事节奏与人物出场节点。".into(),
        archived: false,
        word_count_min: 2000,
        word_count_max: 3000,
        chapter_count: 20,
        created_at: now.clone(),
        updated_at: now,
    };
    db.upsert_novel(&novel)?;

    let meta = serde_json::to_string_pretty(&novel)?;
    fs::write(novel_meta_path(&id), meta)?;

    let tree = sample_tree(&id);
    fs::write(novel_tree_path(&id), serde_json::to_string_pretty(&tree)?)?;
    Ok(())
}

fn sample_tree(novel_id: &str) -> NovelTree {
    let root = "n-root";
    let c1 = "c-1";
    let c2 = "c-2";
    let c3 = "c-3";
    let hero = "char-hero";
    let side = "char-side";
    let plot = "side-1";

    NovelTree {
        novel_id: novel_id.into(),
        nodes: vec![
            TreeNode {
                id: root.into(),
                kind: NodeKind::Novel,
                label: "雾港回声（示例）".into(),
                outline: "主线：寻灯塔".into(),
                character: None,
                knowledge: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 280.0, y: 0.0 },
                word_count: 0,
                word_count_min: 2000,
                word_count_max: 3000,
                chapter_count: 20,
            },
            TreeNode {
                id: c1.into(),
                kind: NodeKind::Chapter,
                label: "第一章 · 退潮之后".into(),
                outline: "少年回到废弃码头，发现灯塔的钥匙还留在潮水线以上。".into(),
                character: None,
                knowledge: None,
                linked_character_ids: vec![hero.into()],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 280.0, y: 160.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            },
            TreeNode {
                id: c2.into(),
                kind: NodeKind::Chapter,
                label: "第二章 · 旧地图".into(),
                outline: "在鱼市档案室找到被撕去一角的港口图，配角暗示有人故意抹去路线。".into(),
                character: None,
                knowledge: None,
                linked_character_ids: vec![hero.into(), side.into()],
                linked_side_plot_ids: vec![plot.into()],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 280.0, y: 320.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            },
            TreeNode {
                id: c3.into(),
                kind: NodeKind::Chapter,
                label: "第三章 · 雾中灯火".into(),
                outline: "夜航靠近灯塔，主线与支线在雾中短暂交错。".into(),
                character: None,
                knowledge: None,
                linked_character_ids: vec![hero.into(), side.into()],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 280.0, y: 480.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            },
            TreeNode {
                id: hero.into(),
                kind: NodeKind::Character,
                label: "林潮（主角）".into(),
                outline: "".into(),
                character: Some(CharacterCard {
                    role: "主角".into(),
                    personality: "沉默、执拗、对海有本能的敬畏".into(),
                    motto: "潮水退去，真相才会露出礁石。".into(),
                    gender: "男".into(),
                    style: "少言，行动优先".into(),
                    alignment: "正派".into(),
                }),
                knowledge: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 40.0, y: 160.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            },
            TreeNode {
                id: side.into(),
                kind: NodeKind::Character,
                label: "阿棠（配角）".into(),
                outline: "".into(),
                character: Some(CharacterCard {
                    role: "配角".into(),
                    personality: "圆滑、消息灵通、偶尔嘴硬心软".into(),
                    motto: "在雾港，知情不报也是一种活法。".into(),
                    gender: "女".into(),
                    style: "旁敲侧击".into(),
                    alignment: "中立".into(),
                }),
                knowledge: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 520.0, y: 320.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            },
            TreeNode {
                id: plot.into(),
                kind: NodeKind::SidePlot,
                label: "支线 · 档案室夜班".into(),
                outline: "另一视角：夜班管理员听见档案柜自己打开，却不见人影。".into(),
                character: None,
                knowledge: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 520.0, y: 400.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            },
        ],
        edges: vec![
            TreeEdge {
                id: "e-r-1".into(),
                source: root.into(),
                target: c1.into(),
                kind: "chapter".into(),
                source_handle: Some("bottom".into()),
                target_handle: Some("top".into()),
                label: String::new(),
            },
            TreeEdge {
                id: "e-1-2".into(),
                source: c1.into(),
                target: c2.into(),
                kind: "chapter".into(),
                source_handle: Some("bottom".into()),
                target_handle: Some("top".into()),
                label: String::new(),
            },
            TreeEdge {
                id: "e-2-3".into(),
                source: c2.into(),
                target: c3.into(),
                kind: "chapter".into(),
                source_handle: Some("bottom".into()),
                target_handle: Some("top".into()),
                label: String::new(),
            },
            TreeEdge {
                id: "e-1-hero".into(),
                source: c1.into(),
                target: hero.into(),
                kind: "character".into(),
                source_handle: Some("left".into()),
                target_handle: Some("right".into()),
                label: String::new(),
            },
            TreeEdge {
                id: "e-2-side".into(),
                source: c2.into(),
                target: side.into(),
                kind: "character".into(),
                source_handle: Some("left".into()),
                target_handle: Some("right".into()),
                label: String::new(),
            },
            TreeEdge {
                id: "e-2-plot".into(),
                source: c2.into(),
                target: plot.into(),
                kind: "side_plot".into(),
                source_handle: Some("right".into()),
                target_handle: Some("left".into()),
                label: String::new(),
            },
        ],
    }
}
