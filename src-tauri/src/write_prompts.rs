//! 根上固定「生成/精修」卡：左右挂知识，只接到用户提示词后，不进入章节继承。

use crate::models::{KnowledgeCardPayload, NodeKind, NodePosition, NovelTree, TreeEdge, TreeNode};
use uuid::Uuid;

pub const WRITE_PROMPTS_SLOT: &str = "write_prompts";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritePromptKind {
    Generate,
    Refine,
}

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_write_prompts_slot(slot: &str) -> bool {
    slot.trim() == WRITE_PROMPTS_SLOT
}

pub fn write_prompts_hub(tree: &NovelTree) -> Option<&TreeNode> {
    tree.nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Knowledge) && is_write_prompts_slot(knowledge_slot(n)))
}

fn write_prompts_hub_id(tree: &NovelTree) -> Option<String> {
    write_prompts_hub(tree).map(|n| n.id.clone())
}

/// 根↔生成/精修卡（这条边不可断开）。
pub fn is_write_prompts_root_edge(tree: &NovelTree, source: &str, target: &str) -> bool {
    let Some(hub_id) = write_prompts_hub_id(tree) else {
        return false;
    };
    let Some(root_id) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.as_str())
    else {
        return false;
    };
    (source == root_id && target == hub_id) || (source == hub_id && target == root_id)
}

/// 本卡及其左右子卡：不进根/卷/章写作继承。
pub fn is_write_prompt_excluded_id(tree: &NovelTree, id: &str) -> bool {
    let Some(hub) = write_prompts_hub(tree) else {
        return false;
    };
    if hub.id == id {
        return true;
    }
    if hub.linked_knowledge_ids.iter().any(|x| x == id) {
        return true;
    }
    tree.edges.iter().any(|e| {
        let other = if e.source == hub.id {
            e.target.as_str()
        } else if e.target == hub.id {
            e.source.as_str()
        } else {
            return false;
        };
        other == id
            && tree
                .nodes
                .iter()
                .any(|n| n.id == other && matches!(n.kind, NodeKind::Knowledge))
    })
}

/// 补齐根上生成/精修卡；已有槽位不改内容。
pub fn ensure_write_prompts_card(tree: &mut NovelTree) -> bool {
    let Some(root) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    else {
        return false;
    };
    let root_id = root.id.clone();
    let root_pos = root.position;
    if write_prompts_hub(tree).is_some() {
        let hub_id = write_prompts_hub_id(tree).unwrap();
        let mut changed = false;
        if let Some(root) = tree.nodes.iter_mut().find(|n| n.id == root_id) {
            if !root.linked_knowledge_ids.contains(&hub_id) {
                root.linked_knowledge_ids.push(hub_id.clone());
                changed = true;
            }
        }
        let has_edge = tree.edges.iter().any(|e| {
            (e.source == root_id && e.target == hub_id) || (e.source == hub_id && e.target == root_id)
        });
        if !has_edge {
            tree.edges.push(TreeEdge {
                id: format!("e-{root_id}-{hub_id}"),
                source: root_id.clone(),
                target: hub_id.clone(),
                kind: "knowledge".into(),
                source_handle: Some("wp".into()),
                target_handle: Some("right".into()),
                label: String::new(),
            });
            changed = true;
        } else if let Some(edge) = tree.edges.iter_mut().find(|e| {
            (e.source == root_id && e.target == hub_id) || (e.source == hub_id && e.target == root_id)
        }) {
            if edge.source != root_id
                || edge.target != hub_id
                || edge.source_handle.as_deref() != Some("wp")
                || edge.target_handle.as_deref() != Some("right")
            {
                edge.source = root_id.clone();
                edge.target = hub_id;
                edge.source_handle = Some("wp".into());
                edge.target_handle = Some("right".into());
                changed = true;
            }
        }
        return changed;
    }
    let id = format!("kb-{}", Uuid::new_v4());
    tree.nodes.push(TreeNode {
        id: id.clone(),
        kind: NodeKind::Knowledge,
        label: "生成/精修".into(),
        outline: String::new(),
        detailed_outline: vec![],
        character: None,
        knowledge: Some(KnowledgeCardPayload {
            slot: WRITE_PROMPTS_SLOT.into(),
            ..Default::default()
        }),
        side_plot: None,
        volume: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: root_pos,
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    });
    if let Some(root) = tree.nodes.iter_mut().find(|n| n.id == root_id) {
        if !root.linked_knowledge_ids.contains(&id) {
            root.linked_knowledge_ids.push(id.clone());
        }
    }
    tree.edges.push(TreeEdge {
        id: format!("e-{root_id}-{id}"),
        source: root_id,
        target: id,
        kind: "knowledge".into(),
        source_handle: Some("wp".into()),
        target_handle: Some("right".into()),
        label: String::new(),
    });
    true
}

