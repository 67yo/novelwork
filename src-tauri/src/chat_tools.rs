//! Root Chat tools: tree / chapter memory / knowledge (DeepSeek function calling).

use crate::chapter_memory;
use crate::db::Db;
use crate::llm::{self, CompletionResult};
use crate::models::{NodeKind, NovelTree};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const MAX_ROUNDS: usize = 6;
const RESULT_CHARS: usize = 4_000;

pub fn tool_definitions() -> Vec<Value> {
    vec![
        fn_tool(
            "list_tree",
            "List structure-tree nodes (id, kind, label, short outline). Call before editing.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        fn_tool(
            "get_node",
            "Get one node by id or chapter number (n).",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Node uuid" },
                    "n": { "type": "integer", "description": "Chapter number, e.g. 3 for 第3章" }
                },
                "additionalProperties": false
            }),
        ),
        fn_tool(
            "update_node",
            "Update a node's label and/or outline. Use node_id or chapter number n. For root novel, omit n and use node_id of the novel root (or set_root_outline).",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "n": { "type": "integer" },
                    "label": { "type": "string" },
                    "outline": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        fn_tool(
            "set_root_outline",
            "Overwrite the novel root node's overall outline / notes.",
            json!({
                "type": "object",
                "properties": {
                    "outline": { "type": "string" }
                },
                "required": ["outline"],
                "additionalProperties": false
            }),
        ),
        fn_tool(
            "get_chapter_memory",
            "Get stored chapter memory bullets for a chapter (node_id or n).",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "n": { "type": "integer" }
                },
                "additionalProperties": false
            }),
        ),
        fn_tool(
            "search_chapter_memory",
            "Keyword search across this novel's chapter memories.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "description": "Max rows, default 12" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        fn_tool(
            "list_knowledge",
            "List knowledge books (prefers books linked on the novel root).",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        fn_tool(
            "search_knowledge",
            "Keyword search in knowledge-base chunks (linked books first, else all).",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "description": "Max chunks, default 8" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
    ]
}

fn fn_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

pub fn tools_system_note(zh: bool) -> &'static str {
    if zh {
        "\n【工具】你可调用工具查询/修改结构树、章节记忆与知识库：list_tree、get_node、update_node、set_root_outline、get_chapter_memory、search_chapter_memory、list_knowledge、search_knowledge。需要事实或改树时先调工具，再回答用户。删除类操作不要用工具（用斜杠指令）。"
    } else {
        "\n[Tools] You may call: list_tree, get_node, update_node, set_root_outline, get_chapter_memory, search_chapter_memory, list_knowledge, search_knowledge. Use tools before answering when you need facts or tree edits. Do not delete via tools (use slash commands)."
    }
}

pub struct ToolRunResult {
    pub completion: CompletionResult,
    pub tree_dirty: bool,
}

/// Multi-round DeepSeek tool loop for root Chat.
pub async fn run_with_tools(
    settings: &crate::models::AppSettings,
    db: &Db,
    novel_id: &str,
    tree: &mut NovelTree,
    system: &str,
    user: &str,
    model: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
    mut on_tool: impl FnMut(&str),
) -> Result<ToolRunResult> {
    let tools = tool_definitions();
    let mut messages = vec![
        json!({"role": "system", "content": system}),
        json!({"role": "user", "content": user}),
    ];
    let mut tree_dirty = false;
    let mut prompt_tokens = 0u32;
    let mut completion_tokens = 0u32;
    let mut model_name = String::new();
    let mut used_mock = false;

    for _ in 0..MAX_ROUNDS {
        let turn = llm::complete_tool_round(settings, &messages, &tools, model, cancel.clone()).await?;
        model_name = turn.model.clone();
        used_mock = turn.used_mock;
        prompt_tokens = prompt_tokens.saturating_add(turn.prompt_tokens);
        completion_tokens = completion_tokens.saturating_add(turn.completion_tokens);

        if turn.tool_calls.is_empty() {
            return Ok(ToolRunResult {
                completion: CompletionResult {
                    content: turn.content,
                    used_mock,
                    model: model_name,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens.saturating_add(completion_tokens),
                },
                tree_dirty,
            });
        }

        messages.push(json!({
            "role": "assistant",
            "content": turn.content,
            "tool_calls": turn.tool_calls.iter().map(|c| json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments }
            })).collect::<Vec<_>>()
        }));

        for call in &turn.tool_calls {
            on_tool(&call.name);
            let (result, dirty) = execute_tool(db, novel_id, tree, &call.name, &call.arguments);
            if dirty {
                tree_dirty = true;
            }
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": truncate_chars(&result, RESULT_CHARS)
            }));
        }
    }

    Ok(ToolRunResult {
        completion: CompletionResult {
            content: "（工具调用轮次过多，已停止。请缩小问题后重试。）".into(),
            used_mock,
            model: model_name,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
        },
        tree_dirty,
    })
}

