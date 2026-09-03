//! Shell-level global chat: rig-agent + MCP tools + skills.

use crate::chapter_memory;
use crate::db::Db;
use crate::kb_context;
use crate::mcp::{self, McpRuntime};
use crate::skills;
use chrono::{Local, Timelike, Utc};

use futures::StreamExt;
use parking_lot::Mutex as ParkingMutex;
use rig_agent::agent::{
    AgentBuilder, AgentHook, HookContext, InvalidToolCallAction, InvalidToolCallContext,
    MultiTurnStreamItem, NoToolConfig,
};
use rig_agent::Agent;
use rig_core::client::CompletionClient;
use rig_core::completion::{CompletionModel, Message};
use rig_core::completion::message::{
    AssistantContent, ToolResult, ToolResultContent, UserContent,
};
use rig_core::memory::{ConversationMemory, MemoryError};
use rig_core::providers::deepseek;
use rig_core::providers::openai::CompletionsClient;
use rig_core::providers::zai;
use rig_core::providers::{
    anthropic, azure, cohere, doubleword, gemini, groq, huggingface, hyperbolic, llamafile, minimax,
    mira, mistral, moonshot, ollama, openai, openrouter, perplexity, together, venice, xai,
    xiaomimimo,
};
use rig_core::streaming::{StreamedAssistantContent, StreamedUserContent};
use rig_core::tool::{PortableDynamicTool, ToolErrorKind, ToolExecutionError, ToolOutput};
use rig_core::vector_store::request::Filter;
use rig_core::vector_store::{TopNResults, VectorSearchRequest, VectorStoreError, VectorStoreIndexDyn};
use rig_core::wasm_compat::WasmBoxedFuture;
use rig_memory::{
    CompactingMemory, HeuristicTokenCounter, TemplateCompactor, TokenWindowMemory,
};
use rmcp::RoleClient;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{Peer, RunningService, ServiceExt};
use rmcp::transport::StreamableHttpClientTransport;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

pub const CANCEL_KEY: &str = "__global_chat__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalChatSendInput {
    pub content: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub route_name: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub path: Option<String>,
    #[serde(default)]
    pub novel_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

pub struct GlobalChatRuntime {
    slots: Arc<ParkingMutex<HashMap<String, ChatSlot>>>,
    busy: AtomicBool,
    memory: Arc<ChatMem>,
}

type ChatMem = CompactingMemory<SlotMemory, TokenWindowMemory, TemplateCompactor>;

struct ChatSlot {
    messages: Vec<GlobalChatMessage>,
    history: Vec<Message>,
}

#[derive(Clone)]
struct SlotMemory {
    slots: Arc<ParkingMutex<HashMap<String, ChatSlot>>>,
}

impl ConversationMemory for SlotMemory {
    fn load<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> WasmBoxedFuture<'a, Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            let mut raw = self
                .slots
                .lock()
                .get(conversation_id)
                .map(|s| s.history.clone())
                .unwrap_or_default();
            let _ = stub_history_tools(&mut raw);
            Ok(raw)
        })
    }

    fn append<'a>(
        &'a self,
        conversation_id: &'a str,
        mut messages: Vec<Message>,
    ) -> WasmBoxedFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let _ = stub_history_tools(&mut messages);
            self.slots
                .lock()
                .entry(conversation_id.to_string())
                .or_insert_with(new_slot)
                .history
                .extend(messages);
            Ok(())
        })
    }

    fn clear<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> WasmBoxedFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            if let Some(slot) = self.slots.lock().get_mut(conversation_id) {
                slot.history.clear();
            }
            Ok(())
        })
    }
}

fn scope_key(novel_id: Option<&str>) -> String {
    novel_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("__global__")
        .to_string()
}

fn new_slot() -> ChatSlot {
    ChatSlot {
        messages: Vec::new(),
        history: Vec::new(),
    }
}

impl GlobalChatRuntime {
    pub fn new() -> Self {
        let slots = Arc::new(ParkingMutex::new(HashMap::new()));
        let memory = Arc::new(CompactingMemory::new(
            SlotMemory {
                slots: slots.clone(),
            },
            TokenWindowMemory::new(8000, HeuristicTokenCounter::default()),
            TemplateCompactor::with_header("【更早轮次】").with_max_bytes(4000),
        ));
        Self {
            slots,
            busy: AtomicBool::new(false),
            memory,
        }
    }

    fn memory(&self) -> Arc<ChatMem> {
        self.memory.clone()
    }

    pub fn list(&self, novel_id: Option<&str>) -> Vec<GlobalChatMessage> {
        let key = scope_key(novel_id);
        self.slots
            .lock()
            .get(&key)
            .map(|s| s.messages.clone())
            .unwrap_or_default()
    }

    pub fn clear(&self, novel_id: Option<&str>) {
        let key = scope_key(novel_id);
        self.slots.lock().insert(key.clone(), new_slot());
        self.memory.forget(&key);
    }
}

fn openai_compat_base(base: &str) -> String {
    base.trim().trim_end_matches('/').to_string()
}

fn extra_chat_params(protocol: &str, intent: &str, novel_id: &str) -> Value {
    if protocol == crate::models::PROTOCOL_OPENAI {
        prompt_cache_params(intent, novel_id)
    } else {
        json!({})
    }
}

fn task_model(preferred: &str, fallback: &str) -> String {
    let p = preferred.trim();
    if p.is_empty() {
        fallback.trim().to_string()
    } else {
        p.to_string()
    }
}

/// 写章：只暴露这 4 个。顺序固定，便于 prompt cache。
const TOOLS_WRITE: &[&str] = &[
    "get_chapter_write_context",
    "generate_detailed_outline",
    "get_chapter_content",
    "set_chapter_content",
];

/// 人物 / 剧情 / 知识卡：不要 get_tree / get_novel_info。
const TOOLS_CARDS: &[&str] = &[
    "get_selected_card",
    "get_character_card",
    "upsert_character_card",
    "upsert_plot_card",
    "upsert_knowledge_card",
    "fill_knowledge_card",
    "link_nodes",
    "unlink_nodes",
];

/// 细纲：材料由 generate_detailed_outline 服务端组装。
const TOOLS_OUTLINE: &[&str] = &[
    "get_selected_card",
    "generate_detailed_outline",
    "regenerate_detailed_outline_item",
    "update_chapter_outline",
];

const TOOLS_SHOTS: &[&str] = &[
    "get_chapter_shots",
    "set_chapter_shots",
    "split_chapter_shots",
    "generate_shot_comfy_prompts",
    "submit_chapter_shots_comfyui",
];

const TOOLS_LIBRARY: &[&str] = &[
    "list_knowledge",
    "import_knowledge",
    "search_knowledge",
    "archive_knowledge",
    "delete_knowledge",
    "list_public_knowledge_cards",
    "upsert_public_knowledge_card",
    "archive_public_knowledge_card",
    "add_public_knowledge_card",
];

const TOOLS_WORLDVIEW_GEN: &[&str] = &["generate_worldview", "generate_story_rules"];

