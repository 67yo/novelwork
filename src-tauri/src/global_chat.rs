//! Shell-level global chat: ADK LlmAgent + skills + MCP HTTP toolset.

use crate::db::Db;
use crate::mcp::{self, McpRuntime};
use crate::skills;
use adk_rust::agent::LlmAgentBuilder;
use adk_rust::model::openai_compatible::{OpenAICompatible, OpenAICompatibleConfig};
use adk_rust::FunctionResponseData;
use adk_rust::prelude::{Content, Event, InMemorySessionService, Part, Runner};
use adk_rust::session::{CreateRequest, GetRequest, SessionService};
use adk_rust::tool::McpHttpClientBuilder;
use chrono::{Local, Timelike, Utc};
use futures::StreamExt;
use parking_lot::Mutex as ParkingMutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const APP_NAME: &str = "nove-work";
const USER_ID: &str = "local-user";
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
    pub route_name: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub novel_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

pub struct GlobalChatRuntime {
    slots: ParkingMutex<HashMap<String, ChatSlot>>,
    sessions: Arc<InMemorySessionService>,
    busy: AtomicBool,
}

struct ChatSlot {
    messages: Vec<GlobalChatMessage>,
    session_id: String,
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
        session_id: Uuid::new_v4().to_string(),
    }
}

impl GlobalChatRuntime {
    pub fn new() -> Self {
        Self {
            slots: ParkingMutex::new(HashMap::new()),
            sessions: Arc::new(InMemorySessionService::new()),
            busy: AtomicBool::new(false),
        }
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
        self.slots.lock().insert(scope_key(novel_id), new_slot());
    }
}

fn openai_compat_base(base: &str) -> String {
    base.trim().trim_end_matches('/').to_string()
}

fn task_model(preferred: &str, fallback: &str) -> String {
    let p = preferred.trim();
    if p.is_empty() {
        fallback.trim().to_string()
    } else {
        p.to_string()
    }
}

