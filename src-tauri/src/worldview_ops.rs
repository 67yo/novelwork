//! 根节点世界观：补齐六卡 + 故事规则，并把 LLM JSON 写入树上（子项先内嵌，注入可读）。
use crate::core_laws_fmt;
use crate::existence_fmt;
use crate::history_culture_fmt;
use crate::info_flow_fmt;
use crate::models::*;
use crate::social_power_fmt;
use crate::spatiotemporal_fmt;
use serde_json::Value;
use uuid::Uuid;

const FAN: &[(&str, &str)] = &[
    ("wv_core_laws", "核心法则"),
    ("wv_spatiotemporal", "时空地理"),
    ("wv_social_power", "社会权力"),
    ("wv_history_culture", "历史文化"),
    ("wv_existence", "存在基础"),
    ("wv_info_flow", "信息传播"),
];
const STORY_RULES: (&str, &str) = ("story_rules", "故事规则");

/// `wv_core_laws` 或 `core_laws` → `core_laws`。非六卡返回 None。
pub fn worldview_json_key_for_slot(slot: &str) -> Option<&'static str> {
    let s = slot.trim();
    if s.is_empty() {
        return None;
    }
    for (fan_slot, _) in FAN {
        let json_key = fan_slot.strip_prefix("wv_").unwrap_or(fan_slot);
        if s == *fan_slot || s == json_key {
            return Some(json_key);
        }
    }
    None
}

/// 只保留 worldview 里的一张卡，避免模型漏写其它键时覆盖整套。
pub fn keep_worldview_slot(wv: Value, json_key: &str) -> Value {
    match wv {
        Value::Object(mut map) => {
            if let Some(v) = map.remove(json_key) {
                serde_json::json!({ json_key: v })
            } else {
                serde_json::json!({ json_key: Value::Object(map) })
            }
        }
        other => serde_json::json!({ json_key: other }),
    }
}

fn empty_payload(slot: &str) -> KnowledgeCardPayload {
    KnowledgeCardPayload {
        book_ids: vec![],
        extract_prompt: String::new(),
        extracted: String::new(),
        from_canon: false,
        slot: slot.into(),
        core_laws: if slot == "wv_core_laws" {
            Some(CoreLawsPayload::default())
        } else {
            None
        },
        spatiotemporal: if slot == "wv_spatiotemporal" {
            Some(SpatiotemporalPayload::default())
        } else {
            None
        },
        social_power: if slot == "wv_social_power" {
            Some(SocialPowerPayload::default())
        } else {
            None
        },
        existence: if slot == "wv_existence" {
            Some(ExistencePayload::default())
        } else {
            None
        },
        info_flow: if slot == "wv_info_flow" {
            Some(InfoFlowPayload::default())
        } else {
            None
        },
        history_culture: if slot == "wv_history_culture" {
            Some(HistoryCulturePayload::default())
        } else {
            None
        },
        world_axiom: None,
        key_location: None,
        world_race: None,
        major_faction: None,
        world_religion: None,
        major_event: None,
        surface_setting: None,
        story_engine: None,
        fulfillment_system: None,
        constraint_redlines: None,
    }
}

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