/// 日常 Chat：不含分镜 / 公共库 / 世界观生成。
const TOOLS_CORE: &[&str] = &[
    "list_novels",
    "create_novel",
    "update_novel",
    "get_novel_info",
    "get_worldview",
    "ensure_worldview",
    "apply_worldview",
    "get_story_rules",
    "apply_story_rules",
    "get_selected_card",
    "get_tree",
    "layout_tree",
    "add_volume",
    "get_volume",
    "upsert_volume",
    "add_chapter",
    "update_chapter_outline",
    "generate_detailed_outline",
    "regenerate_detailed_outline_item",
    "delete_node",
    "get_chapter_content",
    "get_chapter_info",
    "get_chapter_write_context",
    "set_chapter_content",
    "get_chapter_memory",
    "list_chapter_memory",
    "set_chapter_memory",
    "regenerate_chapter_memory",
    "get_character_card",
    "upsert_character_card",
    "upsert_plot_card",
    "upsert_knowledge_card",
    "fill_knowledge_card",
    "link_nodes",
    "unlink_nodes",
];

fn is_write_chapter(text: &str) -> bool {
    [
        "生成第",
        "写第",
        "精修第",
        "重写第",
        "改写第",
        "生成本章",
        "写本章",
        "精修本章",
        "本章正文",
        "写这一章",
        "生成这一章",
    ]
    .iter()
    .any(|p| text.contains(p))
}

fn is_write_body(text: &str) -> bool {
    if text.contains("正文") {
        return true;
    }
    [
        "写第",
        "精修第",
        "重写第",
        "改写第",
        "写本章",
        "精修本章",
        "写这一章",
    ]
    .iter()
    .any(|p| text.contains(p))
        || (is_write_chapter(text) && !text.contains("细纲"))
}

fn is_card_edit(text: &str) -> bool {
    [
        "人物卡",
        "角色卡",
        "剧情卡",
        "支线卡",
        "知识卡",
        "改人设",
        "补人设",
        "character card",
        "plot card",
        "knowledge card",
    ]
    .iter()
    .any(|p| text.contains(p))
}

fn is_outline_gen(text: &str) -> bool {
    [
        "生成细纲",
        "写细纲",
        "重写细纲",
        "进化细纲",
        "本章细纲",
        "细纲",
        "detailed outline",
        "detailed_outline",
    ]
    .iter()
    .any(|p| text.contains(p))
}

fn merge_tool_names(a: &[&'static str], b: &[&'static str]) -> Vec<&'static str> {
    let mut out = a.to_vec();
    for n in b {
        if !out.contains(n) {
            out.push(*n);
        }
    }
    out
}

fn wants_shots(text: &str) -> bool {
    ["分镜", "分镜头", "comfy", "Comfy", "minimax", "MiniMax", "镜头提示"]
        .iter()
        .any(|p| text.contains(p))
}

fn wants_library(text: &str) -> bool {
    ["公共库", "公共知识", "知识库", "导入知识"]
        .iter()
        .any(|p| text.contains(p))
}

fn wants_worldview_gen(text: &str) -> bool {
    ["生成世界观", "生成故事规则", "补全世界观", "写世界观"]
        .iter()
        .any(|p| text.contains(p))
}

/// `None` = 不裁剪（全部 MCP 工具）。intent 写入 prompt_cache_key。
fn chat_tool_allowlist(
    explicit_skill: bool,
    text: &str,
) -> (Option<Vec<&'static str>>, &'static str) {
    if explicit_skill {
        return (None, "all");
    }
    let cards = is_card_edit(text);
    let outline = is_outline_gen(text) && !is_write_body(text);
    if outline || (cards && !is_write_body(text)) {
        return match (cards, outline) {
            (true, true) => (
                Some(merge_tool_names(TOOLS_CARDS, TOOLS_OUTLINE)),
                "cards-outline",
            ),
            (true, false) => (Some(TOOLS_CARDS.to_vec()), "cards"),
            _ => (Some(TOOLS_OUTLINE.to_vec()), "outline"),
        };
    }
    if is_write_chapter(text) || is_write_body(text) {
        return (Some(TOOLS_WRITE.to_vec()), "write");
    }
    let shots = wants_shots(text);
    let lib = wants_library(text);
    let wv = wants_worldview_gen(text);
    if !shots && !lib && !wv {
        return (Some(TOOLS_CORE.to_vec()), "core");
    }
    let mut names = TOOLS_CORE.to_vec();
    if shots {
        names.extend_from_slice(TOOLS_SHOTS);
    }
    if lib {
        names.extend_from_slice(TOOLS_LIBRARY);
    }
    if wv {
        names.extend_from_slice(TOOLS_WORLDVIEW_GEN);
    }
    let intent = match (shots, lib, wv) {
        (true, false, false) => "core-shots",
        (false, true, false) => "core-lib",
        (false, false, true) => "core-wv",
        _ => "core-mix",
    };
    (Some(names), intent)
}

/// 裁剪后的工具集必须写进 system，否则模型会按通用「写章用 get_chapter_write_context」去调未下发的工具，上游直接 PromptError。
fn intent_tool_rule(intent: &str) -> &'static str {
    match intent {
        "write" => {
            "\n本轮只开放写章工具。读一次 get_chapter_write_context，细纲空则 generate_detailed_outline，再 set_chapter_content 一次后立刻用一句话结束。禁止反复 get/set 同一章。"
        }
        "outline" | "cards-outline" => {
            "\n本轮只开放细纲工具：generate_detailed_outline / regenerate_detailed_outline_item / update_chapter_outline / get_selected_card。禁止调用 get_chapter_write_context、set_chapter_content、get_chapter_content。细纲完成后用一句话结束，不要写正文。"
        }
        "cards" => {
            "\n本轮只开放改卡工具。禁止调用 get_chapter_write_context、set_chapter_content、get_tree、get_novel_info。"
        }
        _ => "",
    }
}

fn system_instruction(novel_id: &str, novel_title: &str) -> String {
    let plan_and_ask = "\
回答简洁，操作完成后用中文简要说明结果。默认直接做完，不要每次结尾问「要不要继续 / 是否继续 / 下一步吗」。\
仅当缺关键信息或有互斥路径、不选就无法下一步时才提问一次：是否题只问一句；单选列出 1. 2. 3.；多选写明「可多选」并用 1. 2. 3. 列出。\
用户答「是/否」针对上一问：同意则马上用工具落地，并连续做完其余可自主步骤；禁止重复已做过的检索/提炼，也禁止再问同一句或同义确认（含「要不要开始做任务」「是否继续下一项」）。\n\
【大需求 → 任务列表】用户一次给出多项目标、多章写作、长文多段指令，或明显需要 ≥3 个独立 MCP 动作时：\
先拆成编号任务列表（格式「1. …（待办）」；进行中/已完成改括号状态；**禁止**用 `- [ ]` 勾选行），\
再按顺序逐项执行。任务列表是执行计划，默认直接开做，不要先问用户「是否按此执行」。\
本轮能自主完成的尽量连续做完，每完成一项更新该条状态并一句说明。\
仅当某步真正缺关键参数或存在互斥路径时停下提问；人选完后继续未完成项，不要每步都问。小而单一的需求不要强行拆任务。";

    let bound = if novel_id.is_empty() {
        "当前未绑定具体小说。需要改某本书时先 list_novels 或请用户打开工作台。".to_string()
    } else {
        let title = if novel_title.trim().is_empty() {
            novel_id
        } else {
            novel_title.trim()
        };
        format!(
            "当前绑定小说「{title}」(novel_id={novel_id})。默认只操作这本书；用户明确要求时才换书。工作台已选中卡片时可省略 novel_id 与 node_id。"
        )
    };

    format!(
        "你是 Novel Work 的全局助手。必须通过 MCP 工具读写小说、章节、知识库与卡片；不要臆造库里没有的数据。\n\
         {bound}\n\
         写章用 get_chapter_write_context，遵守工具返回的 ai_guidance。node_id 可写树上 id、「第N章」、章号或唯一标题，不必先 get_tree。不要叠 get_novel_info+get_chapter_info。\
         改人物/剧情/知识卡：get_selected_card 或 get_character_card（指定 node_id）再 upsert_*，不要 get_tree/get_novel_info。\
         细纲只调 generate_detailed_outline（材料由工具组装）。公共知识库不是小说设定源。\n\
         {plan_and_ask}"
    )
}