fn system_instruction(route_name: &str, path: &str, novel_id: &str, novel_title: &str) -> String {
    let plan_and_ask = "\
回答简洁，操作完成后用中文简要说明结果。默认直接做完，不要每次结尾问「要不要继续 / 是否继续 / 下一步吗」。\
仅当缺关键信息或有互斥路径、不选就无法下一步时才提问一次：是否题只问一句；单选列出 1. 2. 3.；多选写明「可多选」并用 1. 2. 3. 列出。\
用户答「是/否」针对上一问：同意则马上用工具落地，并连续做完其余可自主步骤；禁止重复已做过的检索/提炼，也禁止再问同一句或同义确认（含「要不要开始做任务」「是否继续下一项」）。\n\
【大需求 → 任务列表】用户一次给出多项目标、多章写作、长文多段指令，或明显需要 ≥3 个独立 MCP 动作时：\
先拆成编号任务列表（格式「1. …（待办）」；进行中/已完成改括号状态；**禁止**用 `- [ ]` 勾选行），\
再按顺序逐项执行。任务列表是执行计划，默认直接开做，不要先问用户「是否按此执行」。\
本轮能自主完成的尽量连续做完，每完成一项更新该条状态并一句说明。\
仅当某步真正缺关键参数或存在互斥路径时停下提问；人选完后继续未完成项，不要每步都问。小而单一的需求不要强行拆任务。";

    let chapter_body_write = "\
【生成 / 精修第 N 章正文】用户说「生成第 N 章」「写第 N 章内容」「精修第 N 章」「重写第 N 章」「改写第 N 章正文」等时，必须按下列流程（Chat 自写 + set_chapter_content；工作台正文栏已无生成/精修按钮，写章一律走本流程）：\n\
① 必须先调 `get_novel_info`（记 `word_count_min`/`word_count_max`）与 `get_chapter_info`（完整读 `ai_guidance`、`node.outline` 简纲、`node.detailed_outline` 细纲与全部 `linked_*`）。\
若 N 不明或尚无该章节点：用 `get_tree` 按 label/顺序定位章节 `node_id`，缺章则 `add_chapter` 后再 `get_chapter_info`。\n\
② **生成新正文**（用户说生成/写/预写，且未强调在旧稿上改）：若 `detailed_outline` 为空或不存在，**必须先** `generate_detailed_outline`（由简纲进化细纲并写回）；再遵守 `content_usage`——**禁止** `get_chapter_content`，禁止参考磁盘旧稿，**按细纲扩充**写全新正文。\n\
③ **精修 / 改稿已有正文**（用户说精修/重写/润色且章内已有正文，可带自定义提示词）：`get_chapter_info` 后**必须**再 `get_chapter_content` 读旧稿，在旧稿骨架上改，禁止无视旧稿另起炉灶；用户提示词须遵守但仍不大改主线。\n\
④ 正文须：`detailed_outline`（若有）每条落地，否则 `outline` 简纲节拍全覆盖；`linked_plots` 每条（根/卷/本章按各自 order 与 inherited_from）要点落地；`linked_characters` 每人须出场且言行合人设；`linked_knowledge`（含世界观/故事规则若挂在根上）的 extracted 硬约束全遵守。\n\
⑤ **落盘前自检**（内部完成，不要把清单当正文输出）：细纲/简纲节拍是否都写了？根/卷/本章剧情是否按 order 落地？链接人物是否都出场且人设一致？知识卡 extracted 是否遵守？正文非空白字数是否在 `word_count_min`–`word_count_max` 的 ±60 字有效区间内？\
任一项明显不达标须先改正文再 `set_chapter_content`。\n\
⑥ 仅 `set_chapter_content` 写入 Markdown 正文；完成后用一两句说明章号、约多少字、是否精修/新生成。不要输出写作过程或自我评分清单。";

    if novel_id.is_empty() {
        format!(
            "你是 Novel Work 的全局助手。必须通过 MCP 工具读写小说、章节、知识库与卡片；不要臆造库里没有的数据。\n\
             当前未绑定具体小说（route={route_name} path={path}）。需要改某本书时先 list_novels 或请用户打开工作台。\n\
             公共知识库不是小说设定源；只有挂到树上的知识卡才约束写作。不要声称小说「绑定了」某本公共库。\n\
             公共知识卡目录（跨小说）：list_public_knowledge_cards / upsert_public_knowledge_card；挂到某本小说用 add_public_knowledge_card（需 novel_id 或打开工作台）。\n\
             {chapter_body_write}\n\
             {plan_and_ask}"
        )
    } else {
        let title = if novel_title.trim().is_empty() {
            novel_id
        } else {
            novel_title.trim()
        };
        format!(
            "你是 Novel Work 的全局助手。必须通过 MCP 工具读写小说、章节、知识库与卡片；不要臆造库里没有的数据。\n\
             当前绑定小说「{title}」(novel_id={novel_id})。默认只操作这本书；用户明确要求时才换书。\n\
             工作台已选中卡片时，get_novel_info / get_chapter_info / set_chapter_content / fill_knowledge_card 等可省略 novel_id 与 node_id，服务端用当前选中。\n\
             知识卡：完整导入 fill_knowledge_card；AI 提炼 search_knowledge 最多一轮再 upsert_knowledge_card。用户同意写成知识卡后立刻 upsert（新建默认挂根），禁止再检索。\n\
             跨小说共用：list_public_knowledge_cards / upsert_public_knowledge_card（目录）/ add_public_knowledge_card（复制到当前树并挂选中节点）。不是每张树上知识卡都要进目录。\n\
             公共知识库不是小说设定源；只有挂到树上的知识卡才约束写作。不要声称小说「绑定了」某本公共库。\n\
             了解全书先 get_novel_info；写章先 get_chapter_info（无正文，保持生成条件干净）；精修/改稿时再 get_chapter_content 读旧稿。须完整遵守 ai_guidance（含 content_usage）。用户说「这张卡 / 当前选中」时用 get_selected_card。\n\
             {chapter_body_write}\n\
             当前 UI：route={route_name} path={path}\n\
             {plan_and_ask}"
        )
    }
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

async fn ensure_session(
    sessions: &Arc<InMemorySessionService>,
    session_id: &str,
) -> Result<(), String> {
    let exists = sessions
        .get(GetRequest {
            app_name: APP_NAME.into(),
            user_id: USER_ID.into(),
            session_id: session_id.into(),
            num_recent_events: None,
            after: None,
        })
        .await
        .is_ok();
    if exists {
        return Ok(());
    }
    sessions
        .create(CreateRequest {
            app_name: APP_NAME.into(),
            user_id: USER_ID.into(),
            session_id: Some(session_id.into()),
            state: HashMap::<String, Value>::new(),
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn tool_call_key(call_id: Option<&str>, name: &str) -> String {
    call_id.filter(|s| !s.is_empty()).unwrap_or(name).to_string()
}

fn dangling_tool_calls(events: &[Event]) -> Vec<(String, String)> {
    let mut pending: Vec<(String, String)> = Vec::new();
    for ev in events {
        for c in ev.tool_calls() {
            pending.push((tool_call_key(c.call_id, c.name), c.name.to_string()));
        }
        for r in ev.tool_results() {
            let key = tool_call_key(r.call_id, r.name);
            if let Some(i) = pending.iter().rposition(|(id, _)| id == &key) {
                pending.remove(i);
            }
        }
    }
    pending
}

fn cancelled_tool_event(call_id: &str, name: &str) -> Event {
    let mut event = Event::new("cancelled-tools");
    event.author = "global_chat".into();
    event.set_content(Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            function_response: FunctionResponseData::new(
                name,
                serde_json::json!({
                    "error": "cancelled",
                    "message": "user stopped"
                }),
            ),
            id: Some(call_id.to_string()),
            annotations: None,
        }],
    });
    event
}

fn is_unmatched_tool_calls(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("insufficient tool messages")
        || m.contains("tool_call_id")
        || (m.contains("tool_calls") && m.contains("must be followed"))
}

async fn close_dangling_tools(
    sessions: &Arc<InMemorySessionService>,
    session_id: &str,
) -> Result<(), String> {
    let session = match sessions
        .get(GetRequest {
            app_name: APP_NAME.into(),
            user_id: USER_ID.into(),
            session_id: session_id.into(),
            num_recent_events: None,
            after: None,
        })
        .await
    {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    for (id, name) in dangling_tool_calls(&session.events().all()) {
        sessions
            .append_event(session_id, cancelled_tool_event(&id, &name))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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

fn next_progress_step(
    tool_name: Option<&str>,
    has_tool_result: bool,
    has_text: bool,
) -> Option<String> {
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

fn usage_to_record(
    prompt_sum: u32,
    completion_sum: u32,
    est_prompt: u32,
    reply_len: usize,
) -> (u32, u32) {
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

fn push_msg(
    runtime: &GlobalChatRuntime,
    scope: &str,
    role: &str,
    content: String,
) -> GlobalChatMessage {
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
    runtime.list(if novel_id.is_empty() {
        None
    } else {
        Some(novel_id)
    })
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
        .ok_or_else(|| "未配置可用的 OpenAI 兼容 API Key".to_string())?;
    if ep.api_key.trim().is_empty() || ep.base_url.trim().is_empty() {
        return Err("未配置可用的 OpenAI 兼容 API Key".into());
    }
    let api_model = crate::models::api_model_id(&model_name).to_string();

    let novel_id = input
        .novel_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    let scope = scope_key(if novel_id.is_empty() {
        None
    } else {
        Some(novel_id.as_str())
    });
    let novel_title = if novel_id.is_empty() {
        String::new()
    } else {
        db.get_novel(&novel_id)
            .ok()
            .flatten()
            .map(|n| n.title)
            .unwrap_or_default()
    };

    push_msg(&runtime, &scope, "user", content.clone());
    let _unsend = UnsendOnCancel {
        runtime: runtime.clone(),
        scope: scope.clone(),
        cancel: cancel.clone(),
    };

    let llm_user = if let Some((_, injected)) = skills::maybe_inject_skill(&content) {
        injected
    } else {
        content
    };

    let route_name = input.route_name.unwrap_or_default();
    let path = input.path.unwrap_or_default();
    let instruction = system_instruction(&route_name, &path, &novel_id, &novel_title);
    let est_prompt = ((instruction.len() + llm_user.len()) / 4) as u32;
    emit_tokens(&app, est_prompt, 0, false);

    let model = Arc::new(
        OpenAICompatible::new(
            OpenAICompatibleConfig::new(ep.api_key.clone(), api_model.clone())
                .with_base_url(openai_compat_base(&ep.base_url))
                .with_provider_name(if ep.label.trim().is_empty() {
                    "openai-compatible"
                } else {
                    ep.label.as_str()
                }),
        )
        .map_err(|e| e.to_string())?,
    );

    let mcp_url = format!("http://127.0.0.1:{port}/mcp");
    let toolset = McpHttpClientBuilder::new(mcp_url)
        .connect()
        .await
        .map_err(|e| format!("连接本机 MCP 失败: {e}"))?;

    let agent = Arc::new(
        LlmAgentBuilder::new("global_chat")
            .instruction(instruction)
            .model(model)
            .toolset(Arc::new(toolset))
            .build()
            .map_err(|e| e.to_string())?,
    );

    let mut session_id = {
        let mut slots = runtime.slots.lock();
        slots
            .entry(scope.clone())
            .or_insert_with(new_slot)
            .session_id
            .clone()
    };
    ensure_session(&runtime.sessions, &session_id).await?;
    close_dangling_tools(&runtime.sessions, &session_id).await?;

    let sessions: Arc<dyn SessionService> = runtime.sessions.clone();
    let runner = Runner::builder()
        .app_name(APP_NAME)
        .agent(agent)
        .session_service(sessions)
        .build()
        .map_err(|e| e.to_string())?;

    emit_progress(&app, "thinking");
    let mut stream = runner
        .run_str(
            USER_ID,
            &session_id,
            Content::new("user").with_text(llm_user.clone()),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut reply = String::new();
    let mut last_step = String::new();
    let mut prompt_sum = 0u32;
    let mut completion_sum = 0u32;
    let mut retried_tools = false;
    while let Some(ev) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            let _ = runner.interrupt(&session_id);
            // ponytail: 3s drain so in-flight tool_calls land before we stub results
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                while stream.next().await.is_some() {}
            })
            .await;
            let _ = close_dangling_tools(&runtime.sessions, &session_id).await;
            let (p, c) = usage_to_record(prompt_sum, completion_sum, est_prompt, reply.len());
            record_chat_usage(&db, &model_name, &novel_id, p, c);
            pop_last_user(&runtime, &scope);
            return Ok(list_scope(&runtime, &novel_id));
        }
        let ev = match ev {
            Ok(ev) => ev,
            Err(e) => {
                let msg = e.to_string();
                if retried_tools || !is_unmatched_tool_calls(&msg) {
                    if cancel.load(Ordering::SeqCst) {
                        pop_last_user(&runtime, &scope);
                        return Ok(list_scope(&runtime, &novel_id));
                    }
                    return Err(msg);
                }
                retried_tools = true;
                let _ = close_dangling_tools(&runtime.sessions, &session_id).await;
                session_id = Uuid::new_v4().to_string();
                if let Some(slot) = runtime.slots.lock().get_mut(&scope) {
                    slot.session_id.clone_from(&session_id);
                }
                ensure_session(&runtime.sessions, &session_id).await?;
                stream = runner
                    .run_str(
                        USER_ID,
                        &session_id,
                        Content::new("user").with_text(llm_user.clone()),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                continue;
            }
        };
        let mut got_text = false;
        if let Some(c) = ev.content() {
            for p in &c.parts {
                if let Some(t) = p.text() {
                    if !t.is_empty() {
                        reply.push_str(t);
                        got_text = true;
                    }
                }
            }
        }
        if let Some(step) = next_progress_step(
            ev.tool_calls().first().map(|c| c.name),
            !ev.tool_results().is_empty(),
            got_text,
        ) {
            if last_step != step {
                last_step.clone_from(&step);
                emit_progress(&app, &step);
            }
        }
        if let Some(u) = &ev.llm_response.usage_metadata {
            if !ev.llm_response.partial
                && (u.prompt_token_count > 0 || u.candidates_token_count > 0)
            {
                emit_tokens(
                    &app,
                    u.prompt_token_count.max(0) as u32,
                    u.candidates_token_count.max(0) as u32,
                    true,
                );
                prompt_sum = prompt_sum.saturating_add(u.prompt_token_count.max(0) as u32);
                completion_sum =
                    completion_sum.saturating_add(u.candidates_token_count.max(0) as u32);
            }
        }
    }

    if cancel.load(Ordering::SeqCst) {
        pop_last_user(&runtime, &scope);
        return Ok(list_scope(&runtime, &novel_id));
    }
    let (p, c) = usage_to_record(prompt_sum, completion_sum, est_prompt, reply.len());
    record_chat_usage(&db, &model_name, &novel_id, p, c);
    if reply.trim().is_empty() {
        reply = "（无回复）".into();
    }
    push_msg(&runtime, &scope, "assistant", reply);
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

    #[test]
    fn next_progress_prefers_tool_then_thinking_then_writing() {
        assert_eq!(
            next_progress_step(Some("get_chapter_info"), true, true).as_deref(),
            Some("tool:get_chapter_info")
        );
        assert_eq!(
            next_progress_step(None, true, true).as_deref(),
            Some("thinking")
        );
        assert_eq!(
            next_progress_step(None, false, true).as_deref(),
            Some("writing")
        );
        assert_eq!(next_progress_step(None, false, false), None);
        assert_eq!(next_progress_step(Some(""), false, true).as_deref(), Some("writing"));
    }

    #[test]
    fn usage_to_record_prefers_api_then_estimate() {
        assert_eq!(usage_to_record(10, 4, 99, 400), (10, 4));
        assert_eq!(usage_to_record(0, 0, 80, 12), (80, 3));
        assert_eq!(usage_to_record(0, 7, 80, 100), (80, 7));
    }

    #[test]
    fn openai_compat_base_does_not_append_v1() {
        assert_eq!(
            openai_compat_base("https://open.bigmodel.cn/api/paas/v4/"),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert_eq!(
            openai_compat_base("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1"
        );
    }

    #[test]
    fn scope_key_global_vs_novel() {
        assert_eq!(scope_key(None), "__global__");
        assert_eq!(scope_key(Some("")), "__global__");
        assert_eq!(scope_key(Some("  ")), "__global__");
        assert_eq!(scope_key(Some("abc")), "abc");
    }

    #[test]
    fn dangling_tool_calls_unmatched_then_closed() {
        let mut call = Event::new("i1");
        call.set_content(Content {
            role: "assistant".into(),
            parts: vec![Part::FunctionCall {
                name: "search_knowledge".into(),
                args: serde_json::json!({}),
                id: Some("call_1".into()),
                thought_signature: None,
            }],
        });
        assert_eq!(
            dangling_tool_calls(&[call.clone()]),
            vec![("call_1".into(), "search_knowledge".into())]
        );

        let done = cancelled_tool_event("call_1", "search_knowledge");
        assert!(dangling_tool_calls(&[call, done]).is_empty());
    }

    #[test]
    fn dangling_ignores_already_answered_calls() {
        let mut call = Event::new("i1");
        call.set_content(Content {
            role: "assistant".into(),
            parts: vec![
                Part::FunctionCall {
                    name: "a".into(),
                    args: serde_json::json!({}),
                    id: Some("c1".into()),
                    thought_signature: None,
                },
                Part::FunctionCall {
                    name: "b".into(),
                    args: serde_json::json!({}),
                    id: Some("c2".into()),
                    thought_signature: None,
                },
            ],
        });
        let mut result = Event::new("i2");
        result.set_content(Content {
            role: "function".into(),
            parts: vec![Part::FunctionResponse {
                function_response: FunctionResponseData::new("a", serde_json::json!({"ok": true})),
                id: Some("c1".into()),
                annotations: None,
            }],
        });
        assert_eq!(
            dangling_tool_calls(&[call, result]),
            vec![("c2".into(), "b".into())]
        );
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
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[tokio::test]
    async fn ensure_session_does_not_wipe_existing() {
        let sessions = Arc::new(InMemorySessionService::new());
        let sid = Uuid::new_v4().to_string();
        sessions
            .create(CreateRequest {
                app_name: APP_NAME.into(),
                user_id: USER_ID.into(),
                session_id: Some(sid.clone()),
                state: HashMap::from([("marker".into(), Value::String("keep".into()))]),
            })
            .await
            .unwrap();
        ensure_session(&sessions, &sid).await.unwrap();
        let s = sessions
            .get(GetRequest {
                app_name: APP_NAME.into(),
                user_id: USER_ID.into(),
                session_id: sid,
                num_recent_events: None,
                after: None,
            })
            .await
            .unwrap();
        let marker = s.state().get("marker").and_then(|v| v.as_str().map(str::to_string));
        assert_eq!(marker.as_deref(), Some("keep"));
    }

    #[tokio::test]
    async fn close_dangling_tools_stubs_unmatched_calls() {
        let sessions = Arc::new(InMemorySessionService::new());
        let sid = Uuid::new_v4().to_string();
        sessions
            .create(CreateRequest {
                app_name: APP_NAME.into(),
                user_id: USER_ID.into(),
                session_id: Some(sid.clone()),
                state: HashMap::new(),
            })
            .await
            .unwrap();
        let mut call = Event::new("i1");
        call.set_content(Content {
            role: "assistant".into(),
            parts: vec![Part::FunctionCall {
                name: "search_knowledge".into(),
                args: serde_json::json!({}),
                id: Some("call_9".into()),
                thought_signature: None,
            }],
        });
        sessions.append_event(&sid, call).await.unwrap();
        close_dangling_tools(&sessions, &sid).await.unwrap();
        let s = sessions
            .get(GetRequest {
                app_name: APP_NAME.into(),
                user_id: USER_ID.into(),
                session_id: sid,
                num_recent_events: None,
                after: None,
            })
            .await
            .unwrap();
        assert!(dangling_tool_calls(&s.events().all()).is_empty());
    }
}