/// 补齐根上世界观六卡 + 故事规则；已有槽位不改内容。返回是否新建了节点。
pub fn ensure_worldview_cards(tree: &mut NovelTree) -> bool {
    let root_id = match tree.nodes.iter().find(|n| matches!(n.kind, NodeKind::Novel)) {
        Some(r) => r.id.clone(),
        None => return false,
    };
    let root_pos = tree
        .nodes
        .iter()
        .find(|n| n.id == root_id)
        .map(|n| n.position)
        .unwrap_or(NodePosition { x: 280.0, y: 0.0 });

    let mut changed = false;
    let mut defs: Vec<(&str, &str)> = FAN.to_vec();
    defs.push(STORY_RULES);

    for (slot, title) in defs {
        if find_by_slot(tree, slot).is_some() {
            continue;
        }
        let id = format!("kb-{}", Uuid::new_v4());
        tree.nodes.push(TreeNode {
            id: id.clone(),
            kind: NodeKind::Knowledge,
            label: title.into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(empty_payload(slot)),
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
        let edge_exists = tree.edges.iter().any(|e| {
            (e.source == root_id && e.target == id) || (e.source == id && e.target == root_id)
        });
        if !edge_exists {
            let (sh, th) = if slot == STORY_RULES.0 {
                ("right", "left")
            } else {
                ("top", "bottom")
            };
            tree.edges.push(TreeEdge {
                id: format!("e-{root_id}-{id}"),
                source: root_id.clone(),
                target: id,
                kind: "knowledge".into(),
                source_handle: Some(sh.into()),
                target_handle: Some(th.into()),
                label: String::new(),
            });
        }
        changed = true;
    }
    let fan = crate::story_rules_ops::ensure_story_rules_fan_cards(tree);
    changed || fan
}

fn sync_extracted(tree: &mut NovelTree) {
    // collect ids first to avoid borrow issues
    let slots: Vec<String> = FAN.iter().map(|(s, _)| (*s).to_string()).collect();
    for slot in slots {
        let body = {
            let Some(n) = find_by_slot(tree, &slot) else {
                continue;
            };
            match slot.as_str() {
                "wv_core_laws" => core_laws_fmt::format_core_laws_full(tree, n),
                "wv_spatiotemporal" => spatiotemporal_fmt::format_spatiotemporal_full(tree, n),
                "wv_social_power" => social_power_fmt::format_social_power_full(tree, n),
                "wv_existence" => existence_fmt::format_existence_full(n),
                "wv_info_flow" => info_flow_fmt::format_info_flow_full(n),
                "wv_history_culture" => history_culture_fmt::format_history_culture_full(tree, n),
                _ => continue,
            }
        };
        if let Some(n) = find_by_slot_mut(tree, &slot) {
            if let Some(k) = n.knowledge.as_mut() {
                k.extracted = body.clone();
            }
            n.outline = body.chars().take(200).collect();
        }
    }
}

/// 将 worldview JSON 写入树上各槽（子项内嵌进宿主卡；写作注入可读）。
pub fn apply_worldview_payload(tree: &mut NovelTree, worldview: &Value) -> Result<(), String> {
    ensure_worldview_cards(tree);
    if let Some(v) = worldview.get("core_laws") {
        let payload: CoreLawsPayload =
            serde_json::from_value(v.clone()).map_err(|e| format!("core_laws: {e}"))?;
        if let Some(n) = find_by_slot_mut(tree, "wv_core_laws") {
            if let Some(k) = n.knowledge.as_mut() {
                k.core_laws = Some(payload);
            }
        }
    }
    if let Some(v) = worldview.get("spatiotemporal") {
        let payload: SpatiotemporalPayload =
            serde_json::from_value(v.clone()).map_err(|e| format!("spatiotemporal: {e}"))?;
        if let Some(n) = find_by_slot_mut(tree, "wv_spatiotemporal") {
            if let Some(k) = n.knowledge.as_mut() {
                k.spatiotemporal = Some(payload);
            }
        }
    }
    if let Some(v) = worldview.get("social_power") {
        let payload: SocialPowerPayload =
            serde_json::from_value(v.clone()).map_err(|e| format!("social_power: {e}"))?;
        if let Some(n) = find_by_slot_mut(tree, "wv_social_power") {
            if let Some(k) = n.knowledge.as_mut() {
                k.social_power = Some(payload);
            }
        }
    }
    if let Some(v) = worldview.get("existence") {
        let payload: ExistencePayload =
            serde_json::from_value(v.clone()).map_err(|e| format!("existence: {e}"))?;
        if let Some(n) = find_by_slot_mut(tree, "wv_existence") {
            if let Some(k) = n.knowledge.as_mut() {
                k.existence = Some(payload);
            }
        }
    }
    if let Some(v) = worldview.get("info_flow") {
        let payload: InfoFlowPayload =
            serde_json::from_value(v.clone()).map_err(|e| format!("info_flow: {e}"))?;
        if let Some(n) = find_by_slot_mut(tree, "wv_info_flow") {
            if let Some(k) = n.knowledge.as_mut() {
                k.info_flow = Some(payload);
            }
        }
    }
    if let Some(v) = worldview.get("history_culture") {
        // 兼容旧 {extracted} only
        if v.get("premise").is_some()
            || v.get("customs").is_some()
            || v.get("religions").is_some()
            || v.get("major_events").is_some()
        {
            let payload: HistoryCulturePayload =
                serde_json::from_value(v.clone()).map_err(|e| format!("history_culture: {e}"))?;
            if let Some(n) = find_by_slot_mut(tree, "wv_history_culture") {
                if let Some(k) = n.knowledge.as_mut() {
                    k.history_culture = Some(payload);
                }
            }
        } else if let Some(ex) = v.get("extracted").and_then(|x| x.as_str()) {
            if let Some(n) = find_by_slot_mut(tree, "wv_history_culture") {
                if let Some(k) = n.knowledge.as_mut() {
                    k.extracted = ex.to_string();
                    n.outline = ex.chars().take(200).collect();
                }
            }
        }
    }
    if let Some(ex) = worldview
        .get("story_rules")
        .and_then(|s| s.get("extracted"))
        .and_then(|x| x.as_str())
    {
        if let Some(n) = find_by_slot_mut(tree, "story_rules") {
            if let Some(k) = n.knowledge.as_mut() {
                k.extracted = ex.to_string();
            }
            n.outline = ex.chars().take(200).collect();
        }
    }
    if let Some(blocks) = worldview.get("story_rules_blocks") {
        crate::story_rules_ops::apply_story_rules_payload(tree, blocks)?;
    }
    sync_extracted(tree);
    crate::story_rules_ops::sync_story_rules_parent_extracted(tree);
    crate::tree_layout::apply_auto_layout(tree);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_and_apply_inline() {
        let mut tree = NovelTree {
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
        };
        assert!(ensure_worldview_cards(&mut tree));
        assert_eq!(
            tree.nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Knowledge))
                .count(),
            11
        );
        let wv = serde_json::json!({
            "core_laws": {
                "premise": "力量有价",
                "taboos": ["禁无代价"],
                "power_system": "九境",
                "power_expression": "纹",
                "axioms": [{"name": "借力","statement":"借力必还","boundary":"","cost":"","mechanism":""}]
            },
            "story_rules": {"extracted": "禁止说教"}
        });
        apply_worldview_payload(&mut tree, &wv).unwrap();
        let core = find_by_slot(&tree, "wv_core_laws").unwrap();
        let body = core.knowledge.as_ref().unwrap().extracted.clone();
        assert!(body.contains("力量有价"));
        assert!(body.contains("借力"));
        let rules = find_by_slot(&tree, "story_rules").unwrap();
        assert_eq!(
            rules.knowledge.as_ref().unwrap().extracted.trim(),
            "禁止说教"
        );

        apply_worldview_payload(
            &mut tree,
            &serde_json::json!({
                "existence": {
                    "premise": "死亡不可逆",
                    "death": "魂散",
                    "calendar": "帝国历",
                    "lifespan": "80",
                    "disease_reproduction": "雾季瘟疫"
                }
            }),
        )
        .unwrap();
        let core2 = find_by_slot(&tree, "wv_core_laws").unwrap();
        assert!(
            core2
                .knowledge
                .as_ref()
                .unwrap()
                .extracted
                .contains("力量有价"),
            "slot-only apply must not wipe other cards"
        );
        let ex = find_by_slot(&tree, "wv_existence").unwrap();
        assert!(
            ex.knowledge
                .as_ref()
                .unwrap()
                .extracted
                .contains("死亡不可逆")
        );
    }

    #[test]
    fn slot_key_and_keep_filter() {
        assert_eq!(worldview_json_key_for_slot("wv_existence"), Some("existence"));
        assert_eq!(worldview_json_key_for_slot("core_laws"), Some("core_laws"));
        assert_eq!(worldview_json_key_for_slot("story_rules"), None);
        let kept = keep_worldview_slot(
            serde_json::json!({
                "existence": {"premise": "a"},
                "core_laws": {"premise": "b"}
            }),
            "existence",
        );
        assert_eq!(kept["existence"]["premise"], "a");
        assert!(kept.get("core_laws").is_none());
        let wrapped = keep_worldview_slot(
            serde_json::json!({"premise": "x", "death": "y"}),
            "existence",
        );
        assert_eq!(wrapped["existence"]["premise"], "x");
    }
}