fn hub_side(tree: &NovelTree, hub_id: &str, edge: &TreeEdge) -> Option<WritePromptKind> {
    let handle = if edge.source == hub_id {
        edge.source_handle.as_deref()
    } else if edge.target == hub_id {
        edge.target_handle.as_deref()
    } else {
        return None;
    };
    match handle.map(str::trim) {
        Some("right") => Some(WritePromptKind::Refine),
        Some("left") => Some(WritePromptKind::Generate),
        _ => {
            let other = if edge.source == hub_id {
                edge.target.as_str()
            } else {
                edge.source.as_str()
            };
            let hub_x = tree
                .nodes
                .iter()
                .find(|n| n.id == hub_id)
                .map(|n| n.position.x)?;
            let other_x = tree
                .nodes
                .iter()
                .find(|n| n.id == other)
                .map(|n| n.position.x)?;
            if other_x < hub_x {
                Some(WritePromptKind::Generate)
            } else {
                Some(WritePromptKind::Refine)
            }
        }
    }
}

pub fn write_prompt_child_ids(tree: &NovelTree, kind: WritePromptKind) -> Vec<String> {
    let Some(hub) = write_prompts_hub(tree) else {
        return vec![];
    };
    let hub_id = hub.id.as_str();
    let mut ids = Vec::new();
    for e in &tree.edges {
        if e.kind != "knowledge" {
            continue;
        }
        let other = if e.source == hub_id {
            e.target.as_str()
        } else if e.target == hub_id {
            e.source.as_str()
        } else {
            continue;
        };
        if other == hub_id {
            continue;
        }
        let Some(n) = tree.nodes.iter().find(|n| n.id == other) else {
            continue;
        };
        if !matches!(n.kind, NodeKind::Knowledge) {
            continue;
        }
        if is_write_prompts_slot(knowledge_slot(n)) {
            continue;
        }
        if tree
            .nodes
            .iter()
            .any(|x| x.id == other && matches!(x.kind, NodeKind::Novel))
        {
            continue;
        }
        if hub_side(tree, hub_id, e) != Some(kind) {
            continue;
        }
        if !ids.iter().any(|id| id == other) {
            ids.push(other.to_string());
        }
    }
    ids
}

pub fn write_prompt_append(tree: &NovelTree, kind: WritePromptKind) -> String {
    let ids = write_prompt_child_ids(tree, kind);
    if ids.is_empty() {
        return String::new();
    }
    let title = match kind {
        WritePromptKind::Generate => "【生成提示】",
        WritePromptKind::Refine => "【精修提示】",
    };
    let mut body = String::new();
    for id in &ids {
        let Some(n) = tree.nodes.iter().find(|n| n.id == *id) else {
            continue;
        };
        let text = crate::core_laws_fmt::knowledge_card_inject_body(tree, n);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&format!("—— {} ——\n{text}", n.label.trim()));
    }
    if body.is_empty() {
        return String::new();
    }
    format!("{title}\n{body}")
}

pub fn merge_user_brief(user_brief: &str, extra: &str) -> String {
    let a = user_brief.trim();
    let b = extra.trim();
    if b.is_empty() {
        return user_brief.to_string();
    }
    if a.is_empty() {
        return extra.to_string();
    }
    format!("{a}\n\n{b}")
}