fn execute_tool(
    db: &Db,
    novel_id: &str,
    tree: &mut NovelTree,
    name: &str,
    arguments: &str,
) -> (String, bool) {
    let args: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
    match name {
        "list_tree" => (list_tree(tree), false),
        "get_node" => (get_node(tree, &args), false),
        "update_node" => update_node(tree, &args),
        "set_root_outline" => set_root_outline(tree, &args),
        "get_chapter_memory" => (get_chapter_memory(db, novel_id, tree, &args), false),
        "search_chapter_memory" => (search_chapter_memory(db, novel_id, &args), false),
        "list_knowledge" => (list_knowledge(db, tree), false),
        "search_knowledge" => (search_knowledge(db, tree, &args), false),
        _ => (format!("unknown tool: {name}"), false),
    }
}

fn list_tree(tree: &NovelTree) -> String {
    let mut lines = Vec::new();
    for n in &tree.nodes {
        let kind = format!("{:?}", n.kind).to_lowercase();
        let outline: String = n.outline.chars().take(80).collect();
        let outline = if n.outline.chars().count() > 80 {
            format!("{outline}…")
        } else {
            outline
        };
        lines.push(format!(
            "- [{}] {} | {} | words={} | outline={}",
            kind, n.id, n.label, n.word_count, outline
        ));
    }
    if lines.is_empty() {
        "(empty tree)".into()
    } else {
        lines.join("\n")
    }
}

fn resolve_node<'a>(tree: &'a NovelTree, args: &Value) -> Option<&'a crate::models::TreeNode> {
    if let Some(id) = args.get("node_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        return tree.nodes.iter().find(|n| n.id == id);
    }
    if let Some(n) = args.get("n").and_then(|v| v.as_u64()) {
        let n = n as u32;
        return tree.nodes.iter().find(|node| {
            matches!(node.kind, NodeKind::Chapter)
                && crate::commands::chapter_number_from_label(&node.label) == Some(n)
        });
    }
    None
}

fn resolve_node_mut<'a>(
    tree: &'a mut NovelTree,
    args: &Value,
) -> Option<&'a mut crate::models::TreeNode> {
    if let Some(id) = args.get("node_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        return tree.nodes.iter_mut().find(|n| n.id == id);
    }
    if let Some(n) = args.get("n").and_then(|v| v.as_u64()) {
        let n = n as u32;
        return tree.nodes.iter_mut().find(|node| {
            matches!(node.kind, NodeKind::Chapter)
                && crate::commands::chapter_number_from_label(&node.label) == Some(n)
        });
    }
    None
}

fn get_node(tree: &NovelTree, args: &Value) -> String {
    let Some(n) = resolve_node(tree, args) else {
        return "node not found".into();
    };
    let mut v = json!({
        "id": n.id,
        "kind": format!("{:?}", n.kind),
        "label": n.label,
        "outline": n.outline,
        "word_count": n.word_count,
        "linked_character_ids": n.linked_character_ids,
        "linked_side_plot_ids": n.linked_side_plot_ids,
        "linked_knowledge_ids": n.linked_knowledge_ids,
    });
    if let Some(c) = &n.character {
        v["character"] = json!(c);
    }
    v.to_string()
}

fn update_node(tree: &mut NovelTree, args: &Value) -> (String, bool) {
    let label = args.get("label").and_then(|v| v.as_str());
    let outline = args.get("outline").and_then(|v| v.as_str());
    if label.is_none() && outline.is_none() {
        return ("provide label and/or outline".into(), false);
    }
    let Some(n) = resolve_node_mut(tree, args) else {
        return ("node not found".into(), false);
    };
    if matches!(n.kind, NodeKind::Novel) && label.is_some() {
        // root label is novel title elsewhere — allow outline-only preference via set_root_outline
    }
    if let Some(l) = label {
        n.label = l.to_string();
    }
    if let Some(o) = outline {
        n.outline = o.to_string();
    }
    (format!("updated {} ({})", n.id, n.label), true)
}

