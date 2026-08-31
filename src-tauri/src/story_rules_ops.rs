//! 故事规则：右侧四卡补齐、JSON 落盘、快照。
use crate::models::*;
use crate::story_rules_fmt;
use serde_json::Value;
use uuid::Uuid;

pub const FAN: [(&str, &str); 4] = [
    ("sr_surface_setting", "表层设定"),
    ("sr_story_engine", "故事引擎"),
    ("sr_fulfillment_system", "兑现系统"),
    ("sr_constraint_redlines", "约束红线"),
];

fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

fn find_by_slot<'a>(tree: &'a NovelTree, slot: &str) -> Option<&'a TreeNode> {
    tree.nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == slot)
}

fn find_by_slot_mut<'a>(tree: &'a mut NovelTree, slot: &str) -> Option<&'a mut TreeNode> {
    tree.nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == slot)
}

fn empty_fan_payload(slot: &str) -> KnowledgeCardPayload {
    let mut p = KnowledgeCardPayload {
        slot: slot.into(),
        ..Default::default()
    };
    match slot {
        "sr_surface_setting" => p.surface_setting = Some(SurfaceSettingPayload::default()),
        "sr_story_engine" => p.story_engine = Some(StoryEnginePayload::default()),
        "sr_fulfillment_system" => p.fulfillment_system = Some(FulfillmentSystemPayload::default()),
        "sr_constraint_redlines" => p.constraint_redlines = Some(ConstraintRedlinesPayload::default()),
        _ => {}
    }
    p
}

fn sync_fan_extracted(tree: &mut NovelTree, slot: &str) {
    let body = {
        let Some(n) = find_by_slot(tree, slot) else {
            return;
        };
        story_rules_fmt::format_story_rules_fan_full(tree, n)
    };
    if let Some(n) = find_by_slot_mut(tree, slot) {
        if let Some(k) = n.knowledge.as_mut() {
            k.extracted = body.clone();
        }
        n.outline = body.chars().take(200).collect();
    }
}

/// 同步故事规则父卡 extracted（四子卡汇总）。
pub fn sync_story_rules_parent_extracted(tree: &mut NovelTree) {
    let rules_id = find_by_slot(tree, "story_rules").map(|n| n.id.clone());
    if let Some(id) = rules_id {
        let body = {
            let n = find_by_slot(tree, "story_rules").expect("story_rules");
            story_rules_fmt::format_story_rules_full(tree, n)
        };
        if let Some(n) = find_by_slot_mut(tree, "story_rules") {
            if let Some(k) = n.knowledge.as_mut() {
                k.extracted = body.clone();
            }
            n.outline = body.chars().take(200).collect();
        }
        let _ = id;
    }
}

/// 补齐故事规则右侧四卡；须已有 `story_rules` 主卡。
pub fn ensure_story_rules_fan_cards(tree: &mut NovelTree) -> bool {
    let rules_id = find_by_slot(tree, "story_rules").map(|n| n.id.clone());
    if rules_id.is_none() {
        return false;
    }
    let rules_id = rules_id.unwrap();
    let rules_pos = find_by_slot(tree, "story_rules")
        .map(|n| n.position)
        .unwrap_or(NodePosition { x: 640.0, y: 0.0 });
    let mut changed = false;
    for (slot, title) in FAN {
        if let Some(n) = find_by_slot_mut(tree, slot) {
            if n.label.trim() != title {
                n.label = title.into();
                changed = true;
            }
            if n.knowledge.as_ref().map(|k| k.slot.as_str()) != Some(slot) {
                if let Some(k) = n.knowledge.as_mut() {
                    k.slot = slot.into();
                    changed = true;
                }
            }
            continue;
        }
        let id = format!("kb-{}", Uuid::new_v4());
        let payload = empty_fan_payload(slot);
        tree.nodes.push(TreeNode {
            id: id.clone(),
            kind: NodeKind::Knowledge,
            label: title.into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(payload),
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: rules_pos,
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        });
        sync_fan_extracted(tree, slot);
        if let Some(rules) = tree.nodes.iter_mut().find(|n| n.id == rules_id) {
            if !rules.linked_knowledge_ids.contains(&id) {
                rules.linked_knowledge_ids.push(id.clone());
                changed = true;
            }
        }
        let edge_ok = tree.edges.iter().any(|e| e.source == rules_id && e.target == id);
        if !edge_ok {
            tree.edges.push(TreeEdge {
                id: format!("e-{rules_id}-{id}"),
                source: rules_id.clone(),
                target: id,
                kind: "knowledge".into(),
                source_handle: Some("right".into()),
                target_handle: Some("left".into()),
                label: String::new(),
            });
            changed = true;
        }
    }
    if changed {
        sync_story_rules_parent_extracted(tree);
    }
    changed
}