pub fn write_prompt_kind_from_user(content: &str) -> WritePromptKind {
    if content.contains("精修") {
        WritePromptKind::Refine
    } else {
        WritePromptKind::Generate
    }
}

fn dummy_know(id: &str, slot: &str, extracted: &str, x: f64) -> TreeNode {
    TreeNode {
        id: id.into(),
        kind: NodeKind::Knowledge,
        label: id.into(),
        outline: String::new(),
        detailed_outline: vec![],
        character: None,
        knowledge: Some(KnowledgeCardPayload {
            slot: slot.into(),
            extracted: extracted.into(),
            ..Default::default()
        }),
        side_plot: None,
        volume: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition { x, y: 0.0 },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> TreeNode {
        TreeNode {
            id: "root".into(),
            kind: NodeKind::Novel,
            label: "书".into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: None,
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition { x: 320.0, y: 0.0 },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        }
    }

    #[test]
    fn ensure_is_idempotent() {
        let mut tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![root()],
            edges: vec![],
        };
        assert!(ensure_write_prompts_card(&mut tree));
        assert!(!ensure_write_prompts_card(&mut tree));
        assert_eq!(
            tree.nodes
                .iter()
                .filter(|n| is_write_prompts_slot(knowledge_slot(n)))
                .count(),
            1
        );
        let e = tree.edges.iter().find(|e| e.kind == "knowledge").unwrap();
        assert_eq!(e.source_handle.as_deref(), Some("wp"));
        assert_eq!(e.target_handle.as_deref(), Some("right"));
    }

    #[test]
    fn left_right_split_and_exclude() {
        let mut hub = dummy_know("hub", WRITE_PROMPTS_SLOT, "", 0.0);
        hub.linked_knowledge_ids = vec!["g1".into(), "r1".into()];
        let mut tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                root(),
                hub,
                dummy_know("g1", "", "生成侧正文", -100.0),
                dummy_know("r1", "", "精修侧正文", 100.0),
                dummy_know("other", "", "不该出现", 0.0),
            ],
            edges: vec![
                TreeEdge {
                    id: "e-root-hub".into(),
                    source: "root".into(),
                    target: "hub".into(),
                    kind: "knowledge".into(),
                    source_handle: Some("left".into()),
                    target_handle: Some("right".into()),
                    label: String::new(),
                },
                TreeEdge {
                    id: "e-hub-g".into(),
                    source: "hub".into(),
                    target: "g1".into(),
                    kind: "knowledge".into(),
                    source_handle: Some("left".into()),
                    target_handle: Some("right".into()),
                    label: String::new(),
                },
                TreeEdge {
                    id: "e-hub-r".into(),
                    source: "hub".into(),
                    target: "r1".into(),
                    kind: "knowledge".into(),
                    source_handle: Some("right".into()),
                    target_handle: Some("left".into()),
                    label: String::new(),
                },
            ],
        };
        tree.nodes[0].linked_knowledge_ids = vec!["hub".into(), "other".into()];
        assert!(is_write_prompts_root_edge(&tree, "root", "hub"));
        assert!(!is_write_prompts_root_edge(&tree, "hub", "g1"));
        assert!(is_write_prompt_excluded_id(&tree, "hub"));
        assert!(is_write_prompt_excluded_id(&tree, "g1"));
        assert!(is_write_prompt_excluded_id(&tree, "r1"));
        assert!(!is_write_prompt_excluded_id(&tree, "other"));
        let gen = write_prompt_append(&tree, WritePromptKind::Generate);
        assert!(gen.contains("【生成提示】"), "{gen}");
        assert!(gen.contains("生成侧正文"), "{gen}");
        assert!(!gen.contains("精修侧正文"), "{gen}");
        let rf = write_prompt_append(&tree, WritePromptKind::Refine);
        assert!(rf.contains("【精修提示】"), "{rf}");
        assert!(rf.contains("精修侧正文"), "{rf}");
        assert!(!rf.contains("生成侧正文"), "{rf}");
    }
}