fn set_root_outline(tree: &mut NovelTree, args: &Value) -> (String, bool) {
    let Some(outline) = args.get("outline").and_then(|v| v.as_str()) else {
        return ("outline required".into(), false);
    };
    let Some(root) = tree.nodes.iter_mut().find(|n| matches!(n.kind, NodeKind::Novel)) else {
        return ("root not found".into(), false);
    };
    root.outline = outline.to_string();
    ("root outline updated".into(), true)
}

fn get_chapter_memory(db: &Db, novel_id: &str, tree: &NovelTree, args: &Value) -> String {
    let Some(n) = resolve_node(tree, args) else {
        return "node not found".into();
    };
    if !matches!(n.kind, NodeKind::Chapter) {
        return "not a chapter node".into();
    }
    match db.list_chapter_memory_for_nodes(novel_id, &[n.id.clone()]) {
        Ok(rows) => {
            if rows.is_empty() {
                "(no memory)".into()
            } else {
                rows.into_iter()
                    .map(|(_, c)| format!("- {c}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        Err(e) => e.to_string(),
    }
}

fn search_chapter_memory(db: &Db, novel_id: &str, args: &Value) -> String {
    let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("").trim();
    if q.is_empty() {
        return "query required".into();
    }
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(crate::kb_context::MEMORY_RETRIEVE_K as u64) as usize;
    let Ok(all) = db.list_chapter_memory(novel_id) else {
        return "db error".into();
    };
    let hits = chapter_memory::rank_memory(&all, q, limit.max(1));
    if hits.is_empty() {
        return "(no hits)".into();
    }
    hits.into_iter()
        .map(|(nid, c)| {
            let snip = crate::kb_context::truncate_chars(&c, crate::kb_context::MEMORY_FACT_CAP);
            format!("[memory:{nid}] {snip}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn linked_or_all_book_ids(db: &Db, tree: &NovelTree) -> Vec<String> {
    let linked = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.linked_knowledge_ids.clone())
        .unwrap_or_default();
    if !linked.is_empty() {
        return linked;
    }
    db.list_knowledge()
        .unwrap_or_default()
        .into_iter()
        .filter(|b| !b.archived)
        .map(|b| b.id)
        .collect()
}

fn list_knowledge(db: &Db, tree: &NovelTree) -> String {
    let ids: std::collections::HashSet<String> =
        linked_or_all_book_ids(db, tree).into_iter().collect();
    let Ok(books) = db.list_knowledge() else {
        return "db error".into();
    };
    let lines: Vec<String> = books
        .into_iter()
        .filter(|b| !b.archived && (ids.is_empty() || ids.contains(&b.id)))
        .map(|b| {
            format!(
                "- {} | {} | author={} | chunks={} | genres={}",
                b.id,
                b.title,
                b.author,
                b.chunk_count,
                b.genres.join(",")
            )
        })
        .collect();
    if lines.is_empty() {
        "(no knowledge books)".into()
    } else {
        lines.join("\n")
    }
}

fn search_knowledge(db: &Db, tree: &NovelTree, args: &Value) -> String {
    let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("").trim();
    if q.is_empty() {
        return "query required".into();
    }
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(crate::kb_context::CANON_RETRIEVE_K as u64) as usize;
    let book_ids = linked_or_all_book_ids(db, tree);
    if book_ids.is_empty() {
        return "(no knowledge books)".into();
    }
    let hits = crate::kb_context::retrieve_knowledge(
        db,
        &book_ids,
        q,
        limit.max(1),
        crate::kb_context::CANON_CHUNK_CAP,
    );
    if hits.is_empty() {
        return "(no hits)".into();
    }
    hits.into_iter()
        .map(|(sid, title, preview)| format!("[{sid}|{title}] {preview}"))
        .collect::<Vec<_>>()
        .join("\n---\n")
}

fn truncate_chars(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_defs_nonempty() {
        assert!(tool_definitions().len() >= 6);
    }
}