fn restart_mcp(db: &Arc<Db>, mcp: &Arc<McpRuntime>, app: &Arc<Mutex<Option<AppHandle>>>) {
    let s = db.get_settings().unwrap_or_default();
    let ctx = mcp::McpCtx {
        db: db.clone(),
        app: app.clone(),
        selection: mcp.selection.clone(),
    };
    mcp::restart(ctx, mcp.clone(), s.mcp_enabled, s.mcp_port, s.mcp_lan);
}

async fn ensure_mcp_running(
    db: &Arc<Db>,
    mcp: &Arc<McpRuntime>,
    app: &Arc<Mutex<Option<AppHandle>>>,
) -> Result<u16, String> {
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    if !settings.mcp_enabled {
        return Err("请先在设置中启用 MCP 服务".into());
    }
    if !mcp.running.load(Ordering::SeqCst) {
        restart_mcp(db, mcp, app);
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    if !mcp.running.load(Ordering::SeqCst) {
        let err = mcp
            .last_error
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| "MCP 未启动".into());
        return Err(err);
    }
    Ok(mcp.port.load(Ordering::SeqCst))
}

fn prompt_cache_params(intent: &str, novel_id: &str) -> Value {
    let scope = if novel_id.is_empty() { "global" } else { novel_id };
    json!({ "prompt_cache_key": format!("nove-work:chat:{intent}:{scope}") })
}

fn tool_call_key(id: &str, name: &str) -> String {
    if id.is_empty() { name.to_string() } else { id.to_string() }
}