fn apply_block(
    tree: &mut NovelTree,
    slot: &str,
    key: &str,
    v: &Value,
) -> Result<(), String> {
    if let Some(n) = find_by_slot_mut(tree, slot) {
        if let Some(k) = n.knowledge.as_mut() {
            match key {
                "surface_setting" => {
                    let p: SurfaceSettingPayload =
                        serde_json::from_value(v.clone()).map_err(|e| format!("{key}: {e}"))?;
                    k.surface_setting = Some(p);
                }
                "story_engine" => {
                    let p: StoryEnginePayload =
                        serde_json::from_value(v.clone()).map_err(|e| format!("{key}: {e}"))?;
                    k.story_engine = Some(p);
                }
                "fulfillment_system" => {
                    let p: FulfillmentSystemPayload =
                        serde_json::from_value(v.clone()).map_err(|e| format!("{key}: {e}"))?;
                    k.fulfillment_system = Some(p);
                }
                "constraint_redlines" => {
                    let p: ConstraintRedlinesPayload =
                        serde_json::from_value(v.clone()).map_err(|e| format!("{key}: {e}"))?;
                    k.constraint_redlines = Some(p);
                }
                _ => return Ok(()),
            }
            k.slot = slot.into();
        }
        sync_fan_extracted(tree, slot);
    }
    Ok(())
}

/// 写入四卡 JSON（与 Chat `blocks` 同形）。
pub fn apply_story_rules_payload(tree: &mut NovelTree, blocks: &Value) -> Result<(), String> {
    crate::worldview_ops::ensure_worldview_cards(tree);
    ensure_story_rules_fan_cards(tree);
    if let Some(v) = blocks.get("surface_setting") {
        apply_block(tree, "sr_surface_setting", "surface_setting", v)?;
    }
    if let Some(v) = blocks.get("story_engine") {
        apply_block(tree, "sr_story_engine", "story_engine", v)?;
    }
    if let Some(v) = blocks.get("fulfillment_system") {
        apply_block(tree, "sr_fulfillment_system", "fulfillment_system", v)?;
    }
    if let Some(v) = blocks.get("constraint_redlines") {
        apply_block(tree, "sr_constraint_redlines", "constraint_redlines", v)?;
    }
    sync_story_rules_parent_extracted(tree);
    Ok(())
}

pub fn story_rules_blocks_snapshot(tree: &NovelTree) -> Value {
    let mut obj = serde_json::Map::new();
    let rules = find_by_slot(tree, "story_rules");
    if rules.is_none() {
        return Value::Object(obj);
    }
    let rules = rules.unwrap();
    for child in story_rules_fmt::linked_story_rules_blocks(tree, &rules.id) {
        let slot = story_rules_fmt::knowledge_slot(child);
        let k = child.knowledge.as_ref();
        let val = match slot {
            "sr_surface_setting" => k.and_then(|x| x.surface_setting.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(Value::Object(serde_json::Map::new()))
            }),
            "sr_story_engine" => k.and_then(|x| x.story_engine.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(Value::Object(serde_json::Map::new()))
            }),
            "sr_fulfillment_system" => k.and_then(|x| x.fulfillment_system.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(Value::Object(serde_json::Map::new()))
            }),
            "sr_constraint_redlines" => k.and_then(|x| x.constraint_redlines.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(Value::Object(serde_json::Map::new()))
            }),
            _ => None,
        };
        if let Some(v) = val {
            let key = match slot {
                "sr_surface_setting" => "surface_setting",
                "sr_story_engine" => "story_engine",
                "sr_fulfillment_system" => "fulfillment_system",
                "sr_constraint_redlines" => "constraint_redlines",
                _ => continue,
            };
            obj.insert(key.into(), v);
        }
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_tree() -> NovelTree {
        NovelTree {
            novel_id: "n".into(),
            nodes: vec![TreeNode {
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
                position: NodePosition { x: 0.0, y: 0.0 },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            }],
            edges: vec![],
        }
    }

    #[test]
    fn ensure_fan_four() {
        let mut tree = root_tree();
        assert!(crate::worldview_ops::ensure_worldview_cards(&mut tree));
        for (slot, _) in FAN {
            assert!(find_by_slot(&tree, slot).is_some(), "missing {slot}");
        }
        assert!(!ensure_story_rules_fan_cards(&mut tree));
    }

    #[test]
    fn apply_blocks() {
        let mut tree = root_tree();
        crate::worldview_ops::ensure_worldview_cards(&mut tree);
        ensure_story_rules_fan_cards(&mut tree);
        let blocks = serde_json::json!({
            "surface_setting": {
                "premise": "第三人称",
                "core_conflict": "生存",
                "reader_promise": "爽点",
                "target_audience": "男频",
                "tone_reference": "冷峻",
                "commercial_tags": "悬疑",
                "extended_premise": "扩展"
            }
        });
        apply_story_rules_payload(&mut tree, &blocks).unwrap();
        let n = find_by_slot(&tree, "sr_surface_setting").unwrap();
        assert_eq!(
            n.knowledge.as_ref().unwrap().surface_setting.as_ref().unwrap().premise,
            "第三人称"
        );
    }
}