fn dangling_tool_calls(msgs: &[Message]) -> Vec<(String, String)> {
    let mut pending: Vec<(String, String)> = Vec::new();
    for m in msgs {
        match m {
            Message::Assistant { content, .. } => {
                for p in content.iter() {
                    if let AssistantContent::ToolCall(c) = p {
                        pending.push((
                            tool_call_key(c.id.as_str(), &c.function.name),
                            c.function.name.clone(),
                        ));
                    }
                }
            }
            Message::User { content } => {
                for p in content.iter() {
                    if let UserContent::ToolResult(r) = p {
                        let key = r.call.as_str();
                        if let Some(i) = pending.iter().rposition(|(id, _)| id == key) {
                            pending.remove(i);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    pending
}

fn cancelled_tool_msg(call_id: &str) -> Message {
    Message::User {
        content: vec![UserContent::tool_result(
            call_id,
            "cancelled",
            vec![ToolResultContent::json(json!({
                "error": "cancelled",
                "message": "user stopped"
            }))],
        )],
    }
}

fn close_dangling_tools(msgs: &mut Vec<Message>) {
    for (id, _) in dangling_tool_calls(msgs) {
        msgs.push(cancelled_tool_msg(&id));
    }
}

fn is_unmatched_tool_calls(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("insufficient tool messages")
        || m.contains("tool_call_id")
        || (m.contains("tool_calls") && m.contains("must be followed"))
}

fn friendly_chat_err(msg: String) -> String {
    if msg.contains("Max iterations") || msg.contains("MaxTurnsError") || msg.contains("max turns") {
        "模型连续调用工具次数过多，已停止。请再发一次，或把指令写得更具体。".into()
    } else if msg.contains("error decoding response body") {
        "模型返回无法解析。若用的是 DeepSeek 官方 API，请确认地址含 api.deepseek.com 后重试。".into()
    } else {
        msg
    }
}

fn unwrap_tool_json(v: &Value) -> Value {
    if let Some(s) = v.as_str() {
        if let Ok(parsed) = serde_json::from_str::<Value>(s) {
            return unwrap_tool_json(&parsed);
        }
        return v.clone();
    }
    if let Some(out) = v.get("output") {
        return unwrap_tool_json(out);
    }
    v.clone()
}

fn tool_result_value(r: &ToolResult) -> Value {
    match r.content.first() {
        Some(ToolResultContent::Json { value }) => unwrap_tool_json(value),
        Some(ToolResultContent::Text(t)) => serde_json::from_str(&t.text)
            .map(|v| unwrap_tool_json(&v))
            .unwrap_or_else(|_| json!(t.text)),
        _ => Value::Null,
    }
}

fn looks_like_chapter_num(s: &str) -> bool {
    let n = s.chars().count();
    n > 0 && n <= 8 && s.chars().all(|c| c.is_ascii_digit() || "一二三四五六七八九十百千零〇两廿卅".contains(c))
}

fn chapter_num_label(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(i) = rest.find('第') {
        rest = &rest[i + '第'.len_utf8()..];
        let trimmed = rest.trim_start();
        let Some(end) = trimmed.find('章') else { continue };
        let inner = trimmed[..end].trim();
        if looks_like_chapter_num(inner) {
            return Some(inner.to_string());
        }
        rest = trimmed;
    }
    None
}

fn count_body_words(text: &str) -> u32 {
    text.chars().filter(|c| !c.is_whitespace()).count() as u32
}

fn is_write_stub(text: &str) -> bool {
    text.starts_with("已写入")
}

fn chapter_written_stub(content: &str, words: u32) -> String {
    match chapter_num_label(content) {
        Some(n) => format!("已写入第{n}章，约 {words} 字"),
        None => format!("已写入本章，约 {words} 字"),
    }
}

fn is_card_upsert(name: &str) -> bool {
    matches!(
        name,
        "upsert_character_card"
            | "upsert_plot_card"
            | "upsert_knowledge_card"
            | "fill_knowledge_card"
            | "generate_detailed_outline"
            | "regenerate_detailed_outline_item"
            | "update_chapter_outline"
    )
}

fn upsert_args_already_stubbed(args: &Value) -> bool {
    args.get("character").and_then(|v| v.get("_stub")).and_then(|v| v.as_bool()) == Some(true)
        || args.get("outline").and_then(|v| v.as_str()) == Some("（已写入）")
        || args.get("extracted").and_then(|v| v.as_str()) == Some("（已写入）")
}

fn stub_upsert_args(name: &str, args: &mut Value) -> bool {
    if upsert_args_already_stubbed(args) {
        return false;
    }
    let mut changed = false;
    match name {
        "upsert_character_card" => {
            if args.get("character").is_some() {
                args["character"] = json!({ "_stub": true });
                changed = true;
            }
        }
        "upsert_plot_card" => {
            if let Some(o) = args.get("outline").and_then(|v| v.as_str()) {
                if o.chars().count() > 40 {
                    args["outline"] = json!("（已写入）");
                    changed = true;
                }
            }
        }
        "upsert_knowledge_card" => {
            if let Some(o) = args.get("extracted").and_then(|v| v.as_str()) {
                if o.chars().count() > 40 {
                    args["extracted"] = json!("（已写入）");
                    changed = true;
                }
            }
            for key in [
                "core_laws", "spatiotemporal", "social_power", "existence", "info_flow",
                "history_culture", "surface_setting", "story_engine", "fulfillment_system",
                "constraint_redlines",
            ] {
                if args.get(key).map(|v| v.is_object()).unwrap_or(false) {
                    args[key] = json!({ "_stub": true });
                    changed = true;
                }
            }
        }
        _ => {}
    }
    changed
}

fn successful_upsert_keys(msgs: &[Message]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for m in msgs {
        let Message::User { content } = m else { continue };
        for p in content.iter() {
            let UserContent::ToolResult(r) = p else { continue };
            let payload = tool_result_value(r);
            let name = payload.get("tool").and_then(|v| v.as_str()).unwrap_or("");
            // name may not be in result; match by call id against later stub using id only
            if payload.get("error").is_some() {
                continue;
            }
            if payload.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                continue;
            }
            let label = payload.get("label").and_then(|v| v.as_str()).unwrap_or("").trim();
            let stub = if !label.is_empty() {
                format!("已写入{label}")
            } else {
                "已写入".into()
            };
            out.insert(r.call.as_str().to_string(), stub);
            let _ = name;
        }
    }
    out
}

fn successful_write_word_counts(msgs: &[Message]) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for m in msgs {
        let Message::User { content } = m else { continue };
        for p in content.iter() {
            let UserContent::ToolResult(r) = p else { continue };
            let payload = tool_result_value(r);
            let words = payload.get("word_count").and_then(|v| v.as_u64()).map(|n| n as u32);
            let ok = payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if !ok && words.is_none() {
                continue;
            }
            out.insert(r.call.as_str().to_string(), words.unwrap_or(0));
        }
    }
    out
}

fn stub_history_tools(msgs: &mut [Message]) -> bool {
    let word_by_call = successful_write_word_counts(msgs);
    let upsert_by_call = successful_upsert_keys(msgs);
    if word_by_call.is_empty() && upsert_by_call.is_empty() {
        return false;
    }
    let mut changed = false;
    for m in msgs.iter_mut() {
        let Message::Assistant { content, .. } = m else { continue };
        for part in content.iter_mut() {
            let AssistantContent::ToolCall(c) = part else { continue };
            if c.function.name == "set_chapter_content" {
                let Some(&words) = word_by_call.get(c.id.as_str()) else { continue };
                let Some(body) = c.function.arguments.get("content").and_then(|v| v.as_str()) else {
                    continue;
                };
                if is_write_stub(body) {
                    continue;
                }
                let stub = chapter_written_stub(body, words);
                c.function.arguments["content"] = json!(stub);
                changed = true;
                continue;
            }
            if !is_card_upsert(&c.function.name) {
                continue;
            }
            if !upsert_by_call.contains_key(c.id.as_str()) {
                continue;
            }
            if stub_upsert_args(&c.function.name, &mut c.function.arguments) {
                changed = true;
            }
        }
    }
    changed
}

fn prune_history(msgs: &mut Vec<Message>) {
    let _ = stub_history_tools(msgs);
}

struct ChapterMemoryIndex {
    db: Arc<Db>,
    novel_id: String,
}

fn chat_rag_docs(db: &Db, novel_id: &str, query: &str, k: usize) -> Vec<(f64, String, Value)> {
    if novel_id.is_empty() || k == 0 || query.trim().is_empty() {
        return Vec::new();
    }
    let Ok(rows) = db.list_chapter_memory(novel_id) else {
        return Vec::new();
    };
    chapter_memory::rank_memory(&rows, query, k)
        .into_iter()
        .map(|(node_id, content)| {
            let text = kb_context::truncate_chars(&content, kb_context::MEMORY_FACT_CAP);
            (
                1.0,
                format!("chapter-memory:{node_id}"),
                json!({ "text": text }),
            )
        })
        .collect()
}

async fn chat_rag_docs_vec(novel_id: &str, query: &str, k: usize) -> Vec<(f64, String, Value)> {
    match chapter_memory::search_vectors(novel_id, query, k).await {
        Ok(hits) if !hits.is_empty() => hits
            .into_iter()
            .map(|(score, node_id, content)| {
                let text = kb_context::truncate_chars(&content, kb_context::MEMORY_FACT_CAP);
                (
                    score,
                    format!("chapter-memory:{node_id}"),
                    json!({ "text": text }),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl VectorStoreIndexDyn for ChapterMemoryIndex {
    fn top_n<'a>(
        &'a self,
        req: VectorSearchRequest<Filter<Value>>,
    ) -> WasmBoxedFuture<'a, TopNResults> {
        let query = req.query().to_string();
        let k = req.samples() as usize;
        let db = self.db.clone();
        let novel_id = self.novel_id.clone();
        Box::pin(async move {
            let hits = chat_rag_docs_vec(&novel_id, &query, k).await;
            if !hits.is_empty() {
                return Ok(hits);
            }
            Ok(chat_rag_docs(&db, &novel_id, &query, k))
        })
    }

    fn top_n_ids<'a>(
        &'a self,
        req: VectorSearchRequest<Filter<Value>>,
    ) -> WasmBoxedFuture<'a, Result<Vec<(f64, String)>, VectorStoreError>> {
        let query = req.query().to_string();
        let k = req.samples() as usize;
        let db = self.db.clone();
        let novel_id = self.novel_id.clone();
        Box::pin(async move {
            let hits = chat_rag_docs_vec(&novel_id, &query, k).await;
            let docs = if hits.is_empty() {
                chat_rag_docs(&db, &novel_id, &query, k)
            } else {
                hits
            };
            Ok(docs.into_iter().map(|(score, id, _)| (score, id)).collect())
        })
    }
}

fn emit_progress(app: &Arc<Mutex<Option<AppHandle>>>, step: &str) {
    let guard = app.lock().unwrap();
    if let Some(h) = guard.as_ref() {
        let _ = h.emit("global-chat-progress", serde_json::json!({ "step": step }));
    }
}

fn emit_tokens(
    app: &Arc<Mutex<Option<AppHandle>>>,
    prompt_tokens: u32,
    completion_tokens: u32,
    confirmed: bool,
) {
    let guard = app.lock().unwrap();
    if let Some(h) = guard.as_ref() {
        let _ = h.emit(
            "global-chat-tokens",
            serde_json::json!({
                "promptTokens": prompt_tokens,
                "completionTokens": completion_tokens,
                "confirmed": confirmed,
            }),
        );
    }
}

fn next_progress_step(tool_name: Option<&str>, has_tool_result: bool, has_text: bool) -> Option<String> {
    if let Some(name) = tool_name.filter(|n| !n.is_empty()) {
        Some(format!("tool:{name}"))
    } else if has_tool_result {
        Some("thinking".into())
    } else if has_text {
        Some("writing".into())
    } else {
        None
    }
}

fn usage_to_record(prompt_sum: u32, completion_sum: u32, est_prompt: u32, reply_len: usize) -> (u32, u32) {
    let prompt = if prompt_sum > 0 { prompt_sum } else { est_prompt };
    let completion = if completion_sum > 0 {
        completion_sum
    } else {
        (reply_len / 4) as u32
    };
    (prompt, completion)
}

fn record_chat_usage(db: &Db, model: &str, novel_id: &str, prompt: u32, completion: u32) {
    if prompt == 0 && completion == 0 {
        return;
    }
    let now = Local::now();
    let _ = db.add_token_usage(
        &now.format("%Y-%m-%d").to_string(),
        now.hour() as u8,
        model,
        novel_id,
        prompt,
        completion,
        prompt.saturating_add(completion),
    );
}

fn push_msg(runtime: &GlobalChatRuntime, scope: &str, role: &str, content: String) -> GlobalChatMessage {
    let msg = GlobalChatMessage {
        id: Uuid::new_v4().to_string(),
        role: role.into(),
        content,
        created_at: Utc::now().to_rfc3339(),
    };
    let mut slots = runtime.slots.lock();
    let slot = slots.entry(scope.to_string()).or_insert_with(new_slot);
    slot.messages.push(msg.clone());
    msg
}

fn pop_last_user(runtime: &GlobalChatRuntime, scope: &str) {
    let mut slots = runtime.slots.lock();
    if let Some(slot) = slots.get_mut(scope) {
        if slot.messages.last().is_some_and(|m| m.role == "user") {
            slot.messages.pop();
        }
    }
}

fn list_scope(runtime: &GlobalChatRuntime, novel_id: &str) -> Vec<GlobalChatMessage> {
    runtime.list(if novel_id.is_empty() { None } else { Some(novel_id) })
}

fn mcp_portable_tool(peer: Peer<RoleClient>, t: rmcp::model::Tool) -> PortableDynamicTool {
    let name = t.name.to_string();
    let desc = t.description.unwrap_or_default().to_string();
    let params = Value::Object((*t.input_schema).clone());
    let n2 = name.clone();
    PortableDynamicTool::new(name, desc, params, move |args| {
        let peer = peer.clone();
        let n2 = n2.clone();
        Box::pin(async move {
            let obj = match args {
                Value::Object(m) => m,
                _ => Default::default(),
            };
            match peer
                .call_tool(CallToolRequestParams::new(n2.clone()).with_arguments(obj))
                .await
            {
                Ok(r) => Ok(ToolOutput::json(
                    serde_json::to_value(&r).unwrap_or_else(|_| json!({"ok": true})),
                )),
                Err(e) => Err(ToolExecutionError::new(ToolErrorKind::Other, e.to_string())),
            }
        })
    })
}

struct RetryUnknownTool;

impl AgentHook for RetryUnknownTool {
    fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        _event: &InvalidToolCallContext,
    ) -> impl std::future::Future<Output = Option<InvalidToolCallAction>> + rig_core::wasm_compat::WasmCompatSend
    {
        async {
            Some(InvalidToolCallAction::retry(
                "该工具本轮不可用。只调用系统下发的工具，不要编造名称。",
            ))
        }
    }
}

fn assemble_chat_agent(
    model: impl CompletionModel + 'static,
    instruction: &str,
    extra: Value,
    max_turns: usize,
    runtime: &GlobalChatRuntime,
    scope: String,
    novel_id: &str,
    cache_intent: &str,
    db: Arc<Db>,
    tools: Vec<PortableDynamicTool>,
) -> Agent {
    let mut builder = AgentBuilder::new(model)
        .preamble(instruction)
        .additional_params(extra)
        .default_max_turns(max_turns)
        .memory(runtime.memory())
        .conversation(scope)
        .add_hook(RetryUnknownTool);
    if !novel_id.is_empty() && cache_intent != "write" {
        builder = builder.dynamic_context(
            kb_context::MEMORY_RETRIEVE_K,
            ChapterMemoryIndex {
                db,
                novel_id: novel_id.to_string(),
            },
        );
    }
    finish_agent(builder, tools)
}

async fn chat_with<C>(
    client: C,
    api_model: &str,
    instruction: &str,
    extra: Value,
    max_turns: usize,
    runtime: &GlobalChatRuntime,
    scope: String,
    novel_id: &str,
    cache_intent: &str,
    db: Arc<Db>,
    tools: Vec<PortableDynamicTool>,
    prompt: String,
    app: &Arc<Mutex<Option<AppHandle>>>,
    cancel: &AtomicBool,
) -> Result<DriveOut, String>
where
    C: CompletionClient,
    C::CompletionModel: CompletionModel + 'static,
{
    chat_once(
        client.completion_model(api_model),
        instruction,
        extra,
        max_turns,
        runtime,
        scope,
        novel_id,
        cache_intent,
        db,
        tools,
        prompt,
        app,
        cancel,
    )
    .await
}

async fn chat_once(
    model: impl CompletionModel + 'static,
    instruction: &str,
    extra: Value,
    max_turns: usize,
    runtime: &GlobalChatRuntime,
    scope: String,
    novel_id: &str,
    cache_intent: &str,
    db: Arc<Db>,
    tools: Vec<PortableDynamicTool>,
    prompt: String,
    app: &Arc<Mutex<Option<AppHandle>>>,
    cancel: &AtomicBool,
) -> Result<DriveOut, String> {
    let agent = assemble_chat_agent(
        crate::compat_messages::EnsureMessageContent(model),
        instruction,
        extra,
        max_turns,
        runtime,
        scope,
        novel_id,
        cache_intent,
        db,
        tools,
    );
    drive_with_retry(&agent, prompt, app, cancel).await
}

fn finish_agent(builder: AgentBuilder<NoToolConfig>, tools: Vec<PortableDynamicTool>) -> Agent {
    let mut it = tools.into_iter();
    let Some(first) = it.next() else {
        return builder.build();
    };
    let mut b = builder.portable_dynamic_tool(first);
    for t in it {
        b = b.portable_dynamic_tool(t);
    }
    b.build()
}

struct DriveOut {
    reply: String,
    prompt_tokens: u32,
    completion_tokens: u32,
}

async fn drive_chat(
    agent: &Agent,
    prompt: String,
    app: &Arc<Mutex<Option<AppHandle>>>,
    cancel: &AtomicBool,
) -> Result<DriveOut, String> {
    let mut stream = agent
        .runner(prompt.clone())
        .max_invalid_tool_call_retries(2)
        .stream()
        .await;
    let mut reply = String::new();
    let mut last_step = String::new();
    let mut prompt_sum = 0u32;
    let mut completion_sum = 0u32;
    let mut final_output: Option<String> = None;
    while let Some(item) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Err("__cancelled__".into());
        }
        let item = item.map_err(|e| e.to_string())?;
        match item {
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(t)) => {
                reply.push_str(&t.text);
                if let Some(step) = next_progress_step(None, false, true) {
                    if last_step != step {
                        last_step.clone_from(&step);
                        emit_progress(app, &step);
                    }
                }
            }
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                crate::ai_log::write(
                    "chat.tool_call",
                    json!({
                        "name": tool_call.function.name,
                        "call_id": tool_call.id.as_str(),
                        "args": tool_call.function.arguments,
                    }),
                );
                if let Some(step) = next_progress_step(Some(&tool_call.function.name), false, false) {
                    if last_step != step {
                        last_step.clone_from(&step);
                        emit_progress(app, &step);
                    }
                }
            }
            MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult { tool_result, .. }) => {
                crate::ai_log::write(
                    "chat.tool_result",
                    json!({
                        "call_id": tool_result.call.as_str(),
                        "response": tool_result_value(&tool_result),
                    }),
                );
                if let Some(step) = next_progress_step(None, true, false) {
                    if last_step != step {
                        last_step.clone_from(&step);
                        emit_progress(app, &step);
                    }
                }
            }
            MultiTurnStreamItem::CompletionCall(c) => {
                if c.usage.input_tokens > 0 || c.usage.output_tokens > 0 {
                    emit_tokens(
                        app,
                        c.usage.input_tokens as u32,
                        c.usage.output_tokens as u32,
                        true,
                    );
                    prompt_sum = prompt_sum.saturating_add(c.usage.input_tokens as u32);
                    completion_sum = completion_sum.saturating_add(c.usage.output_tokens as u32);
                }
            }
            MultiTurnStreamItem::FinalResponse(r) => {
                if prompt_sum == 0 && r.usage.input_tokens > 0 {
                    prompt_sum = r.usage.input_tokens as u32;
                    completion_sum = r.usage.output_tokens as u32;
                }
                final_output = Some(r.output);
            }
            MultiTurnStreamItem::ModelTurnRetried { .. } => {
                reply.clear();
            }
            _ => {}
        }
    }
    let mut reply = if reply.trim().is_empty() {
        final_output.unwrap_or_else(|| "（无回复）".into())
    } else {
        reply
    };
    if reply.trim().is_empty() {
        reply = "（无回复）".into();
    }
    Ok(DriveOut {
        reply,
        prompt_tokens: prompt_sum,
        completion_tokens: completion_sum,
    })
}

async fn drive_with_retry(
    agent: &Agent,
    prompt: String,
    app: &Arc<Mutex<Option<AppHandle>>>,
    cancel: &AtomicBool,
) -> Result<DriveOut, String> {
    let mut retried = false;
    loop {
        match drive_chat(agent, prompt.clone(), app, cancel).await {
            Ok(o) => return Ok(o),
            Err(e) if e == "__cancelled__" || cancel.load(Ordering::SeqCst) => {
                return Err("__cancelled__".into());
            }
            Err(e) if !retried && is_unmatched_tool_calls(&e) => {
                retried = true;
            }
            Err(e) => return Err(e),
        }
    }
}

pub async fn send(
    db: Arc<Db>,
    mcp: Arc<McpRuntime>,
    app: Arc<Mutex<Option<AppHandle>>>,
    runtime: Arc<GlobalChatRuntime>,
    cancel: Arc<AtomicBool>,
    input: GlobalChatSendInput,
) -> Result<Vec<GlobalChatMessage>, String> {
    if runtime.busy.swap(true, Ordering::SeqCst) {
        return Err("全局 Chat 正在回复中".into());
    }
    let _busy = BusyGuard(&runtime.busy);
    let _run = crate::ai_log::begin_run();

    let content = input.content.trim().to_string();
    if content.is_empty() {
        return Err("消息不能为空".into());
    }

    let port = ensure_mcp_running(&db, &mcp, &app).await?;
    let mut settings = db.get_settings().map_err(|e| e.to_string())?;
    if let Some(m) = input.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        settings.chat_model = m.to_string();
        let _ = db.save_settings(&settings);
    }
    let model_name = task_model(&settings.chat_model, &settings.default_model);
    let ep = settings
        .resolve_compat(&model_name)
        .ok_or_else(|| "未配置可用的 AI API".to_string())?;
    if !ep.is_ready() {
        return Err("未配置可用的 AI API".into());
    }
    let api_model = crate::models::api_model_id(&model_name).to_string();

    let novel_id = input
        .novel_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    let scope = scope_key(if novel_id.is_empty() { None } else { Some(novel_id.as_str()) });
    let novel_title = if novel_id.is_empty() {
        String::new()
    } else {
        db.get_novel(&novel_id).ok().flatten().map(|n| n.title).unwrap_or_default()
    };

    push_msg(&runtime, &scope, "user", content.clone());
    let _unsend = UnsendOnCancel {
        runtime: runtime.clone(),
        scope: scope.clone(),
        cancel: cancel.clone(),
    };

    let skill_hit = skills::maybe_inject_skill(&content);
    let mut llm_user = skill_hit.as_ref().map(|(_, t)| t.clone()).unwrap_or_else(|| content.clone());
    let (allow, cache_intent) = chat_tool_allowlist(skill_hit.is_some(), &content);
    if cache_intent == "write" && !novel_id.is_empty() {
        if let Ok(tree) = crate::commands::get_tree(novel_id.clone()) {
            let extra = crate::write_prompts::write_prompt_append(
                &tree,
                crate::write_prompts::write_prompt_kind_from_user(&content),
            );
            llm_user = crate::write_prompts::merge_user_brief(&llm_user, &extra);
        }
    }

    crate::ai_log::write(
        "chat.start",
        json!({
            "user": content,
            "llm_user": llm_user,
            "model": model_name,
            "api_model": api_model,
            "provider": ep.label,
            "novel_id": novel_id,
            "novel_title": novel_title,
            "allowlist": allow,
            "intent": cache_intent,
        }),
    );

    let mut instruction = system_instruction(&novel_id, &novel_title);
    instruction.push_str(intent_tool_rule(cache_intent));
    let est_prompt = ((instruction.len() + llm_user.len()) / 4) as u32;
    emit_tokens(&app, est_prompt, 0, false);

    let mcp_url = format!("http://127.0.0.1:{port}/mcp");
    let transport = StreamableHttpClientTransport::from_uri(mcp_url);
    let mcp_client: RunningService<RoleClient, ()> = ().serve(transport).await.map_err(|e| {
        crate::ai_log::write("chat.error", json!({ "phase": "mcp", "error": e.to_string() }));
        format!("连接本机 MCP 失败: {e}")
    })?;
    let listed = mcp_client.list_tools(None).await.map_err(|e| {
        crate::ai_log::write("chat.error", json!({ "phase": "mcp", "error": e.to_string() }));
        format!("连接本机 MCP 失败: {e}")
    })?;
    let peer = mcp_client.peer().clone();
    let mut tools = Vec::new();
    for t in listed.tools {
        if let Some(names) = allow.as_ref() {
            if !names.iter().any(|n| *n == t.name.as_ref()) {
                continue;
            }
        }
        tools.push(mcp_portable_tool(peer.clone(), t));
    }

    let base = openai_compat_base(&ep.base_url);
    let max_turns = if cache_intent == "write" { 8 } else { 20 };
    let proto = ep.chat_protocol();
    let extra = extra_chat_params(proto, cache_intent, &novel_id);
    emit_progress(&app, "thinking");
    let key = ep.api_key.as_str();
    macro_rules! run_chat {
        ($client:expr) => {
            chat_with(
                $client,
                &api_model,
                &instruction,
                extra,
                max_turns,
                &runtime,
                scope.clone(),
                &novel_id,
                cache_intent,
                db.clone(),
                tools,
                llm_user,
                &app,
                &cancel,
            )
            .await
        };
    }
    let driven = match proto {
        crate::models::PROTOCOL_DEEPSEEK => run_chat!(deepseek::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_ZAI => run_chat!(zai::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_ANTHROPIC => run_chat!(anthropic::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_GEMINI => run_chat!(gemini::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_GROQ => run_chat!(groq::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_MOONSHOT => run_chat!(moonshot::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_MISTRAL => run_chat!(mistral::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_OPENROUTER => run_chat!(openrouter::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_TOGETHER => run_chat!(together::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_XAI => run_chat!(xai::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_OLLAMA => run_chat!(ollama::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_HYPERBOLIC => run_chat!(hyperbolic::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_HUGGINGFACE => run_chat!(huggingface::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_MINIMAX => run_chat!(minimax::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_MIRA => run_chat!(mira::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_PERPLEXITY => run_chat!(perplexity::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_VENICE => run_chat!(venice::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_COHERE => run_chat!(cohere::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_XIAOMIMIMO => run_chat!(xiaomimimo::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_DOUBLEWORD => run_chat!(doubleword::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_OPENAI_RESPONSES => run_chat!(openai::Client::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_AZURE => run_chat!(azure::Client::builder()
            .api_key(azure::AzureOpenAIAuth::ApiKey(ep.api_key.clone()))
            .azure_endpoint(base.clone())
            .build()
            .map_err(|e| e.to_string())?),
        crate::models::PROTOCOL_LLAMAFILE => {
            run_chat!(llamafile::Client::from_url(&base).map_err(|e| e.to_string())?)
        }
        _ => run_chat!(CompletionsClient::builder()
            .api_key(key)
            .base_url(&base)
            .build()
            .map_err(|e| e.to_string())?),
    };
    let out = match driven {
        Ok(o) => o,
        Err(e) if e == "__cancelled__" || cancel.load(Ordering::SeqCst) => {
            return Ok(list_scope(&runtime, &novel_id));
        }
        Err(e) => {
            crate::ai_log::write("chat.error", json!({ "error": e }));
            return Err(friendly_chat_err(e));
        }
    };

    let (p, c) = usage_to_record(out.prompt_tokens, out.completion_tokens, est_prompt, out.reply.len());
    record_chat_usage(&db, &model_name, &novel_id, p, c);
    crate::ai_log::write(
        "chat.done",
        json!({
            "reply": out.reply,
            "prompt_tokens": p,
            "completion_tokens": c,
        }),
    );
    push_msg(&runtime, &scope, "assistant", out.reply);
    Ok(list_scope(&runtime, &novel_id))
}

struct UnsendOnCancel {
    runtime: Arc<GlobalChatRuntime>,
    scope: String,
    cancel: Arc<AtomicBool>,
}
impl Drop for UnsendOnCancel {
    fn drop(&mut self) {
        if self.cancel.load(Ordering::SeqCst) {
            pop_last_user(&self.runtime, &self.scope);
        }
    }
}

struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::completion::message::{ToolCall, ToolCallId, ToolFunction};

    fn assistant_calls(calls: Vec<ToolCall>) -> Message {
        let parts: Vec<_> = calls.into_iter().map(AssistantContent::ToolCall).collect();
        Message::Assistant {
            id: None,
            content: parts,
        }
    }

    fn tool_res(id: &str, payload: Value) -> Message {
        Message::User {
            content: vec![UserContent::tool_result(
                id,
                "tool",
                vec![ToolResultContent::json(payload)],
            )],
        }
    }

    fn test_call(id: &str, function: ToolFunction) -> ToolCall {
        ToolCall::new(ToolCallId::new_or_mint(id), function)
    }

    #[test]
    fn system_instruction_is_short_and_defers_to_ai_guidance() {
        let unbound = system_instruction("", "");
        let bound = system_instruction("n1", "测试书");
        for s in [&unbound, &bound] {
            assert!(s.contains("get_chapter_write_context"), "{s}");
            assert!(s.contains("ai_guidance"), "{s}");
            assert!(!s.contains('①'), "{s}");
            assert!(!s.contains("include_root"), "{s}");
            assert!(!s.contains("route="), "{s}");
        }
        assert!(bound.contains("测试书"));
        assert!(bound.contains("n1"));
        assert!(unbound.contains("upsert"), "{unbound}");
        assert!(unbound.len() < 2000, "unbound {}", unbound.len());
        assert!(bound.len() < 2200, "bound {}", bound.len());
    }

    #[test]
    fn chat_tool_allowlist_write_core_and_skill() {
        let (write, w_intent) = chat_tool_allowlist(false, "生成第 19 章");
        assert_eq!(w_intent, "write");
        let write = write.expect("write allow");
        assert_eq!(write, TOOLS_WRITE);
        assert!(!write.contains(&"get_tree"));
        let (cards, c_intent) = chat_tool_allowlist(false, "把这张人物卡改一下");
        assert_eq!(c_intent, "cards");
        let cards = cards.expect("cards allow");
        assert_eq!(cards, TOOLS_CARDS);
        let (ol, o_intent) = chat_tool_allowlist(false, "生成本章细纲");
        assert_eq!(o_intent, "outline");
        assert_eq!(ol.expect("outline").as_slice(), TOOLS_OUTLINE);
        let (write_beats, _) = chat_tool_allowlist(false, "按细纲写第3章");
        assert_eq!(write_beats.expect("write").as_slice(), TOOLS_WRITE);
        let (shots, s_intent) = chat_tool_allowlist(false, "给本章拆分镜");
        assert_eq!(s_intent, "core-shots");
        assert!(shots.unwrap().contains(&"split_chapter_shots"));
        let (lib, _) = chat_tool_allowlist(false, "从公共库搜一段");
        assert!(lib.unwrap().contains(&"search_knowledge"));
        let (wv, _) = chat_tool_allowlist(false, "生成世界观");
        assert!(wv.unwrap().contains(&"generate_worldview"));
        assert!(!is_write_chapter("生成世界观"));
        let (all, a_intent) = chat_tool_allowlist(true, "生成第 1 章");
        assert_eq!(a_intent, "all");
        assert!(all.is_none());
    }

    #[test]
    fn intent_tool_rule_forbids_write_tools_on_outline() {
        let o = intent_tool_rule("outline");
        assert!(o.contains("禁止调用 get_chapter_write_context"), "{o}");
        let w = intent_tool_rule("write");
        assert!(w.contains("get_chapter_write_context"), "{w}");
        assert!(!w.contains("禁止调用 get_chapter_write_context"), "{w}");
        assert!(intent_tool_rule("core").is_empty());
    }

    #[test]
    fn prompt_cache_key_is_stable_per_intent_and_novel() {
        let a = prompt_cache_params("write", "n1");
        let b = prompt_cache_params("write", "n1");
        assert_eq!(a, b);
        let c = prompt_cache_params("core", "n1");
        assert_ne!(a, c);
    }

    #[test]
    fn next_progress_prefers_tool_then_thinking_then_writing() {
        assert_eq!(next_progress_step(Some("get_chapter_info"), true, true).as_deref(), Some("tool:get_chapter_info"));
        assert_eq!(next_progress_step(None, true, true).as_deref(), Some("thinking"));
        assert_eq!(next_progress_step(None, false, true).as_deref(), Some("writing"));
        assert_eq!(next_progress_step(None, false, false), None);
        assert_eq!(next_progress_step(Some(""), false, true).as_deref(), Some("writing"));
    }

    #[test]
    fn friendly_err_maps_max_iterations() {
        assert!(friendly_chat_err("Max iterations (8) exceeded".into()).contains("次数过多"));
        assert!(friendly_chat_err("MaxTurnsError: reached max turns limit: 8".into()).contains("次数过多"));
        assert_eq!(friendly_chat_err("other".into()), "other");
    }

    #[test]
    fn usage_to_record_prefers_api_then_estimate() {
        assert_eq!(usage_to_record(10, 4, 99, 400), (10, 4));
        assert_eq!(usage_to_record(0, 0, 80, 12), (80, 3));
        assert_eq!(usage_to_record(0, 7, 80, 100), (80, 7));
    }

    #[test]
    fn openai_compat_base_does_not_append_v1() {
        assert_eq!(openai_compat_base("https://open.bigmodel.cn/api/paas/v4/"), "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(openai_compat_base("https://api.deepseek.com/v1"), "https://api.deepseek.com/v1");
    }

    #[test]
    fn uses_deepseek_api_only_official_host() {
        assert_eq!(
            crate::models::infer_compat_protocol("openai", "https://api.deepseek.com/v1"),
            crate::models::PROTOCOL_DEEPSEEK
        );
        assert_eq!(
            crate::models::infer_compat_protocol("openai", "https://API.DeepSeek.com"),
            crate::models::PROTOCOL_DEEPSEEK
        );
        assert_eq!(
            crate::models::infer_compat_protocol("openai", "https://api.qiniu.com/v1"),
            crate::models::PROTOCOL_OPENAI
        );
        assert_eq!(
            crate::models::infer_compat_protocol(
                "openai",
                "https://dashscope.aliyuncs.com/compatible-mode/v1"
            ),
            crate::models::PROTOCOL_ALIYUN
        );
    }

    #[test]
    fn friendly_err_maps_decode_body() {
        assert!(friendly_chat_err(
            "CompletionError: ProviderError: Http client error: error decoding response body".into()
        )
        .contains("无法解析"));
    }

    #[test]
    fn scope_key_global_vs_novel() {
        assert_eq!(scope_key(None), "__global__");
        assert_eq!(scope_key(Some("")), "__global__");
        assert_eq!(scope_key(Some("abc")), "abc");
    }

    #[test]
    fn dangling_tool_calls_unmatched_then_closed() {
        let call = assistant_calls(vec![test_call(
            "call_1",
            ToolFunction::new("search_knowledge".into(), json!({})),
        )]);
        assert_eq!(dangling_tool_calls(&[call.clone()]), vec![("call_1".into(), "search_knowledge".into())]);
        let done = cancelled_tool_msg("call_1");
        assert!(dangling_tool_calls(&[call, done]).is_empty());
    }

    #[test]
    fn dangling_ignores_already_answered_calls() {
        let call = assistant_calls(vec![
            test_call("c1", ToolFunction::new("a".into(), json!({}))),
            test_call("c2", ToolFunction::new("b".into(), json!({}))),
        ]);
        let result = tool_res("c1", json!({"ok": true}));
        assert_eq!(dangling_tool_calls(&[call, result]), vec![("c2".into(), "b".into())]);
    }

    #[test]
    fn unmatched_tool_error_detects_deepseek_400() {
        assert!(is_unmatched_tool_calls(
            "An assistant message with 'tool_calls' must be followed by tool messages responding to each 'tool_call_id'. (insufficient tool messages following tool_calls message)"
        ));
        assert!(!is_unmatched_tool_calls("rate limit exceeded"));
    }

    #[test]
    fn slots_are_isolated() {
        let r = GlobalChatRuntime::new();
        push_msg(&r, &scope_key(Some("n1")), "user", "a".into());
        push_msg(&r, &scope_key(Some("n2")), "user", "b".into());
        assert_eq!(r.list(Some("n1"))[0].content, "a");
        assert_eq!(r.list(Some("n2"))[0].content, "b");
        assert!(r.list(None).is_empty());
        r.clear(Some("n1"));
        assert!(r.list(Some("n1")).is_empty());
        assert_eq!(r.list(Some("n2")).len(), 1);
    }

    #[test]
    fn pop_last_user_only_drops_trailing_user() {
        let r = GlobalChatRuntime::new();
        let s = scope_key(Some("n1"));
        push_msg(&r, &s, "user", "keep".into());
        push_msg(&r, &s, "assistant", "ok".into());
        push_msg(&r, &s, "user", "abort".into());
        pop_last_user(&r, &s);
        let msgs = r.list(Some("n1"));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "ok");
        pop_last_user(&r, &s);
        assert_eq!(r.list(Some("n1")).len(), 2);
    }

    #[test]
    fn chapter_written_stub_uses_heading_and_word_count() {
        assert_eq!(chapter_written_stub("# 第十九章 夜雨\n\n正文", 16), "已写入第十九章，约 16 字");
        assert_eq!(chapter_written_stub("没有标题的正文abc", 3), "已写入本章，约 3 字");
        assert_eq!(count_body_words("a b\nc"), 3);
    }

    #[test]
    fn stub_history_tools_replaces_successful_write_body() {
        let call = assistant_calls(vec![test_call(
            "call_w",
            ToolFunction::new(
                "set_chapter_content".into(),
                json!({"node_id":"ch1","content":"# 第2章 标题\n\n很长的正文不会进历史"}),
            ),
        )]);
        let result = tool_res("call_w", json!({"output": "{\"ok\":true,\"word_count\":42}"}));
        let mut msgs = vec![call, result];
        assert!(stub_history_tools(&mut msgs));
        let body = match &msgs[0] {
            Message::Assistant { content, .. } => content
                .iter()
                .find_map(|p| match p {
                    AssistantContent::ToolCall(c) if c.function.name == "set_chapter_content" => {
                        c.function.arguments.get("content")?.as_str().map(str::to_string)
                    }
                    _ => None,
                }),
            _ => None,
        }
        .unwrap();
        assert_eq!(body, "已写入第2章，约 42 字");
        assert!(!stub_history_tools(&mut msgs));
    }

    #[test]
    fn stub_history_tools_replaces_successful_character_upsert() {
        let call = assistant_calls(vec![test_call(
            "call_c",
            ToolFunction::new(
                "upsert_character_card".into(),
                json!({"node_id":"char-1","name":"李四","character":{"gender":"男","deep":{"desire":"很长一段不会进历史"}}}),
            ),
        )]);
        let result = tool_res("call_c", json!({"output":"{\"ok\":true,\"label\":\"李四\"}"}));
        let mut msgs = vec![call, result];
        assert!(stub_history_tools(&mut msgs));
        let ch = match &msgs[0] {
            Message::Assistant { content, .. } => content.iter().find_map(|p| match p {
                AssistantContent::ToolCall(c) if c.function.name == "upsert_character_card" => {
                    c.function.arguments.get("character").cloned()
                }
                _ => None,
            }),
            _ => None,
        }
        .unwrap();
        assert_eq!(ch, json!({ "_stub": true }));
    }

    #[test]
    fn token_window_keeps_recent_under_budget() {
        use rig_memory::MemoryPolicy;
        let policy = TokenWindowMemory::new(2, |_: &Message| 1);
        let msgs = vec![
            Message::user("第一轮"),
            Message::assistant("a1"),
            Message::user("第二轮"),
            Message::assistant("a2"),
        ];
        let kept = policy.apply(msgs).expect("policy");
        assert_eq!(kept.len(), 2);
        let texts: Vec<String> = kept
            .iter()
            .map(|m| match m {
                Message::User { content } => content
                    .iter()
                    .filter_map(|c| match c {
                        UserContent::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect(),
                Message::Assistant { content, .. } => content
                    .iter()
                    .filter_map(|c| match c {
                        AssistantContent::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect(),
                Message::System { content } => content.clone(),
            })
            .collect();
        assert_eq!(texts, vec!["第二轮".to_string(), "a2".to_string()]);
    }

    #[test]
    fn close_dangling_appends_cancelled_results() {
        let mut msgs = vec![assistant_calls(vec![test_call(
            "call_9",
            ToolFunction::new("search_knowledge".into(), json!({})),
        )])];
        close_dangling_tools(&mut msgs);
        assert!(dangling_tool_calls(&msgs).is_empty());
    }

    #[test]
    fn prune_history_stubs_set_chapter_content() {
        let mut msgs = vec![
            assistant_calls(vec![test_call(
                "call_w",
                ToolFunction::new("set_chapter_content".into(), json!({"content":"# 第3章\n长正文"})),
            )]),
            tool_res("call_w", json!({"output":"{\"ok\":true,\"word_count\":9}"})),
        ];
        prune_history(&mut msgs);
        let body = match &msgs[0] {
            Message::Assistant { content, .. } => content.iter().find_map(|p| match p {
                AssistantContent::ToolCall(c) => c.function.arguments.get("content")?.as_str().map(str::to_string),
                _ => None,
            }),
            _ => None,
        }
        .unwrap();
        assert_eq!(body, "已写入第3章，约 9 字");
    }
}
