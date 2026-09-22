//! Local MCP (Model Context Protocol) via `rmcp` Streamable HTTP — tools only.
//! Endpoint: `http://127.0.0.1:{port}/mcp` (or `0.0.0.0` when LAN enabled).
//! Payload / tool contract changes: sync per `.cursor/rules/mcp-sync.mdc` + `API.md`.

use crate::commands::{
    add_public_knowledge_card_inner, chapter_length_feedback, create_novel_with_tree,
    delete_tree_card_inner, fill_knowledge_card_inner, generate_detailed_outline_inner,
    generate_story_rules_chat_inner, generate_worldview_chat_inner, get_chapter,
    get_chapter_memory_inner, get_tree, ingest_knowledge_source, knowledge_attach_host,
    last_chapter_anchor, list_all_chapter_memory_inner, regenerate_chapter_memory_inner,
    regenerate_detailed_outline_item_inner, resolve_link_host, resolve_node_ref, save_chapter,
    save_tree,
    set_chapter_memory_inner, shot_character_looks, submit_chapter_shots_comfyui_inner,
    upsert_public_knowledge_card_inner, worldview_snapshot_value, ChatTurn,
};
use crate::db::Db;
use crate::kb_context::retrieve_knowledge;
use crate::models::{
    CharacterCard, ConstraintRedlinesPayload, CoreLawsPayload, ExistencePayload,
    FulfillmentSystemPayload, HistoryCulturePayload, InfoFlowPayload, KnowledgeCardPayload,
    NodeKind, NodePosition, NovelFeatures, NovelProject, NovelTree, SidePlotMeta, SocialPowerPayload,
    SpatiotemporalPayload, StoryEnginePayload, SurfaceSettingPayload, TreeEdge, TreeNode,
    VolumePayload,
};
use crate::paths::novel_meta_path;
use crate::tree_links::{
    self, chapter_effective_knowledge_ids, chapter_local_knowledge_ids, chapter_local_plot_ids,
    chapter_parent_volume_id, root_character_ids, root_knowledge_ids, root_plot_ids,
    volume_character_ids, volume_local_knowledge_ids, volume_local_plot_ids, volume_plot_ids,
};
use chrono::Utc;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const DEFAULT_MCP_PORT: u16 = 17832;

#[derive(Clone)]
pub struct McpCtx {
    pub db: Arc<Db>,
    pub app: Arc<Mutex<Option<AppHandle>>>,
    /// 工作台当前选中卡片（与 `McpRuntime.selection` 同一把锁）
    pub selection: Arc<Mutex<Option<(String, String)>>>,
}

pub struct McpRuntime {
    pub port: AtomicU16,
    pub running: AtomicBool,
    pub last_error: Mutex<Option<String>>,
    shutdown: Mutex<Option<CancellationToken>>,
    /// 工作台当前选中：(novel_id, node_id)；进程内，不落盘
    pub selection: Arc<Mutex<Option<(String, String)>>>,
}

impl McpRuntime {
    pub fn new(port: u16) -> Self {
        Self {
            port: AtomicU16::new(port),
            running: AtomicBool::new(false),
            last_error: Mutex::new(None),
            shutdown: Mutex::new(None),
            selection: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_workspace_selection(&self, novel_id: &str, node_id: &str) {
        *self.selection.lock().unwrap() = Some((novel_id.to_string(), node_id.to_string()));
    }

    pub fn clear_workspace_selection(&self) {
        *self.selection.lock().unwrap() = None;
    }

    /// Leave workspace A after opening B: only clear if still pointing at A
    /// (avoids wiping B when A's unmount `clear` races after B's `set`).
    pub fn clear_workspace_selection_if(&self, novel_id: &str) {
        let mut sel = self.selection.lock().unwrap();
        if sel.as_ref().is_some_and(|(n, _)| n == novel_id) {
            *sel = None;
        }
    }

    pub fn workspace_selection(&self) -> Option<(String, String)> {
        self.selection.lock().unwrap().clone()
    }

    pub fn status_json(&self, lan: bool) -> Value {
        let port = self.port.load(Ordering::SeqCst);
        let running = self.running.load(Ordering::SeqCst);
        let err = self.last_error.lock().unwrap().clone();
        json!({
            "running": running,
            "port": port,
            "lan_enabled": lan,
            "endpoint": format!("http://127.0.0.1:{port}/mcp"),
            "lan_endpoint": if lan {
                Some(format!("http://<LAN-IP>:{port}/mcp"))
            } else {
                None::<String>
            },
            "error": err,
        })
    }
}

#[derive(Clone)]
struct NoveMcpServer {
    ctx: McpCtx,
}

impl ServerHandler for NoveMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "nove-work",
                env!("CARGO_PKG_VERSION"),
            ))
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tool_defs()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_else(Map::new));
        let result = match call_tool(self.ctx.clone(), request.name.as_ref(), args).await {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        };
        Ok(result.into())
    }
}

pub fn restart(ctx: McpCtx, runtime: Arc<McpRuntime>, enabled: bool, port: u16, lan: bool) {
    if let Some(ct) = runtime.shutdown.lock().unwrap().take() {
        ct.cancel();
    }
    runtime.running.store(false, Ordering::SeqCst);
    runtime.port.store(port, Ordering::SeqCst);
    *runtime.last_error.lock().unwrap() = None;

    if !enabled {
        return;
    }
    let port = if (1024..=65535).contains(&port) {
        port
    } else {
        DEFAULT_MCP_PORT
    };
    runtime.port.store(port, Ordering::SeqCst);

    let ct = CancellationToken::new();
    *runtime.shutdown.lock().unwrap() = Some(ct.clone());
    let rt = runtime.clone();
    let bind = if lan { "0.0.0.0" } else { "127.0.0.1" };
    tauri::async_runtime::spawn(async move {
        match TcpListener::bind(format!("{bind}:{port}")).await {
            Ok(listener) => {
                rt.running.store(true, Ordering::SeqCst);
                let ctx_for_factory = ctx.clone();
                let service: StreamableHttpService<NoveMcpServer, LocalSessionManager> =
                    StreamableHttpService::new(
                        move || {
                            Ok(NoveMcpServer {
                                ctx: ctx_for_factory.clone(),
                            })
                        },
                        Arc::new(LocalSessionManager::default()),
                        StreamableHttpServerConfig::default()
                            .with_json_response(true)
                            .with_cancellation_token(ct.child_token()),
                    );
                let router = axum::Router::new().nest_service("/mcp", service);
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(async move { ct.cancelled().await })
                    .await;
                rt.running.store(false, Ordering::SeqCst);
            }
            Err(e) => {
                *rt.last_error.lock().unwrap() = Some(e.to_string());
                rt.running.store(false, Ordering::SeqCst);
            }
        }
    });
}

fn tool_defs() -> Vec<Tool> {
    vec![
        tool(
            "list_knowledge",
            "List public knowledge books (set include_archived to include archived).",
            json!({
                "type":"object",
                "properties":{
                    "include_archived":{"type":"boolean","default":false}
                }
            }),
        ),
        tool(
            "import_knowledge",
            "Import a local file (txt/epub) into the public knowledge library. Optional title; EPUB title is auto-detected when omitted.",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "title":{"type":"string"},
                    "genres":{"type":"array","items":{"type":"string"}}
                },
                "required":["path"]
            }),
        ),
        tool(
            "search_knowledge",
            "Vector/keyword search over public knowledge (passages for AI to refine, then write back with upsert_knowledge_card). Empty book_ids = all non-archived books. Do not dump full books into a knowledge card — use fill_knowledge_card for that.",
            json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "book_ids":{"type":"array","items":{"type":"string"}},
                    "limit":{"type":"integer","default":8},
                    "chunk_cap":{"type":"integer","default":600}
                },
                "required":["query"]
            }),
        ),
        tool(
            "archive_knowledge",
            "Archive or unarchive a public knowledge book.",
            json!({
                "type":"object",
                "properties":{
                    "id":{"type":"string"},
                    "archived":{"type":"boolean","default":true}
                },
                "required":["id"]
            }),
        ),
        tool(
            "delete_knowledge",
            "Permanently delete an archived public knowledge book.",
            json!({
                "type":"object",
                "properties":{"id":{"type":"string"}},
                "required":["id"]
            }),
        ),
        tool(
            "list_novels",
            "List novels (non-archived by default).",
            json!({
                "type":"object",
                "properties":{"include_archived":{"type":"boolean","default":false}}
            }),
        ),
        tool(
            "create_novel",
            "Create a novel with title, synopsis, optional chapter_count, per-chapter word range, and features (题材/核心玩法/风格/关系/受众).",
            json!({
                "type":"object",
                "properties":{
                    "title":{"type":"string"},
                    "synopsis":{"type":"string"},
                    "chapter_count":{"type":"integer"},
                    "word_count_min":{"type":"integer"},
                    "word_count_max":{"type":"integer"},
                    "features":{
                        "type":"object",
                        "description":"功能选项：genres / core_play / styles / relationships / audiences（均为 string[]；预设 id 或自定义文本）",
                        "properties":{
                            "genres":{"type":"array","items":{"type":"string"}},
                            "core_play":{"type":"array","items":{"type":"string"}},
                            "styles":{"type":"array","items":{"type":"string"}},
                            "relationships":{"type":"array","items":{"type":"string"}},
                            "audiences":{"type":"array","items":{"type":"string"}}
                        }
                    }
                },
                "required":["title"]
            }),
        ),
        tool(
            "update_novel",
            "Update novel title, synopsis, chapter_count, word range, and/or features (题材/核心玩法/风格/关系/受众).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "synopsis":{"type":"string"},
                    "chapter_count":{"type":"integer"},
                    "word_count_min":{"type":"integer"},
                    "word_count_max":{"type":"integer"},
                    "features":{
                        "type":"object",
                        "description":"功能选项（整对象替换）：genres / core_play / styles / relationships / audiences",
                        "properties":{
                            "genres":{"type":"array","items":{"type":"string"}},
                            "core_play":{"type":"array","items":{"type":"string"}},
                            "styles":{"type":"array","items":{"type":"string"}},
                            "relationships":{"type":"array","items":{"type":"string"}},
                            "audiences":{"type":"array","items":{"type":"string"}}
                        }
                    }
                }
            }),
        ),
        tool(
            "get_novel_info",
            "Novel snapshot: title, synopsis, features (题材/核心玩法/风格/关系/受众), word/chapter plan, root-linked characters, plot cards, knowledge cards. Prefer this over get_tree for orientation.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "get_worldview",
            "Read root worldview snapshot (six fan cards + story_rules). Also returns novel.features used as hard constraints when generating.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "ensure_worldview",
            "Ensure the six worldview cards + story_rules exist on the novel root (empty shells if missing). Does not overwrite content.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "apply_worldview",
            "Write a worldview JSON object onto the tree (same shape as generate_worldview / Chat). Keys: core_laws / spatiotemporal / social_power / existence / info_flow / history_culture / story_rules (extracted) / story_rules_blocks (four fan cards). Creates missing slots; layouts after apply.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "worldview":{"type":"object","description":"含 core_laws / spatiotemporal / social_power / existence / info_flow / history_culture / story_rules"}
                },
                "required":["worldview"]
            }),
        ),
        tool(
            "generate_worldview",
            "LLM-generate worldview from instruction. Always respects novel.features (题材/核心玩法/风格/关系/受众). Optional slot fills only that one of the six cards (all fields). Default apply=true writes cards; set apply=false to only return JSON.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "instruction":{"type":"string","description":"用户指令：随机全新 / 按现有扩展 / 具体种子"},
                    "slot":{"type":"string","description":"可选。只补一张卡的全部字段：wv_core_laws / wv_spatiotemporal / wv_social_power / wv_history_culture / wv_existence / wv_info_flow（也可写 JSON 键 core_laws 等）。省略则生成全部六卡 + story_rules。"},
                    "apply":{"type":"boolean","default":true},
                    "model":{"type":"string"}
                },
                "required":["instruction"]
            }),
        ),
        tool(
            "get_story_rules",
            "Read story_rules parent + four fan cards (surface_setting / story_engine / fulfillment_system / constraint_redlines). Returns blocks JSON, formatted inject text, and parent snapshot.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "apply_story_rules",
            "Write story rules four-card JSON (same shape as generate_story_rules / Chat blocks). Creates missing fan cards; syncs extracted + layout.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "blocks":{"type":"object","description":"含 surface_setting / story_engine / fulfillment_system / constraint_redlines"}
                },
                "required":["blocks"]
            }),
        ),
        tool(
            "generate_story_rules",
            "LLM-generate story rules four cards from instruction (uses novel context + worldview). Default apply=true writes blocks; set apply=false to only return JSON.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "instruction":{"type":"string","description":"用户指令"},
                    "apply":{"type":"boolean","default":true},
                    "model":{"type":"string"}
                },
                "required":["instruction"]
            }),
        ),
        tool(
            "get_selected_card",
            "The card currently selected in the workspace UI. Returns the live tree node; chapter/plot body is omitted unless include_content=true. Prefer this or get_character_card over get_tree/get_novel_info when editing cards.",
            json!({
                "type":"object",
                "properties":{
                    "include_content":{"type":"boolean","default":false,"description":"章/剧情卡是否附带正文；改卡/细纲请保持 false"}
                }
            }),
        ),
        tool(
            "get_tree",
            "Get the novel structure tree (nodes + edges).",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "layout_tree",
            "Rewrite node positions with the four-band canvas layout. Workspace no longer auto-layouts; call this after adding many cards if the tree is piled up.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "add_volume",
            "Add an optional volume (分卷) under the novel root (root.bottom → volume.top). Volumes can link characters/plots/knowledge (knowledge does not inherit to chapters). Write structured `volume` payload (positioning / layers / conflicts / key_beats); do not use outline. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "volume":{"type":"object","description":"VolumePayload：positioning, layer_setup/confrontation/resolution, conflict_*, key_beats"}
                }
            })),
        ),
        tool(
            "get_volume",
            "Read volume(s): title, full VolumePayload, formatted (写作注入), linked plot/character/knowledge cards. Omit node_id for all volumes.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"分卷 id 或唯一标题；省略则返回本书全部分卷"}
                }
            }),
        ),
        tool(
            "upsert_volume",
            "Update volume (partial merge). Fields: title, volume object, or top-level positioning / layer_setup / layer_confrontation / layer_resolution / conflict_external / conflict_internal / conflict_deep / key_beats. Do not write outline; use formatted for injection text. Keeps linked plots/knowledge/characters.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"分卷 id 或唯一标题"},
                    "title":{"type":"string"},
                    "volume":{"type":"object","description":"VolumePayload 部分字段，深度合并"},
                    "positioning":{"type":"string","description":"本卷定位"},
                    "layer_setup":{"type":"object","properties":{"chapters":{"type":"string"},"description":{"type":"string"}}},
                    "layer_confrontation":{"type":"object","properties":{"chapters":{"type":"string"},"description":{"type":"string"}}},
                    "layer_resolution":{"type":"object","properties":{"chapters":{"type":"string"},"description":{"type":"string"}}},
                    "conflict_external":{"type":"string"},
                    "conflict_internal":{"type":"string"},
                    "conflict_deep":{"type":"string"},
                    "key_beats":{"type":"array","items":{"type":"object","properties":{"order":{"type":"integer"},"cost":{"type":"string"},"description":{"type":"string"}}}}
                },
                "required":["node_id"]
            }),
        ),
        tool(
            "add_chapter",
            "Add a chapter card. Hang on volume if link_to is a volume (or last chapter under that volume); else first chapter on root, later chapters chain by canvas y. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "outline":{"type":"string"},
                    "link_to":{"type":"string","description":"宿主：volume / chapter / root；省略则挂末章或根"}
                }
            })),
        ),
        tool(
            "update_chapter_outline",
            "Update a chapter (or plot) card title and/or brief outline (`outline`) and/or detailed outline beats (`detailed_outline` string array, chapters only).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "outline":{"type":"string","description":"简纲（章节）或剧情要点"},
                    "detailed_outline":{"type":"array","items":{"type":"string"},"description":"细纲分条（仅章节）；正文据此扩充"}
                }
            }),
        ),
        tool(
            "generate_detailed_outline",
            "Evolve chapter detailed_outline from brief outline. Assembles synopsis, story tags, characters, plot beats, and full worldview/story-rules server-side — do not get_tree/get_novel_info first. Beats must not contradict those cards. Beat count follows per-chapter word target. Writes tree. Call before writing body if detailed_outline is empty.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "user_notes":{"type":"string","description":"可选补充要求"},
                    "model":{"type":"string"}
                }
            }),
        ),
        tool(
            "regenerate_detailed_outline_item",
            "AI-rewrite one beat of chapter detailed_outline by 0-based index. Must not contradict worldview/story-rules. Keeps other beats; writes tree.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "index":{"type":"integer","description":"细纲下标（从 0 起）"},
                    "user_notes":{"type":"string","description":"可选补充要求"},
                    "model":{"type":"string"}
                },
                "required":["index"]
            }),
        ),
        tool(
            "delete_node",
            "Delete a chapter/volume/character/plot/knowledge card (not root, not the write_prompts Generate/Refine hub). Deleting a volume reparents its chapters to root.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "get_chapter_content",
            "Read chapter/plot Markdown body only. Use for refine/edit; do not call when generating fresh body from outline (use get_chapter_write_context instead).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "get_chapter_info",
            "Chapter snapshot (no body): local outline/cards only. Knowledge is not inherited. For generating/refining chapter body use get_chapter_write_context.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "get_chapter_write_context",
            "Write-chapter bundle: optional root (worldview/story rules + root cards) + optional volume (direct cards + volume payload) + chapter cards/outline/detailed_outline. Dedupes character/plot/knowledge ids (first-seen wins). If this Chat already received root, pass include_root=false; if same volume already received, include_volume=false. Do not start a new session. No chapter body.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"章节 id、「第N章」、章号或唯一标题；省略则用当前选中"},
                    "include_root":{"type":"boolean","description":"本会话尚未提交根节点时 true（默认）。已有根材料则 false，不再返回根卡。"},
                    "include_volume":{"type":"boolean","description":"本会话尚未提交本章所属分卷时 true（默认）。同卷已提交则 false。无分卷时忽略。"}
                }
            }),
        ),
        tool(
            "set_chapter_content",
            "Write chapter Markdown body. Returns word_count and in_band. If in_band, do not rewrite for length.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "content":{"type":"string"}
                },
                "required":["content"]
            }),
        ),
        tool(
            "get_chapter_memory",
            "Read distilled chapter memory items (facts, not body text).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "list_chapter_memory",
            "All chapters with memory, grouped in tree chapter order (skips chapters without memory).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "set_chapter_memory",
            "Overwrite or clear chapter memory. Empty items clears SQLite + Lance.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "items":{"type":"array","items":{"type":"string"}}
                },
                "required":["items"]
            }),
        ),
        tool(
            "get_chapter_shots",
            "Read storyboard shots for a chapter (action/camera/dialogue/duration/comfy_prompt). Empty if not split yet.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "set_chapter_shots",
            "Overwrite the chapter's shot list (one chapter → many shots; empty array clears). Fields: action, camera, dialogue, duration_sec (5–15), comfy_prompt.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "shots":{"type":"array","items":{"type":"object"}}
                },
                "required":["shots"]
            }),
        ),
        tool(
            "split_chapter_shots",
            "Return chapter body + character looks + shot schema. Does NOT call the app LLM — you split shots yourself, then overwrite with set_chapter_shots. Requires non-empty body.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "generate_shot_comfy_prompts",
            "Return existing shots + character looks for MiniMax/ComfyUI prompts. Does NOT call the app LLM — you write each comfy_prompt, then set_chapter_shots.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "submit_chapter_shots_comfyui",
            "Queue shot prompts to local ComfyUI Desktop (POST /prompt). Needs settings.comfyui_url + API-format workflow (MiniMax text/prompt node). Returns prompt_ids; does not wait for video files.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "regenerate_chapter_memory",
            "LLM extract memory from chapter body and overwrite. Requires non-empty body on disk.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "user_notes":{"type":"string","description":"抽取注意事项，可空"},
                    "memory_node_ids":{"type":"array","items":{"type":"string"},"description":"其他章 id，注入其记忆作对照去重"}
                }
            }),
        ),
        tool(
            "get_character_card",
            "Read one character card (structured fields + formatted). Pass node_id. Omit node_id to list id/label only — do not dump every full card.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"人物卡 id 或唯一人名；省略则返回本书全部人物卡"}
                }
            }),
        ),
        tool(
            "upsert_character_card",
            "Create or update a character card (partial merge). Returns ok/node_id/label only — do not echo the full card back. Pass flat fields and/or `character`. Do not send personality. New cards hang on a chapter unless link_to set. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"有则更新该人物卡（id 或唯一人名）；省略则新建"},
                    "name":{"type":"string","description":"真名（节点 label）；更新时可省略保留原名"},
                    "character":{"type":"object","description":"人物卡 JSON（CharacterCard 本体，或 get_character_card 返回的整条 entry）；深度合并，不整卡清空"},
                    "world_position":{"type":"object"},
                    "world_anchors":{"type":"object"},
                    "relations":{"type":"array","items":{"type":"object"}},
                    "core_belief":{"type":"object"},
                    "deep":{"type":"object"},
                    "voice":{"type":"object","description":"角色声线（含 body_language 肢体语言）"},
                    "body_language":{"type":"string","description":"肢体语言，写入 voice.body_language"},
                    "role":{"type":"string","description":"社会角色（扁平字段，覆盖 character）"},
                    "gender":{"type":"string"},
                    "age":{"type":"string","description":"年龄，自由文本"},
                    "aliases":{"type":"string","description":"别称"},
                    "alignment":{"type":"string","description":"阵营归属（扁平）"},
                    "style":{"type":"string"},
                    "motto":{"type":"string"},
                    "constraints":{"type":"string","description":"写章注入全文；通常由结构化字段自动生成"},
                    "link_to":{"type":"string","description":"章/根 id，或 root/novel。新建省略则挂当前选中章，否则末章，再否则根"}
                }
            })),
        ),
        tool(
            "upsert_plot_card",
            "Create or update a plot card. Returns ok/node_id/label only. New cards hang on a chapter (chapter.right → plot.left). Omit link_to: selection, else last chapter, else root. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"有则更新该剧情卡（id 或唯一标题）；省略则新建"},
                    "title":{"type":"string"},
                    "outline":{"type":"string"},
                    "status":{"type":"string"},
                    "link_to":{"type":"string","description":"章/根 id，或 root/novel。新建省略则挂当前选中章，否则末章，再否则根"}
                }
            })),
        ),
        tool(
            "upsert_knowledge_card",
            "Create or update a tree knowledge card (partial). Returns ok/node_id/label/slot only — do not echo extracted. Worldview/story_rules: set slot or node_id + structured payload; extracted auto-synced. Fixed root slots update in place. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"有则更新该知识卡（id 或唯一标题）；省略则新建或按 slot 匹配"},
                    "slot":{"type":"string","description":"固定槽位 id（wv_core_laws / story_rules / sr_surface_setting 等）；更新时可省略 node_id"},
                    "title":{"type":"string","description":"新建必填；更新可省略保留原标题"},
                    "book_ids":{"type":"array","items":{"type":"string"}},
                    "extract_prompt":{"type":"string"},
                    "extracted":{"type":"string","description":"普通知识卡；结构化卡优先用下方 payload，会自动同步 extracted"},
                    "core_laws":{"type":"object"},
                    "spatiotemporal":{"type":"object"},
                    "social_power":{"type":"object"},
                    "existence":{"type":"object"},
                    "info_flow":{"type":"object"},
                    "history_culture":{"type":"object"},
                    "surface_setting":{"type":"object"},
                    "story_engine":{"type":"object"},
                    "fulfillment_system":{"type":"object"},
                    "constraint_redlines":{"type":"object"},
                    "link_to":{"type":"string","description":"章/根 id，或 root/novel。新建省略则挂当前选中章，否则末章，再否则根"}
                }
            })),
        ),
        tool(
            "fill_knowledge_card",
            "Full-import: copy associated public knowledge books into this knowledge card's extracted field (capped). Returns ok/extracted_chars only. Requires book_ids on the card or in args. For AI-processed write-back use search_knowledge then upsert_knowledge_card(extracted).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":["string","integer"],"description":"树上 id、「第N章」、章号或唯一标题；省略则用工作台当前选中"},
                    "book_ids":{"type":"array","items":{"type":"string"},"description":"有则写入卡片再导入；否则用卡上已有 book_ids"},
                    "max_chars":{"type":"integer","default":14000}
                }
            }),
        ),
        tool(
            "list_public_knowledge_cards",
            "List shared knowledge cards (catalog, reusable across novels). Not public knowledge books. Default hides archived.",
            json!({
                "type":"object",
                "properties":{
                    "include_archived":{"type":"boolean","default":false}
                }
            }),
        ),
        tool(
            "upsert_public_knowledge_card",
            "Create or update a shared knowledge card in the catalog. Pass title+extracted, or omit them to copy the current/selected tree knowledge card (node_id). Does not attach to a novel. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "id":{"type":"string","description":"有则更新该公共知识卡"},
                    "title":{"type":"string"},
                    "book_ids":{"type":"array","items":{"type":"string"}},
                    "extract_prompt":{"type":"string"},
                    "extracted":{"type":"string"},
                    "novel_id":{"type":"string","description":"从树上知识卡复制时用"},
                    "node_id":{"type":["string","integer"],"description":"树上知识卡 id 或唯一标题；省略则用工作台当前选中"}
                }
            })),
        ),
        tool(
            "archive_public_knowledge_card",
            "Archive or unarchive a shared knowledge card in the catalog. Does not change copies already on novel trees.",
            json!({
                "type":"object",
                "properties":{
                    "id":{"type":"string"},
                    "archived":{"type":"boolean","default":true}
                },
                "required":["id"]
            }),
        ),
        tool(
            "add_public_knowledge_card",
            "Copy a shared catalog knowledge card onto the current novel tree and link it to the selected node (chapter/root, or the host of the selected card). Does not bind public libraries as novel canon. Pass items[] to add many.",
            with_batch_items(json!({
                "type":"object",
                "properties":{
                    "public_id":{"type":"string","description":"公共知识卡 id"},
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "link_to":{"type":"string","description":"章/根节点 id，或 root/novel；省略则用当前选中"}
                }
            })),
        ),
        tool(
            "link_nodes",
            "Link two nodes (character/side_plot/knowledge/chapter). Updates linked_* on the host.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "source_id":{"type":"string"},
                    "target_id":{"type":"string"},
                    "kind":{"type":"string","description":"character|side_plot|knowledge|chapter"}
                },
                "required":["source_id","target_id"]
            }),
        ),
        tool(
            "unlink_nodes",
            "Remove edge(s) between two nodes (or by edge_id). Cannot unlink worldview/story-rules cards, or the root↔write_prompts (Generate/Refine) edge. Child knowledge on write_prompts can be unlinked.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "edge_id":{"type":"string"},
                    "source_id":{"type":"string"},
                    "target_id":{"type":"string"}
                }
            }),
        ),
    ]
}

/// ponytail: 逐项 save_tree；若经常一次上百张再改成单次落盘。
const BATCH_MAX: usize = 50;

fn batch_items_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": 50,
        "description": "批量添加：每项字段同本工具单条。顶层 novel_id、link_to 作缺省。有 items 时按项执行（最多 50）；失败项不回滚已成功项。整列画布请 layout_tree。",
        "items": { "type": "object" }
    })
}

fn with_batch_items(mut schema: Value) -> Value {
    if let Some(props) = schema
        .get_mut("properties")
        .and_then(|v| v.as_object_mut())
    {
        props.insert("items".into(), batch_items_schema());
    }
    schema
}

fn merge_batch_item(parent: &Value, item: &Value) -> Value {
    let mut obj = item.as_object().cloned().unwrap_or_default();
    if let Some(p) = parent.as_object() {
        for key in ["novel_id", "link_to"] {
            if !obj.contains_key(key) {
                if let Some(v) = p.get(key) {
                    obj.insert(key.to_string(), v.clone());
                }
            }
        }
    }
    Value::Object(obj)
}

fn run_maybe_batch(
    args: Value,
    f: impl Fn(Value) -> Result<String, String>,
) -> Result<String, String> {
    let Some(arr) = args.get("items").and_then(|v| v.as_array()) else {
        return f(args);
    };
    if arr.is_empty() {
        return Err("items 不能为空".into());
    }
    if arr.len() > BATCH_MAX {
        return Err(format!("一次最多 {BATCH_MAX} 张，请分批"));
    }
    let mut results = Vec::with_capacity(arr.len());
    let mut ok_n = 0usize;
    for (i, item) in arr.iter().enumerate() {
        let one = merge_batch_item(&args, item);
        match f(one) {
            Ok(s) => {
                ok_n += 1;
                let parsed = serde_json::from_str::<Value>(&s).unwrap_or(Value::String(s));
                results.push(json!({ "ok": true, "index": i, "result": parsed }));
            }
            Err(e) => {
                results.push(json!({ "ok": false, "index": i, "error": e }));
            }
        }
    }
    Ok(json!({
        "count": results.len(),
        "ok": ok_n,
        "failed": results.len() - ok_n,
        "results": results,
        "ai_guidance": "批量已处理。失败项未回滚已成功项。需要整列画布时 layout_tree。不要把整卡贴回对话。",
    })
    .to_string())
}

fn tool(name: &str, description: &str, input_schema: Value) -> Tool {
    let schema = match input_schema {
        Value::Object(m) => Arc::new(m),
        _ => Arc::new(Map::new()),
    };
    Tool::new(name.to_string(), description.to_string(), schema)
}

async fn call_tool(ctx: McpCtx, name: &str, args: Value) -> Result<String, String> {
    match name {
        "list_knowledge" => {
            let include = args
                .get("include_archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut books = ctx.db.list_knowledge().map_err(|e| e.to_string())?;
            if !include {
                books.retain(|b| !b.archived);
            }
            Ok(serde_json::to_string_pretty(&books).unwrap_or_default())
        }
        "import_knowledge" => {
            let path = arg_str(&args, "path")?;
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let genres = args
                .get("genres")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let src = crate::knowledge::load_source(&path).map_err(|e| e.to_string())?;
            let app = ctx.app.lock().unwrap().clone();
            let book = ingest_knowledge_source(
                app.as_ref(),
                &ctx.db,
                src,
                path,
                genres,
                title,
            )
            .await?;
            Ok(serde_json::to_string_pretty(&book).unwrap_or_default())
        }
        "search_knowledge" => {
            let query = arg_str(&args, "query")?;
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
            let cap = args
                .get("chunk_cap")
                .and_then(|v| v.as_u64())
                .unwrap_or(600) as usize;
            let mut book_ids: Vec<String> = args
                .get("book_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if book_ids.is_empty() {
                book_ids = ctx
                    .db
                    .list_knowledge()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter(|b| !b.archived)
                    .map(|b| b.id)
                    .collect();
            }
            let hits = retrieve_knowledge(&ctx.db, &book_ids, &query, limit.max(1), cap.max(100));
            let out: Vec<Value> = hits
                .into_iter()
                .map(|(sid, title, content)| {
                    json!({"id": sid, "title": title, "content": content})
                })
                .collect();
            Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
        }
        "archive_knowledge" => {
            let id = arg_str(&args, "id")?;
            let archived = args
                .get("archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            ctx.db
                .set_knowledge_archived(&id, archived)
                .map_err(|e| e.to_string())?;
            let book = ctx
                .db
                .get_knowledge(&id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "知识库不存在".to_string())?;
            Ok(serde_json::to_string_pretty(&book).unwrap_or_default())
        }
        "delete_knowledge" => {
            let id = arg_str(&args, "id")?;
            let book = ctx
                .db
                .get_knowledge(&id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "知识库不存在".to_string())?;
            if !book.archived {
                return Err("请先归档后再删除".into());
            }
            let _ = crate::knowledge_vec::delete_book(&id).await;
            ctx.db.delete_knowledge(&id).map_err(|e| e.to_string())?;
            let _ = ctx.db.unlink_knowledge_from_novels(&id);
            Ok(json!({"ok": true, "id": id}).to_string())
        }
        "list_novels" => {
            let include = args
                .get("include_archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut novels = ctx.db.list_novels().map_err(|e| e.to_string())?;
            if !include {
                novels.retain(|n| !n.archived);
            }
            let out: Vec<Value> = novels.iter().map(novel_json_for_mcp).collect();
            Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
        }
        "create_novel" => {
            let title = arg_str(&args, "title")?;
            let synopsis = args
                .get("synopsis")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let wmin = args
                .get("word_count_min")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000) as u32;
            let wmax = args
                .get("word_count_max")
                .and_then(|v| v.as_u64())
                .unwrap_or(3000) as u32;
            let chapters = args
                .get("chapter_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as u32;
            let features = args
                .get("features")
                .cloned()
                .and_then(|v| serde_json::from_value::<NovelFeatures>(v).ok())
                .unwrap_or_default();
            let novel = create_novel_with_tree(
                &ctx.db,
                &title,
                &synopsis,
                &[],
                "",
                wmin,
                wmax,
                chapters,
                None,
                None,
                features,
            )?;
            Ok(serde_json::to_string_pretty(&novel).unwrap_or_default())
        }
        "update_novel" => update_novel(&ctx, args),
        "get_novel_info" => get_novel_info(&ctx, args),
        "get_worldview" => get_worldview(&ctx, args),
        "ensure_worldview" => ensure_worldview(&ctx, args),
        "apply_worldview" => apply_worldview(&ctx, args),
        "generate_worldview" => generate_worldview(&ctx, args).await,
        "get_story_rules" => get_story_rules(&ctx, args),
        "apply_story_rules" => apply_story_rules(&ctx, args),
        "generate_story_rules" => generate_story_rules(&ctx, args).await,
        "get_selected_card" => get_selected_card(&ctx, args),
        "get_tree" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let tree = get_tree(novel_id)?;
            Ok(serde_json::to_string_pretty(&tree).unwrap_or_default())
        }
        "layout_tree" => layout_tree(&ctx, args),
        "add_volume" => run_maybe_batch(args, |a| add_volume(&ctx, a)),
        "get_volume" => get_volume(&ctx, args),
        "upsert_volume" => upsert_volume(&ctx, args),
        "add_chapter" => run_maybe_batch(args, |a| add_chapter(&ctx, a)),
        "update_chapter_outline" => update_outline(&ctx, args),
        "generate_detailed_outline" => mcp_generate_detailed_outline(&ctx, args).await,
        "regenerate_detailed_outline_item" => {
            mcp_regenerate_detailed_outline_item(&ctx, args).await
        }
        "delete_node" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let tree = delete_tree_card_inner(&ctx.db, novel_id, node_id)?;
            emit_tree_changed(&ctx, &tree.novel_id);
            Ok(serde_json::to_string_pretty(&tree).unwrap_or_default())
        }
        "get_chapter_content" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            get_chapter(novel_id, node_id)
        }
        "get_chapter_info" => get_chapter_info(&ctx, args),
        "get_chapter_write_context" => get_chapter_write_context(&ctx, args),
        "set_chapter_content" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let content = arg_str(&args, "content")?;
            let before = get_chapter(novel_id.clone(), node_id.clone()).unwrap_or_default();
            let words = save_chapter(novel_id.clone(), node_id.clone(), content)?;
            emit_tree_changed(&ctx, &novel_id);
            emit_chapter_content_changed(&ctx, &novel_id, &node_id, &before);
            let (wmin, wmax) = ctx
                .db
                .get_novel(&novel_id)
                .ok()
                .flatten()
                .map(|n| (n.word_count_min, n.word_count_max))
                .unwrap_or((2000, 3000));
            Ok(chapter_length_feedback(words, wmin, wmax).to_string())
        }
        "get_chapter_shots" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let shots = crate::chapter_shots::load_shots(&novel_id, &node_id)?;
            Ok(json!({"node_id": node_id, "shots": shots}).to_string())
        }
        "set_chapter_shots" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let shots = args
                .get("shots")
                .cloned()
                .ok_or_else(|| "缺少 shots".to_string())?;
            let shots: Vec<crate::chapter_shots::ChapterShot> =
                serde_json::from_value(shots).map_err(|e| e.to_string())?;
            let shots = crate::chapter_shots::save_shots(&novel_id, &node_id, shots)?;
            Ok(json!({"node_id": node_id, "shots": shots}).to_string())
        }
        "split_chapter_shots" => mcp_split_chapter_shots_brief(&ctx, args),
        "generate_shot_comfy_prompts" => mcp_shot_comfy_prompts_brief(&ctx, args),
        "submit_chapter_shots_comfyui" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let result = submit_chapter_shots_comfyui_inner(&ctx.db, &novel_id, &node_id).await?;
            Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        "get_chapter_memory" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let items = get_chapter_memory_inner(&ctx.db, &novel_id, &node_id)?;
            Ok(json!({"node_id": node_id, "items": items}).to_string())
        }
        "list_chapter_memory" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let groups = list_all_chapter_memory_inner(&ctx.db, &novel_id)?;
            Ok(serde_json::to_string_pretty(&groups).unwrap_or_default())
        }
        "set_chapter_memory" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let items = arg_str_vec(&args, "items").unwrap_or_default();
            set_chapter_memory_inner(&ctx.db, &novel_id, &node_id, items).await?;
            Ok(json!({"ok": true}).to_string())
        }
        "regenerate_chapter_memory" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let user_notes = args
                .get("user_notes")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let memory_node_ids = arg_str_vec(&args, "memory_node_ids");
            let app = ctx.app.lock().unwrap().clone();
            let items = regenerate_chapter_memory_inner(
                &ctx.db,
                app.as_ref(),
                novel_id,
                node_id,
                user_notes,
                memory_node_ids,
                None,
            )
            .await?;
            Ok(json!({"items": items}).to_string())
        }
        "upsert_character_card" => run_maybe_batch(args, |a| upsert_character(&ctx, a)),
        "get_character_card" => get_character_card(&ctx, args),
        "upsert_plot_card" => run_maybe_batch(args, |a| upsert_plot(&ctx, a)),
        "upsert_knowledge_card" => run_maybe_batch(args, |a| upsert_knowledge_card(&ctx, a)),
        "fill_knowledge_card" => fill_knowledge_card(&ctx, args),
        "list_public_knowledge_cards" => {
            let include_archived = args
                .get("include_archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut cards = ctx
                .db
                .list_public_knowledge_cards()
                .map_err(|e| e.to_string())?;
            if !include_archived {
                cards.retain(|c| !c.archived);
            }
            Ok(serde_json::to_string_pretty(&cards).unwrap_or_default())
        }
        "upsert_public_knowledge_card" => {
            run_maybe_batch(args, |a| upsert_public_knowledge_card(&ctx, a))
        }
        "archive_public_knowledge_card" => {
            let id = arg_str(&args, "id")?;
            let archived = args.get("archived").and_then(|v| v.as_bool()).unwrap_or(true);
            ctx.db
                .set_public_knowledge_card_archived(&id, archived)
                .map_err(|e| e.to_string())?;
            let card = ctx
                .db
                .get_public_knowledge_card(&id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "公共知识卡不存在".to_string())?;
            Ok(serde_json::to_string_pretty(&card).unwrap_or_default())
        }
        "add_public_knowledge_card" => {
            run_maybe_batch(args, |a| add_public_knowledge_card(&ctx, a))
        }
        "link_nodes" => link_nodes(&ctx, args),
        "unlink_nodes" => unlink_nodes(&ctx, args),
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn emit_tree_changed(ctx: &McpCtx, novel_id: &str) {
    if let Ok(g) = ctx.app.lock() {
        if let Some(h) = g.as_ref() {
            let _ = h.emit("novel-tree-changed", json!({ "novelId": novel_id }));
        }
    }
}

fn emit_chapter_content_changed(ctx: &McpCtx, novel_id: &str, node_id: &str, before: &str) {
    if let Ok(g) = ctx.app.lock() {
        if let Some(h) = g.as_ref() {
            let _ = h.emit(
                "chapter-content-changed",
                json!({ "novelId": novel_id, "nodeId": node_id, "before": before }),
            );
        }
    }
}

fn save_tree_notify(ctx: &McpCtx, mut tree: NovelTree, layout: bool) -> Result<(), String> {
    if layout {
        crate::tree_layout::apply_auto_layout(&mut tree);
    }
    let id = tree.novel_id.clone();
    save_tree(tree)?;
    emit_tree_changed(ctx, &id);
    Ok(())
}

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing {key}"))
}

/// 字符串或正整数（模型常把章号写成 `3` 而不是 `"第3章"`）。
fn json_node_spec(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(s.to_string());
    }
    if let Some(n) = v.as_u64() {
        return Some(n.to_string());
    }
    v.as_i64().filter(|n| *n > 0).map(|n| n.to_string())
}

fn workspace_sel(ctx: &McpCtx) -> Option<(String, String)> {
    ctx.selection.lock().unwrap().clone()
}

/// 参数优先；缺省用工作台当前选中。
fn arg_or(args: &Value, key: &str, fallback: Option<String>) -> Result<String, String> {
    arg_str(args, key).or_else(|_| {
        fallback
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("missing {key}：请传入，或在工作台选中一张卡片"))
    })
}

fn arg_novel_id(ctx: &McpCtx, args: &Value) -> Result<String, String> {
    arg_or(args, "novel_id", workspace_sel(ctx).map(|(n, _)| n))
}

fn arg_node_id(ctx: &McpCtx, args: &Value) -> Result<String, String> {
    let raw = json_node_spec(args.get("node_id"))
        .or_else(|| json_node_spec(args.get("n")))
        .or_else(|| workspace_sel(ctx).map(|(_, id)| id))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "missing node_id：请传入树上 id、「第N章」或标题，或在工作台选中一张卡片".to_string()
        })?;
    let novel_id = arg_novel_id(ctx, args)?;
    let tree = get_tree(novel_id)?;
    resolve_node_ref(&tree, &raw)
}

/// upsert 用：有 node_id 则解析到树上已有节点；省略表示新建（不用当前选中）。
fn optional_resolved_node_id(tree: &NovelTree, args: &Value) -> Result<Option<String>, String> {
    match json_node_spec(args.get("node_id")) {
        Some(raw) => Ok(Some(resolve_node_ref(tree, &raw)?)),
        None => Ok(None),
    }
}

fn update_novel(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut novel = ctx
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut tree = get_tree(novel_id.clone())?;
    if let Some(t) = args.get("title").and_then(|v| v.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            novel.title = t.to_string();
            if let Some(root) = tree.nodes.iter_mut().find(|n| matches!(n.kind, NodeKind::Novel)) {
                root.label = t.to_string();
            }
        }
    }
    if let Some(s) = args.get("synopsis").and_then(|v| v.as_str()) {
        novel.synopsis = s.to_string();
        if let Some(root) = tree.nodes.iter_mut().find(|n| matches!(n.kind, NodeKind::Novel)) {
            root.outline = s.to_string();
        }
    }
    let wmin = args
        .get("word_count_min")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(novel.word_count_min);
    let wmax = args
        .get("word_count_max")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(novel.word_count_max);
    novel.word_count_min = wmin.max(1);
    novel.word_count_max = wmax.max(novel.word_count_min);
    if let Some(root) = tree.nodes.iter_mut().find(|n| matches!(n.kind, NodeKind::Novel)) {
        root.word_count_min = novel.word_count_min;
        root.word_count_max = novel.word_count_max;
    }
    if let Some(n) = args.get("chapter_count").and_then(|v| v.as_u64()) {
        let n = (n as u32).clamp(1, 500);
        novel.chapter_count = n;
        if let Some(root) = tree.nodes.iter_mut().find(|n| matches!(n.kind, NodeKind::Novel)) {
            root.chapter_count = n;
        }
    }
    novel.updated_at = Utc::now().to_rfc3339();
    if let Some(f) = args.get("features") {
        if let Ok(feat) = serde_json::from_value::<crate::models::NovelFeatures>(f.clone()) {
            novel.features = feat;
        }
    }
    ctx.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    let _ = std::fs::write(
        novel_meta_path(&novel.id),
        serde_json::to_string_pretty(&novel).unwrap_or_default(),
    );
    save_tree_notify(ctx, tree, false)?;
    Ok(serde_json::to_string_pretty(&novel).unwrap_or_default())
}

fn layout_tree(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    crate::tree_layout::apply_auto_layout(&mut tree);
    let n = tree.nodes.len();
    save_tree_notify(ctx, tree, false)?;
    Ok(json!({ "ok": true, "nodes": n }).to_string())
}

fn volume_json_entry(tree: &NovelTree, n: &TreeNode) -> Value {
    let payload = n.volume.clone().unwrap_or_default();
    let formatted = crate::volume_fmt::format_volume_full(&payload);
    let plot_ids = tree_links::union_linked_ids(
        tree,
        &n.id,
        &n.linked_side_plot_ids,
        NodeKind::SidePlot,
    );
    let char_ids = tree_links::union_linked_ids(
        tree,
        &n.id,
        &n.linked_character_ids,
        NodeKind::Character,
    );
    let knowledge_ids = tree_links::union_linked_ids(
        tree,
        &n.id,
        &n.linked_knowledge_ids,
        NodeKind::Knowledge,
    );
    json!({
        "id": n.id,
        "label": n.label,
        "volume": payload,
        "formatted": formatted,
        "linked_character_ids": char_ids,
        "linked_side_plot_ids": plot_ids,
        "linked_knowledge_ids": knowledge_ids,
        "linked_plots": json_linked_plots(tree, &plot_ids),
        "linked_characters": json_linked_characters(tree, &char_ids),
        "linked_knowledge": json_linked_knowledge(tree, &knowledge_ids),
    })
}

fn patch_volume_payload(p: &mut VolumePayload, args: &Value) -> Result<(), String> {
    let mut base = serde_json::to_value(&*p).map_err(|e| e.to_string())?;
    if let Some(v) = args.get("volume") {
        deep_merge_json(&mut base, v);
    }
    for key in [
        "positioning",
        "layer_setup",
        "layer_confrontation",
        "layer_resolution",
        "conflict_external",
        "conflict_internal",
        "conflict_deep",
        "key_beats",
    ] {
        if let Some(v) = args.get(key) {
            deep_merge_json(&mut base, &json!({ key: v }));
        }
    }
    *p = serde_json::from_value(base).map_err(|e| format!("volume: {e}"))?;
    Ok(())
}

fn add_volume(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id.clone())?;
    let n = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Volume))
        .count()
        + 1;
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("第 {n} 卷"));
    let mut payload = VolumePayload::default();
    patch_volume_payload(&mut payload, &args)?;
    let id = format!("vol-{}", Uuid::new_v4());
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .ok_or_else(|| "根节点不存在".to_string())?;
    let max_y = tree
        .nodes
        .iter()
        .map(|n| n.position.y)
        .fold(root.position.y, f64::max);
    let node = TreeNode {
        id: id.clone(),
        kind: NodeKind::Volume,
        label: title,
        outline: String::new(),
        detailed_outline: vec![],
        character: None,
        knowledge: None,
        side_plot: None,
        volume: Some(payload),
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition {
            x: root.position.x,
            y: max_y + 140.0,
        },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    };
    let root_id = root.id.clone();
    tree.nodes.push(node);
    link_pair(&mut tree, &root_id, &id, "volume")?;
    save_tree_notify(ctx, tree.clone(), false)?;
    let added = tree.nodes.iter().find(|n| n.id == id).unwrap();
    Ok(serde_json::to_string_pretty(&volume_json_entry(&tree, added)).unwrap_or_default())
}

fn get_volume(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let tree = get_tree(novel_id)?;
    let node_id = optional_resolved_node_id(&tree, &args)?;
    if let Some(id) = node_id {
        let n = tree
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| "节点不存在".to_string())?;
        if !matches!(n.kind, NodeKind::Volume) {
            return Err("不是分卷".into());
        }
        return Ok(serde_json::to_string_pretty(&json!({
            "volume": volume_json_entry(&tree, n),
            "ai_guidance": "修改结构化字段用 upsert_volume（volume / positioning / layer_* / conflict_* / key_beats）。勿写 outline；写作注入用 formatted。关联剧情/知识/人物仅改字段，勿 unlink 固定关联除非用户要求。",
        }))
        .unwrap_or_default());
    }
    let mut vols: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Volume))
        .collect();
    vols.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let list: Vec<Value> = vols.iter().map(|v| volume_json_entry(&tree, v)).collect();
    Ok(serde_json::to_string_pretty(&json!({
        "count": list.len(),
        "volumes": list,
    }))
    .unwrap_or_default())
}

fn upsert_volume(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    let n = tree
        .nodes
        .iter_mut()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if !matches!(n.kind, NodeKind::Volume) {
        return Err("不是分卷".into());
    }
    if let Some(title) = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        n.label = title.to_string();
    }
    let mut payload = n.volume.clone().unwrap_or_default();
    patch_volume_payload(&mut payload, &args)?;
    n.volume = Some(payload);
    save_tree_notify(ctx, tree.clone(), false)?;
    let out = tree.nodes.iter().find(|x| x.id == node_id).unwrap();
    Ok(serde_json::to_string_pretty(&volume_json_entry(&tree, out)).unwrap_or_default())
}

fn add_chapter(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id.clone())?;
    let n = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .count()
        + 1;
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("第 {n} 章"));
    let outline = args
        .get("outline")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let id = format!("ch-{}", Uuid::new_v4());
    let link_to = args
        .get("link_to")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let (host, x, y0) = if let Some(ref lt) = link_to {
        let host = resolve_link_host(&tree, lt)?;
        let hn = tree
            .nodes
            .iter()
            .find(|n| n.id == host)
            .ok_or_else(|| "宿主不存在".to_string())?;
        if matches!(hn.kind, NodeKind::Volume) {
            // 挂到该卷末章，否则挂分卷本身
            let mut under: Vec<&TreeNode> = tree
                .nodes
                .iter()
                .filter(|n| {
                    matches!(n.kind, NodeKind::Chapter)
                        && chapter_parent_volume_id(&tree, &n.id).as_deref() == Some(host.as_str())
                })
                .collect();
            under.sort_by(|a, b| {
                a.position
                    .y
                    .partial_cmp(&b.position.y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(last) = under.last() {
                (last.id.clone(), last.position.x, last.position.y)
            } else {
                (host, hn.position.x, hn.position.y)
            }
        } else if matches!(hn.kind, NodeKind::Chapter | NodeKind::Novel) {
            (host, hn.position.x, hn.position.y)
        } else {
            last_chapter_anchor(&tree)
        }
    } else {
        last_chapter_anchor(&tree)
    };
    let node = TreeNode {
        id: id.clone(),
        kind: NodeKind::Chapter,
        label: title,
        outline,
        detailed_outline: vec![],
        character: None,
        knowledge: None,
        side_plot: None,
        volume: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition {
            x,
            y: y0 + 140.0,
        },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    };
    tree.nodes.push(node);
    link_pair(&mut tree, &host, &id, "chapter")?;
    save_tree_notify(ctx, tree.clone(), false)?;
    let added = tree.nodes.iter().find(|n| n.id == id).cloned();
    Ok(serde_json::to_string_pretty(&added).unwrap_or_default())
}

fn json_linked_plots_tagged(
    tree: &NovelTree,
    ids: &[(String, &'static str)],
) -> Vec<Value> {
    ids.iter()
        .enumerate()
        .filter_map(|(order, (id, from))| {
            let p = tree.nodes.iter().find(|n| n.id == *id)?;
            Some(json!({
                "order": order,
                "id": p.id,
                "label": p.label,
                "outline": p.outline,
                "status": p.side_plot.as_ref().map(|s| s.status.clone()).unwrap_or_else(|| "active".into()),
                "absorbed": p.side_plot.as_ref().map(|s| s.absorbed).unwrap_or(false),
                "inherited_from": from,
                "inherited_from_root": *from == "root",
            }))
        })
        .collect()
}

fn json_linked_plots_legacy(tree: &NovelTree, ids: &[String]) -> Vec<Value> {
    ids.iter()
        .enumerate()
        .filter_map(|(order, id)| {
            let p = tree.nodes.iter().find(|n| n.id == *id)?;
            Some(json!({
                "order": order,
                "id": p.id,
                "label": p.label,
                "outline": p.outline,
                "status": p.side_plot.as_ref().map(|s| s.status.clone()).unwrap_or_else(|| "active".into()),
                "absorbed": p.side_plot.as_ref().map(|s| s.absorbed).unwrap_or(false),
            }))
        })
        .collect()
}

fn json_linked_plots(tree: &NovelTree, ids: &[String]) -> Vec<Value> {
    json_linked_plots_legacy(tree, ids)
}

fn json_linked_characters(tree: &NovelTree, ids: &[String]) -> Vec<Value> {
    ids.iter()
        .enumerate()
        .filter_map(|(order, id)| {
            let c = tree.nodes.iter().find(|n| n.id == *id)?;
            Some(character_json_entry(tree, c, Some(order)))
        })
        .collect()
}

fn character_json_entry(tree: &NovelTree, n: &TreeNode, order: Option<usize>) -> Value {
    let formatted = n
        .character
        .as_ref()
        .map(|c| crate::character_fmt::format_character_full(&n.label, c))
        .unwrap_or_default();
    let mut ch = serde_json::to_value(&n.character).unwrap_or(Value::Null);
    if let Some(obj) = ch.as_object_mut() {
        obj.remove("personality");
    }
    let mut obj = json!({
        "id": n.id,
        "label": n.label,
        "true_name": n.label,
        "character": ch,
        "formatted": formatted,
    });
    if let Some(o) = order {
        obj["order"] = json!(o);
    }
    if matches!(n.kind, NodeKind::Character) {
        obj["character_relations"] = json!(character_relations_json(tree, &n.id));
    }
    obj
}

fn character_relations_json(tree: &NovelTree, char_id: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for e in &tree.edges {
        if e.kind != "character" {
            continue;
        }
        let other = if e.source == char_id {
            e.target.as_str()
        } else if e.target == char_id {
            e.source.as_str()
        } else {
            continue;
        };
        let peer = tree.nodes.iter().find(|n| n.id == other);
        if !matches!(peer, Some(p) if matches!(p.kind, NodeKind::Character)) {
            continue;
        }
        let label = peer.map(|p| p.label.as_str()).unwrap_or(other);
        out.push(json!({
            "peer_id": other,
            "peer_label": label,
            "relation": e.label,
        }));
    }
    out
}

fn deep_merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                if matches!(
                    k.as_str(),
                    "id" | "label" | "true_name" | "formatted" | "character_relations" | "order"
                ) {
                    continue;
                }
                // get_character_card 条目再套一层 character → 摊平合并进当前对象
                if k == "character" {
                    if let Value::Object(inner) = v {
                        for (ik, iv) in inner {
                            match base_map.get_mut(ik) {
                                Some(existing) if existing.is_object() && iv.is_object() => {
                                    deep_merge_json(existing, iv);
                                }
                                _ => {
                                    base_map.insert(ik.clone(), iv.clone());
                                }
                            }
                        }
                    }
                    continue;
                }
                match base_map.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => {
                        deep_merge_json(existing, v);
                    }
                    _ => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// 从 MCP 参数解析人物卡：支持 CharacterCard 本体，或 get_character_card 返回的条目包装。
fn coerce_character_patch(v: &Value) -> Result<Value, String> {
    if let Some(inner) = v.get("character") {
        if inner.is_object()
            && (v.get("formatted").is_some()
                || v.get("id").is_some()
                || v.get("true_name").is_some()
                || v.get("label").is_some())
        {
            return Ok(inner.clone());
        }
    }
    if v.is_object() {
        Ok(v.clone())
    } else {
        Err("character 须为对象".into())
    }
}

/// MCP 人物补丁：丢掉 personality；顶层 body_language 并进 voice。
fn fold_character_mcp_patch(patch: &mut Value) {
    let Some(obj) = patch.as_object_mut() else {
        return;
    };
    obj.remove("personality");
    if let Some(inner) = obj.get_mut("character") {
        fold_character_mcp_patch(inner);
    }
    if let Some(bl) = obj.remove("body_language") {
        let voice = obj.entry("voice").or_insert_with(|| json!({}));
        if let Some(vo) = voice.as_object_mut() {
            vo.insert("body_language".into(), bl);
        }
    }
}

fn patch_character_card_from_args(
    card: &mut CharacterCard,
    args: &Value,
    label: &str,
) -> Result<(), String> {
    let mut base = serde_json::to_value(&*card).map_err(|e| e.to_string())?;
    if let Some(ch) = args.get("character") {
        let mut patch = coerce_character_patch(ch)?;
        fold_character_mcp_patch(&mut patch);
        deep_merge_json(&mut base, &patch);
    }
    // 顶层也可直接传结构化字段（与 character 内同名键深度合并）
    for key in [
        "world_position",
        "world_anchors",
        "relations",
        "core_belief",
        "deep",
        "voice",
    ] {
        if let Some(v) = args.get(key) {
            deep_merge_json(&mut base, &json!({ key: v }));
        }
    }
    if let Some(s) = args.get("body_language").and_then(|v| v.as_str()) {
        deep_merge_json(&mut base, &json!({ "voice": { "body_language": s } }));
    }
    // 扁平遗留字段覆盖（无 personality）
    let mut flat = serde_json::Map::new();
    for key in [
        "role",
        "gender",
        "age",
        "aliases",
        "alignment",
        "style",
        "motto",
        "constraints",
    ] {
        if let Some(s) = args.get(key).and_then(|v| v.as_str()) {
            flat.insert(key.into(), json!(s));
        }
    }
    if !flat.is_empty() {
        deep_merge_json(&mut base, &Value::Object(flat));
    }
    *card = serde_json::from_value(base).map_err(|e| format!("character: {e}"))?;
    crate::character_fmt::sync_character_legacy(label, card);
    Ok(())
}

fn get_character_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let tree = get_tree(novel_id)?;
    let node_id = optional_resolved_node_id(&tree, &args)?;
    if let Some(id) = node_id {
        let n = tree
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| "节点不存在".to_string())?;
        if !matches!(n.kind, NodeKind::Character) {
            return Err("不是人物卡".into());
        }
        return Ok(serde_json::to_string_pretty(&json!({
            "character": character_json_entry(&tree, n, None),
            "ai_guidance": "再改用 upsert_character_card 部分字段；不要把整卡贴回对话。",
        }))
        .unwrap_or_default());
    }
    let chars: Vec<Value> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .map(|n| {
            json!({
                "id": n.id,
                "label": n.label,
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&json!({
        "count": chars.len(),
        "characters": chars,
        "ai_guidance": "此为 id/label 列表。读完整卡请带 node_id。",
    }))
    .unwrap_or_default())
}

fn upsert_card_ack(kind: &str, id: &str, label: &str, created: bool, extra: Value) -> String {
    let mut obj = json!({
        "ok": true,
        "created": created,
        "node_id": id,
        "label": label,
        "kind": kind,
        "ai_guidance": "已写入。勿把整卡贴回对话；再改同一张卡继续 upsert，省略未改字段。",
    });
    if let (Some(dst), Some(src)) = (obj.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    serde_json::to_string_pretty(&obj).unwrap_or_default()
}

fn json_linked_knowledge(tree: &NovelTree, ids: &[String]) -> Vec<Value> {
    ids.iter()
        .enumerate()
        .filter_map(|(order, id)| {
            let k = tree.nodes.iter().find(|n| n.id == *id)?;
            if !matches!(k.kind, NodeKind::Knowledge) {
                return None;
            }
            let kn = k.knowledge.as_ref();
            // 核心法则等：拼完整注入正文，勿只返回可能过期/截断的 extracted
            let extracted = crate::core_laws_fmt::knowledge_card_inject_body(tree, k);
            let slot = kn.map(|x| x.slot.clone()).unwrap_or_default();
            let mut entry = json!({
                "order": order,
                "id": k.id,
                "label": k.label,
                "outline": k.outline,
                "extract_prompt": kn.map(|x| x.extract_prompt.clone()).unwrap_or_default(),
                "extracted": extracted,
                "formatted": extracted,
                "slot": slot,
                "book_ids": kn.map(|x| x.book_ids.clone()).unwrap_or_default(),
            });
            if let Some(obj) = entry.as_object_mut() {
                if let Some(p) = kn {
                    insert_knowledge_structured_fields(obj, p);
                }
                if slot == "story_rules" {
                    obj.insert(
                        "blocks".into(),
                        crate::story_rules_ops::story_rules_blocks_snapshot(tree),
                    );
                }
            }
            Some(entry)
        })
        .collect()
}

fn take_unseen(ids: &[String], seen: &mut HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        if seen.insert(id.clone()) {
            out.push(id.clone());
        }
    }
    out
}

fn json_write_characters(tree: &NovelTree, ids: &[String]) -> Vec<Value> {
    ids.iter()
        .filter_map(|id| {
            let c = tree.nodes.iter().find(|n| n.id == *id)?;
            let formatted = c
                .character
                .as_ref()
                .map(|ch| crate::character_fmt::format_character_full(&c.label, ch))
                .unwrap_or_default();
            Some(json!({
                "id": c.id,
                "label": c.label,
                "formatted": formatted,
                "character_relations": character_relations_json(tree, &c.id),
            }))
        })
        .collect()
}

fn json_write_plots(tree: &NovelTree, ids: &[String], scope: &str) -> Vec<Value> {
    ids.iter()
        .filter_map(|id| {
            let p = tree.nodes.iter().find(|n| n.id == *id)?;
            Some(json!({
                "id": p.id,
                "label": p.label,
                "outline": p.outline,
                "status": p.side_plot.as_ref().map(|s| s.status.clone()).unwrap_or_else(|| "active".into()),
                "absorbed": p.side_plot.as_ref().map(|s| s.absorbed).unwrap_or(false),
                "scope": scope,
            }))
        })
        .collect()
}

fn json_write_knowledge(tree: &NovelTree, ids: &[String], scope: &str) -> Vec<Value> {
    ids.iter()
        .filter_map(|id| {
            let k = tree.nodes.iter().find(|n| n.id == *id)?;
            if !matches!(k.kind, NodeKind::Knowledge) {
                return None;
            }
            let formatted = crate::core_laws_fmt::knowledge_card_inject_body(tree, k);
            let slot = k
                .knowledge
                .as_ref()
                .map(|x| x.slot.clone())
                .unwrap_or_default();
            Some(json!({
                "id": k.id,
                "label": k.label,
                "slot": slot,
                "formatted": formatted,
                "scope": scope,
            }))
        })
        .collect()
}

/// 将 KnowledgeCardPayload 上非空结构化字段摊到 MCP 返回对象（与 §3 / linked_knowledge 一致）。
fn insert_knowledge_structured_fields(
    obj: &mut serde_json::Map<String, Value>,
    p: &KnowledgeCardPayload,
) {
    macro_rules! put {
        ($key:literal, $field:expr) => {
            if let Some(v) = $field.as_ref() {
                obj.insert($key.into(), serde_json::to_value(v).unwrap_or(Value::Null));
            }
        };
    }
    put!("core_laws", p.core_laws);
    put!("spatiotemporal", p.spatiotemporal);
    put!("world_axiom", p.world_axiom);
    put!("key_location", p.key_location);
    put!("social_power", p.social_power);
    put!("world_race", p.world_race);
    put!("major_faction", p.major_faction);
    put!("existence", p.existence);
    put!("info_flow", p.info_flow);
    put!("history_culture", p.history_culture);
    put!("world_religion", p.world_religion);
    put!("major_event", p.major_event);
    put!("surface_setting", p.surface_setting);
    put!("story_engine", p.story_engine);
    put!("fulfillment_system", p.fulfillment_system);
    put!("constraint_redlines", p.constraint_redlines);
}

fn get_selected_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let Some((novel_id, node_id)) = ctx.selection.lock().unwrap().clone() else {
        return Ok(json!({ "selected": false }).to_string());
    };
    let include_content = arg_bool(&args, "include_content", false);
    let tree = get_tree(novel_id.clone())?;
    let Some(node) = find_selected_node(&tree, &node_id).cloned() else {
        return Ok(json!({
            "selected": false,
            "novel_id": novel_id,
            "node_id": node_id,
            "reason": "missing",
        })
        .to_string());
    };
    let content = if include_content
        && matches!(node.kind, NodeKind::Chapter | NodeKind::SidePlot)
    {
        get_chapter(novel_id.clone(), node_id.clone()).unwrap_or_default()
    } else {
        String::new()
    };
    Ok(serde_json::to_string_pretty(&json!({
        "selected": true,
        "novel_id": novel_id,
        "node": node,
        "content": content,
    }))
    .unwrap_or_default())
}

fn find_selected_node<'a>(tree: &'a NovelTree, node_id: &str) -> Option<&'a TreeNode> {
    tree.nodes.iter().find(|n| n.id == node_id)
}

fn novel_json_for_mcp(novel: &NovelProject) -> Value {
    json!({
        "id": novel.id,
        "title": novel.title,
        "synopsis": novel.synopsis,
        "cover_path": novel.cover_path,
        "archived": novel.archived,
        "word_count_min": novel.word_count_min,
        "word_count_max": novel.word_count_max,
        "chapter_count": novel.chapter_count,
        "features": novel.features,
        "created_at": novel.created_at,
        "updated_at": novel.updated_at,
    })
}

fn get_novel_info(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let novel = ctx
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let tree = get_tree(novel_id)?;
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .cloned()
        .ok_or_else(|| "根节点不存在".to_string())?;
    let plot_ids = tree_links::union_linked_ids(
        &tree,
        &root.id,
        &root.linked_side_plot_ids,
        NodeKind::SidePlot,
    );
    let char_ids = tree_links::union_linked_ids(
        &tree,
        &root.id,
        &root.linked_character_ids,
        NodeKind::Character,
    );
    let knowledge_ids = tree_links::union_linked_ids(
        &tree,
        &root.id,
        &root.linked_knowledge_ids,
        NodeKind::Knowledge,
    );
    let volumes: Vec<Value> = {
        let mut vols: Vec<&TreeNode> = tree
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Volume))
            .collect();
        vols.sort_by(|a, b| {
            a.position
                .y
                .partial_cmp(&b.position.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        vols.iter()
            .map(|v| {
                json!({
                    "id": v.id,
                    "label": v.label,
                    "volume": v.volume,
                    "formatted": v
                        .volume
                        .as_ref()
                        .map(crate::volume_fmt::format_volume_full)
                        .unwrap_or_default(),
                })
            })
            .collect()
    };
    Ok(serde_json::to_string_pretty(&json!({
        "ai_guidance": {
            "features": "novel.features 为根节点功能选项（题材/核心玩法/风格/关系/受众）。生成世界观必须遵守；写章口吻与爽点也应贴合。",
            "plots": "linked_plots 为根节点关联的跨章剧情卡，按 order（0 最先）理解全书主线；写章时与分卷/章节卡上的剧情一并遵守，不得改写既定要点。分卷可选，见 volumes。",
            "characters": "linked_characters 含完整结构化 character（world_position / world_anchors / relations / core_belief / deep / voice.body_language 等）与 formatted 全文（写章注入同形）。无 personality。须符合人设与 character_relations；formatted 非空时严格遵守。",
            "knowledge": "linked_knowledge 为根上知识卡（全书写作约束）。优先 extracted；若为空则遵守 extract_prompt 与 outline。公共知识库不是小说设定源，只有挂到树上的知识卡才约束写作。"
        },
        "novel": novel_json_for_mcp(&novel),
        "features_text": crate::novel_features::format_novel_features_block(&novel.features),
        "node": root,
        "volumes": volumes,
        "linked_plots": json_linked_plots(&tree, &plot_ids),
        "linked_characters": json_linked_characters(&tree, &char_ids),
        "linked_knowledge": json_linked_knowledge(&tree, &knowledge_ids),
        "linked_character_ids": char_ids,
        "linked_side_plot_ids": plot_ids,
        "linked_knowledge_ids": knowledge_ids,
    }))
    .unwrap_or_default())
}

fn get_worldview(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let novel = ctx
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let tree = get_tree(novel_id)?;
    Ok(serde_json::to_string_pretty(&json!({
        "features": novel.features,
        "features_text": crate::novel_features::format_novel_features_block(&novel.features),
        "snapshot": worldview_snapshot_value(&tree),
        "story_rules_blocks": crate::story_rules_ops::story_rules_blocks_snapshot(&tree),
    }))
    .unwrap_or_default())
}

fn get_story_rules(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let tree = get_tree(novel_id)?;
    let rules = tree
        .nodes
        .iter()
        .find(|n| {
            matches!(n.kind, NodeKind::Knowledge)
                && n.knowledge
                    .as_ref()
                    .map(|k| k.slot.trim() == "story_rules")
                    .unwrap_or(false)
        });
    let formatted = rules
        .map(|n| crate::story_rules_fmt::format_story_rules_full(&tree, n))
        .unwrap_or_default();
    let parent = rules.map(|n| {
        let k = n.knowledge.as_ref();
        json!({
            "node_id": n.id,
            "label": n.label,
            "extracted": k.map(|x| x.extracted.clone()).unwrap_or_default(),
            "formatted": formatted,
        })
    });
    let mut cards = Vec::new();
    for (slot, title) in crate::story_rules_ops::FAN {
        if let Some(n) = tree.nodes.iter().find(|n| {
            matches!(n.kind, NodeKind::Knowledge)
                && n.knowledge
                    .as_ref()
                    .map(|k| k.slot.trim() == slot)
                    .unwrap_or(false)
        }) {
            let kn = n.knowledge.as_ref();
            let body = crate::core_laws_fmt::knowledge_card_inject_body(&tree, n);
            let mut card = json!({
                "node_id": n.id,
                "slot": slot,
                "title": title,
                "label": n.label,
                "formatted": body,
            });
            if let Some(obj) = card.as_object_mut() {
                if let Some(p) = kn {
                    if let Some(v) = p.surface_setting.as_ref() {
                        obj.insert("surface_setting".into(), serde_json::to_value(v).unwrap_or(Value::Null));
                    }
                    if let Some(v) = p.story_engine.as_ref() {
                        obj.insert("story_engine".into(), serde_json::to_value(v).unwrap_or(Value::Null));
                    }
                    if let Some(v) = p.fulfillment_system.as_ref() {
                        obj.insert("fulfillment_system".into(), serde_json::to_value(v).unwrap_or(Value::Null));
                    }
                    if let Some(v) = p.constraint_redlines.as_ref() {
                        obj.insert("constraint_redlines".into(), serde_json::to_value(v).unwrap_or(Value::Null));
                    }
                }
            }
            cards.push(card);
        }
    }
    Ok(serde_json::to_string_pretty(&json!({
        "blocks": crate::story_rules_ops::story_rules_blocks_snapshot(&tree),
        "formatted": formatted,
        "parent": parent,
        "cards": cards,
        "ai_guidance": "改写四卡用 apply_story_rules(blocks) 或 upsert_knowledge_card(slot + surface_setting|story_engine|…)。缺卡先 ensure_worldview。整套生成用 generate_story_rules。",
    }))
    .unwrap_or_default())
}

fn apply_story_rules(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let blocks = args
        .get("blocks")
        .cloned()
        .ok_or_else(|| "缺少 blocks".to_string())?;
    let mut tree = get_tree(novel_id)?;
    crate::story_rules_ops::apply_story_rules_payload(&mut tree, &blocks)?;
    save_tree_notify(ctx, tree, false)?;
    get_story_rules(ctx, args)
}

async fn generate_story_rules(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let instruction = arg_str(&args, "instruction")?;
    let apply = args
        .get("apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let model = args
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let app = ctx.app.lock().unwrap().clone();
    let messages = vec![ChatTurn {
        role: "user".into(),
        content: instruction,
    }];
    let result = generate_story_rules_chat_inner(
        app.as_ref(),
        &ctx.db,
        &novel_id,
        &messages,
        model.as_deref(),
        None,
    )
    .await?;
    if apply {
        let mut tree = get_tree(novel_id.clone())?;
        crate::story_rules_ops::apply_story_rules_payload(&mut tree, &result.blocks)?;
        save_tree_notify(ctx, tree, false)?;
    }
    Ok(serde_json::to_string_pretty(&json!({
        "assistant": result.assistant,
        "applied": apply,
        "blocks": result.blocks,
    }))
    .unwrap_or_default())
}

fn ensure_worldview(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    let created = crate::worldview_ops::ensure_worldview_cards(&mut tree);
    let fan = crate::story_rules_ops::ensure_story_rules_fan_cards(&mut tree);
    save_tree_notify(ctx, tree, false)?;
    Ok(json!({ "ok": true, "created": created || fan }).to_string())
}

fn apply_worldview(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let worldview = args
        .get("worldview")
        .cloned()
        .ok_or_else(|| "缺少 worldview".to_string())?;
    let mut tree = get_tree(novel_id)?;
    crate::worldview_ops::apply_worldview_payload(&mut tree, &worldview)?;
    save_tree_notify(ctx, tree, false)?;
    Ok(json!({ "ok": true }).to_string())
}

async fn generate_worldview(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let instruction = arg_str(&args, "instruction")?;
    let apply = args
        .get("apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let model = args
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let app = ctx.app.lock().unwrap().clone();
    let messages = vec![ChatTurn {
        role: "user".into(),
        content: instruction,
    }];
    let slot = args.get("slot").and_then(|v| v.as_str());
    let result = generate_worldview_chat_inner(
        app.as_ref(),
        &ctx.db,
        &novel_id,
        &messages,
        model.as_deref(),
        slot,
        None,
    )
    .await?;
    if apply {
        let mut tree = get_tree(novel_id.clone())?;
        crate::worldview_ops::apply_worldview_payload(&mut tree, &result.worldview)?;
        save_tree_notify(ctx, tree, false)?;
    }
    Ok(serde_json::to_string_pretty(&json!({
        "assistant": result.assistant,
        "applied": apply,
        "worldview": result.worldview,
    }))
    .unwrap_or_default())
}

fn chapter_write_ai_guidance() -> Value {
    json!({
        "plots": "linked_plots 的 inherited_from 为 root|volume|chapter：根/分卷继承按各自 order；本章剧情严格按 linked_side_plot_ids 的 order 0→N。不得混序或改写要点。章节继承根 ∪ 所属分卷（若有）。",
        "characters": "linked_characters 为本章必须出场的人物；正文中须全部出现，并符合 character 结构化字段与 formatted 全文；人物关系见 character_relations。",
        "knowledge": "linked_knowledge 为写作硬约束。世界观/故事规则与根、卷直连知识不向章继承；写章用 get_chapter_write_context 分层取根→卷→章并去重。根上 slot=write_prompts（生成/精修）及其左右子卡不在 linked_knowledge 里，由应用接到用户提示词后。优先遵守 formatted。禁止另起风格。",
        "lore": "落盘前对照 get_chapter_write_context 根层世界观六卡与故事规则：若正文违背硬设定（法则、地理、权力、生死历法、信息规则、故事红线等），就地修订冲突句段，禁止整章重写或另起主线；无冲突则不要为「校验」改文。",
        "narrative_coherence": "生成本章正文须保证叙事合理、连贯、可读，并与 node.outline（简纲）、node.detailed_outline（细纲，优先）及前序章节记忆（若已提供）一致。①时间与因果：事件按可理解的时间顺序展开；后文不得推翻前文已确立的事实；场景/视角切换须有可感知过渡。②人物一致：决策与言行须符合人设与当前处境、认知；禁止无铺垫的性格、立场、关系或能力突变。③细节一致：人名、称谓、地名、物品、数量、伤势、天气、时辰等须前后统一。④段落衔接：相邻段落须有因果、时间或空间上的延续；禁止硬切、跳剪式跳跃导致读者无法重建过程。⑤逻辑自洽：禁止为推剧情而强行降智、反常行为或违背常识的设定（除非章纲/知识卡明确为世界观规则）。⑥节奏与情绪：变化须符合简纲/细纲与剧情卡推进，禁止情绪或基调无源反转。⑦信息有效：每段应推进情节或刻画人物/氛围，禁止无意义同义反复与凑字数。",
        "forbidden": "严禁：前后段落、场景或时间线逻辑冲突；同一事实在章内前后矛盾；人物言行与人设/当前处境不符且无解释；未在简纲/细纲或 linked_plots 中出现的重大新设定、无关支线或另起主线；缺乏因果铺垫的「机械降神」式巧合解决核心矛盾（除非大纲明确要求）；场景硬切、对话说明文式生硬灌设定；与 linked_plots 顺序或要点相悖的叙述；复制粘贴式重复段落；打破第四面墙；输出写作过程、自我评价、提纲清单或评分。",
        "length": "正文字数瞄准 get_chapter_write_context 的 word_count_min/max 中位，按细纲条数均分篇幅。±60 只是标注带，不是反复重写门槛。一次写完；偏长只删冗、偏短只补场面。set_chapter_content 若 in_band=true 禁止再为字数改正文；明显超限也只改一轮，禁止整章重写。",
        "output": "只输出本章 Markdown 正文（可用 `# 章标题` 开头，或直接正文）；禁止输出注释、写作说明、「本章完」、检查清单或自我评分。",
        "content_usage": "写章用 get_chapter_write_context（不含正文）。本会话已提交过根则 include_root=false；同卷已提交则 include_volume=false；不要新开 session。生成新章：细纲空则先 generate_detailed_outline；禁止 get_chapter_content。精修：再 get_chapter_content 读旧稿后改。落盘前对照根层世界观/故事规则自检一轮（见 lore）。"
    })
}

fn shot_split_ai_guidance() -> &'static str {
    "本工具不调用应用内置 LLM，不写入镜头。一章必须拆成多条连续镜头（通常 6–20；短章至少 2 镜）。禁止把整章收成 1 条。按场面转换、对白轮次、机位变化切开；每镜 5–15 秒；不发明正文没有的情节；对话原句保留。用 body / characters 拆完后 set_chapter_shots 整表覆盖（shots 为数组）。字段：action、camera、dialogue（可空）、duration_sec、comfy_prompt（拆镜时可空）。"
}

fn shot_comfy_prompt_ai_guidance() -> &'static str {
    "本工具不调用应用内置 LLM，不写入镜头。你必须为每镜写 cinematic comfy_prompt（英文为主、专名可中文：外貌与人物卡一致、环境、动作、镜头运动、光线；有对白则写成 spoken line），再 set_chapter_shots 整表写回（保留 id/order/action/camera/dialogue/duration_sec）。禁止网文腔与抽象情绪堆砌，禁止发明分镜以外的情节。"
}

fn mcp_split_chapter_shots_brief(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let tree = get_tree(novel_id.clone())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "章节不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节可拆分镜头".into());
    }
    let body = get_chapter(novel_id.clone(), node_id.clone())?;
    if body.trim().is_empty() {
        return Err("请先写好本章正文".into());
    }
    let existing = crate::chapter_shots::load_shots(&novel_id, &node_id)?;
    let characters = shot_character_looks(&tree, &node_id);
    Ok(json!({
        "node_id": node_id,
        "label": node.label,
        "body": body,
        "characters": characters,
        "existing_shots": existing,
        "saved": false,
        "shot_fields": ["id", "order", "action", "camera", "dialogue", "duration_sec", "comfy_prompt"],
        "min_shots": 2,
        "typical_shots": "6-20",
        "ai_guidance": shot_split_ai_guidance(),
    })
    .to_string())
}

fn mcp_shot_comfy_prompts_brief(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let tree = get_tree(novel_id.clone())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "章节不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节可写分镜提示词".into());
    }
    let shots = crate::chapter_shots::load_shots(&novel_id, &node_id)?;
    if shots.is_empty() {
        return Err("请先拆分镜头".into());
    }
    let characters = shot_character_looks(&tree, &node_id);
    Ok(json!({
        "node_id": node_id,
        "label": node.label,
        "characters": characters,
        "shots": shots,
        "saved": false,
        "ai_guidance": shot_comfy_prompt_ai_guidance(),
    })
    .to_string())
}

fn get_chapter_info(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let tree = get_tree(novel_id.clone())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?
        .clone();
    if !matches!(node.kind, NodeKind::Chapter | NodeKind::SidePlot) {
        return Err("只能查询章节或支线剧情卡".into());
    }

    let (linked_plots, local_plot_ids, root_ids, volume_ids, knowledge_ids, volume_json) =
        if matches!(node.kind, NodeKind::Chapter) {
            let root_ids = root_plot_ids(&tree);
            let vol_id = chapter_parent_volume_id(&tree, &node_id);
            let volume_ids = vol_id
                .as_ref()
                .map(|vid| volume_plot_ids(&tree, vid))
                .unwrap_or_default();
            let local = chapter_local_plot_ids(&tree, &node_id);
            let mut tagged: Vec<(String, &'static str)> = Vec::new();
            for id in &root_ids {
                tagged.push((id.clone(), "root"));
            }
            for id in &volume_ids {
                if !root_ids.iter().any(|r| r == id) {
                    tagged.push((id.clone(), "volume"));
                }
            }
            for id in &local {
                tagged.push((id.clone(), "chapter"));
            }
            let plots = json_linked_plots_tagged(&tree, &tagged);
            let know = chapter_effective_knowledge_ids(&tree, &node_id);
            let volume_json = vol_id.and_then(|vid| {
                tree.nodes.iter().find(|n| n.id == vid).map(|v| {
                    json!({
                        "id": v.id,
                        "label": v.label,
                        "volume": v.volume,
                        "formatted": v
                            .volume
                            .as_ref()
                            .map(crate::volume_fmt::format_volume_full)
                            .unwrap_or_default(),
                    })
                })
            });
            (plots, local, root_ids, volume_ids, know, volume_json)
        } else {
            let local = tree_links::union_linked_ids(
                &tree,
                &node_id,
                &node.linked_side_plot_ids,
                NodeKind::SidePlot,
            );
            let know = tree_links::union_linked_ids(
                &tree,
                &node_id,
                &node.linked_knowledge_ids,
                NodeKind::Knowledge,
            );
            (
                json_linked_plots(&tree, &local),
                local.clone(),
                Vec::<String>::new(),
                Vec::<String>::new(),
                know,
                None,
            )
        };
    let linked_characters = json_linked_characters(&tree, &node.linked_character_ids);
    let linked_knowledge = json_linked_knowledge(&tree, &knowledge_ids);

    Ok(serde_json::to_string_pretty(&json!({
        "ai_guidance": chapter_write_ai_guidance(),
        "node": node,
        "volume": volume_json,
        "linked_plots": linked_plots,
        "linked_characters": linked_characters,
        "linked_knowledge": linked_knowledge,
        "linked_side_plot_ids": local_plot_ids,
        "linked_root_plot_ids": root_ids,
        "linked_volume_plot_ids": volume_ids,
        "linked_knowledge_ids": knowledge_ids,
    }))
    .unwrap_or_default())
}

fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn get_chapter_write_context(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let include_root = arg_bool(&args, "include_root", true);
    let include_volume = arg_bool(&args, "include_volume", true);
    let novel = ctx
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let tree = get_tree(novel_id)?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("只能用于章节".into());
    }

    let mut seen_chars = HashSet::new();
    let mut seen_plots = HashSet::new();
    let mut seen_know = HashSet::new();

    let root_chars = root_character_ids(&tree);
    let root_plots = root_plot_ids(&tree);
    let root_know = root_knowledge_ids(&tree);

    let root = if include_root {
        Some(json!({
            "id": tree.nodes.iter().find(|n| matches!(n.kind, NodeKind::Novel)).map(|n| n.id.clone()),
            "title": novel.title,
            "synopsis": novel.synopsis,
            "features_text": crate::novel_features::format_novel_features_block(&novel.features),
            "word_count_min": novel.word_count_min,
            "word_count_max": novel.word_count_max,
            "linked_characters": json_write_characters(&tree, &take_unseen(&root_chars, &mut seen_chars)),
            "linked_plots": json_write_plots(&tree, &take_unseen(&root_plots, &mut seen_plots), "root"),
            "linked_knowledge": json_write_knowledge(&tree, &take_unseen(&root_know, &mut seen_know), "root"),
        }))
    } else {
        for id in &root_chars {
            seen_chars.insert(id.clone());
        }
        for id in &root_plots {
            seen_plots.insert(id.clone());
        }
        for id in &root_know {
            seen_know.insert(id.clone());
        }
        None
    };

    let vol_id = chapter_parent_volume_id(&tree, &node_id);
    let volume = if let Some(vid) = vol_id.as_ref() {
        let v_chars = volume_character_ids(&tree, vid);
        let v_plots = volume_local_plot_ids(&tree, vid);
        let v_know = volume_local_knowledge_ids(&tree, vid);
        if include_volume {
            tree.nodes.iter().find(|n| n.id == *vid).map(|v| {
                json!({
                    "id": v.id,
                    "label": v.label,
                    "formatted": v.volume.as_ref().map(crate::volume_fmt::format_volume_full).unwrap_or_default(),
                    "linked_characters": json_write_characters(&tree, &take_unseen(&v_chars, &mut seen_chars)),
                    "linked_plots": json_write_plots(&tree, &take_unseen(&v_plots, &mut seen_plots), "volume"),
                    "linked_knowledge": json_write_knowledge(&tree, &take_unseen(&v_know, &mut seen_know), "volume"),
                })
            })
        } else {
            for id in v_chars {
                seen_chars.insert(id);
            }
            for id in v_plots {
                seen_plots.insert(id);
            }
            for id in v_know {
                seen_know.insert(id);
            }
            None
        }
    } else {
        None
    };

    let ch_chars = tree_links::union_linked_ids(
        &tree,
        &node_id,
        &node.linked_character_ids,
        NodeKind::Character,
    );
    let ch_plots = chapter_local_plot_ids(&tree, &node_id);
    let ch_know = chapter_local_knowledge_ids(&tree, &node_id);
    let chapter = json!({
        "id": node.id,
        "label": node.label,
        "outline": node.outline,
        "detailed_outline": node.detailed_outline,
        "word_count_min": novel.word_count_min,
        "word_count_max": novel.word_count_max,
        "linked_characters": json_write_characters(&tree, &take_unseen(&ch_chars, &mut seen_chars)),
        "linked_plots": json_write_plots(&tree, &take_unseen(&ch_plots, &mut seen_plots), "chapter"),
        "linked_knowledge": json_write_knowledge(&tree, &take_unseen(&ch_know, &mut seen_know), "chapter"),
    });

    Ok(serde_json::to_string(&json!({
        "included": {
            "root": root.is_some(),
            "volume": volume.is_some(),
            "chapter": true,
        },
        "root": root,
        "volume": volume,
        "chapter": chapter,
        "ai_guidance": if include_root {
            chapter_write_ai_guidance()
        } else {
            json!({ "note": "沿用本会话已提交的根/卷约束；只使用本次返回的 chapter（及 volume，若 included.volume）。" })
        },
    }))
    .unwrap_or_default())
}

fn update_outline(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    let n = tree
        .nodes
        .iter_mut()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if let Some(t) = args.get("title").and_then(|v| v.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            n.label = t.to_string();
        }
    }
    if let Some(o) = args.get("outline").and_then(|v| v.as_str()) {
        if matches!(n.kind, NodeKind::Volume) {
            return Err("分卷请用 upsert_volume 写入 volume 结构化字段，勿写 outline".into());
        }
        n.outline = o.to_string();
    }
    if let Some(arr) = args.get("detailed_outline").and_then(|v| v.as_array()) {
        if !matches!(n.kind, NodeKind::Chapter) {
            return Err("detailed_outline 仅适用于章节卡".into());
        }
        n.detailed_outline = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
    }
    let kind = match n.kind {
        NodeKind::Chapter => "chapter",
        NodeKind::SidePlot => "side_plot",
        _ => "node",
    };
    let ack = json!({
        "outline": n.outline,
        "detailed_outline": n.detailed_outline,
    });
    let id = n.id.clone();
    let label = n.label.clone();
    save_tree_notify(ctx, tree, false)?;
    Ok(upsert_card_ack(kind, &id, &label, false, ack))
}

async fn mcp_generate_detailed_outline(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let user_notes = args
        .get("user_notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = args
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let app = ctx.app.lock().unwrap().clone();
    let items = generate_detailed_outline_inner(
        app.as_ref(),
        &ctx.db,
        &novel_id,
        &node_id,
        &user_notes,
        model.as_deref(),
        None,
        true,
    )
    .await?;
    emit_tree_changed(ctx, &novel_id);
    Ok(serde_json::to_string_pretty(&json!({
        "node_id": node_id,
        "detailed_outline": items,
        "ai_guidance": "细纲已写入。条数已按本章字数目标控制；写正文按条均分篇幅、瞄准中位，禁止把一条扩成半章。场面须符合世界观/故事规则/功能选项。手改用 update_chapter_outline；单条重写用 regenerate_detailed_outline_item。材料已由本工具组装，不要再 get_tree。",
        "ok": true,
        "label": "细纲",
    }))
    .unwrap_or_default())
}

async fn mcp_regenerate_detailed_outline_item(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let index = args
        .get("index")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "缺少 index（细纲下标，从 0 起）".to_string())? as usize;
    let user_notes = args
        .get("user_notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = args
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let app = ctx.app.lock().unwrap().clone();
    let item = regenerate_detailed_outline_item_inner(
        app.as_ref(),
        &ctx.db,
        &novel_id,
        &node_id,
        index,
        &user_notes,
        model.as_deref(),
        None,
    )
    .await?;
    emit_tree_changed(ctx, &novel_id);
    let tree = get_tree(novel_id)?;
    let list = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.detailed_outline.clone())
        .unwrap_or_default();
    Ok(serde_json::to_string_pretty(&json!({
        "node_id": node_id,
        "index": index,
        "item": item,
        "detailed_outline": list,
        "ai_guidance": "已重写 detailed_outline[index]。场面须符合世界观/故事规则。可用 update_chapter_outline 调整顺序或全文；写正文按整份细纲扩充。",
        "ok": true,
        "label": "细纲",
    }))
    .unwrap_or_default())
}

fn upsert_character(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    let node_id = optional_resolved_node_id(&tree, &args)?;
    let name_arg = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let created = node_id.is_none();
    let id = if let Some(id) = node_id {
        let n = tree
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| "节点不存在".to_string())?;
        if !matches!(n.kind, NodeKind::Character) {
            return Err("不是人物卡".into());
        }
        let name = name_arg
            .map(|s| s.to_string())
            .unwrap_or_else(|| n.label.clone());
        if name.trim().is_empty() {
            return Err("人物真名不能为空".into());
        }
        let mut card = n.character.clone().unwrap_or_default();
        patch_character_card_from_args(&mut card, &args, &name)?;
        n.label = name;
        n.character = Some(card);
        id
    } else {
        let name = name_arg
            .map(|s| s.to_string())
            .ok_or_else(|| "新建人物卡需要 name".to_string())?;
        let mut card = CharacterCard::default();
        patch_character_card_from_args(&mut card, &args, &name)?;
        let id = format!("char-{}", Uuid::new_v4());
        let chars = tree
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Character))
            .count();
        tree.nodes.push(TreeNode {
            id: id.clone(),
            kind: NodeKind::Character,
            label: name,
            outline: String::new(),
            detailed_outline: vec![],
            character: Some(card),
            knowledge: None,
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition {
                x: 40.0,
                y: 80.0 + chars as f64 * 140.0,
            },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        });
        id
    };
    maybe_link_new_card(
        ctx,
        &mut tree,
        created,
        &id,
        "character",
        args.get("link_to").and_then(|v| v.as_str()),
    )?;
    save_tree_notify(ctx, tree.clone(), created)?;
    let label = tree
        .nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.label.clone())
        .unwrap_or_default();
    Ok(upsert_card_ack("character", &id, &label, created, json!({})))
}

fn upsert_plot(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let title = arg_str(&args, "title")?;
    let mut tree = get_tree(novel_id)?;
    let outline = args
        .get("outline")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("active")
        .to_string();
    let node_id = optional_resolved_node_id(&tree, &args)?;
    let created = node_id.is_none();
    let id = if let Some(id) = node_id {
        let n = tree
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| "节点不存在".to_string())?;
        if !matches!(n.kind, NodeKind::SidePlot) {
            return Err("不是剧情卡".into());
        }
        n.label = title;
        n.outline = outline;
        n.side_plot = Some(SidePlotMeta {
            status,
            absorbed: n.side_plot.as_ref().map(|s| s.absorbed).unwrap_or(false),
        });
        id
    } else {
        let id = format!("plot-{}", Uuid::new_v4());
        let plots = tree
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::SidePlot))
            .count();
        tree.nodes.push(TreeNode {
            id: id.clone(),
            kind: NodeKind::SidePlot,
            label: title,
            outline,
            detailed_outline: vec![],
            character: None,
            knowledge: None,
            side_plot: Some(SidePlotMeta {
                status,
                absorbed: false,
            }),
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition {
                x: 520.0,
                y: 80.0 + plots as f64 * 140.0,
            },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        });
        id
    };
    maybe_link_new_card(
        ctx,
        &mut tree,
        created,
        &id,
        "side_plot",
        args.get("link_to").and_then(|v| v.as_str()),
    )?;
    save_tree_notify(ctx, tree.clone(), created)?;
    let label = tree
        .nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.label.clone())
        .unwrap_or_default();
    Ok(upsert_card_ack("side_plot", &id, &label, created, json!({})))
}

fn arg_str_vec(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key)?.as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect()
    })
}

fn patch_knowledge_payload(p: &mut KnowledgeCardPayload, args: &Value) {
    if let Some(ids) = arg_str_vec(args, "book_ids") {
        p.book_ids = ids;
    }
    if let Some(v) = args.get("extract_prompt").and_then(|v| v.as_str()) {
        p.extract_prompt = v.to_string();
    }
    if let Some(v) = args.get("extracted").and_then(|v| v.as_str()) {
        p.extracted = v.to_string();
    }
    if let Some(s) = args.get("slot").and_then(|v| v.as_str()) {
        p.slot = s.trim().to_string();
    }
    if let Some(v) = args.get("core_laws") {
        if let Ok(x) = serde_json::from_value::<CoreLawsPayload>(v.clone()) {
            p.core_laws = Some(x);
        }
    }
    if let Some(v) = args.get("spatiotemporal") {
        if let Ok(x) = serde_json::from_value::<SpatiotemporalPayload>(v.clone()) {
            p.spatiotemporal = Some(x);
        }
    }
    if let Some(v) = args.get("social_power") {
        if let Ok(x) = serde_json::from_value::<SocialPowerPayload>(v.clone()) {
            p.social_power = Some(x);
        }
    }
    if let Some(v) = args.get("existence") {
        if let Ok(x) = serde_json::from_value::<ExistencePayload>(v.clone()) {
            p.existence = Some(x);
        }
    }
    if let Some(v) = args.get("info_flow") {
        if let Ok(x) = serde_json::from_value::<InfoFlowPayload>(v.clone()) {
            p.info_flow = Some(x);
        }
    }
    if let Some(v) = args.get("history_culture") {
        if let Ok(x) = serde_json::from_value::<HistoryCulturePayload>(v.clone()) {
            p.history_culture = Some(x);
        }
    }
    if let Some(v) = args.get("surface_setting") {
        if let Ok(x) = serde_json::from_value::<SurfaceSettingPayload>(v.clone()) {
            p.surface_setting = Some(x);
        }
    }
    if let Some(v) = args.get("story_engine") {
        if let Ok(x) = serde_json::from_value::<StoryEnginePayload>(v.clone()) {
            p.story_engine = Some(x);
        }
    }
    if let Some(v) = args.get("fulfillment_system") {
        if let Ok(x) = serde_json::from_value::<FulfillmentSystemPayload>(v.clone()) {
            p.fulfillment_system = Some(x);
        }
    }
    if let Some(v) = args.get("constraint_redlines") {
        if let Ok(x) = serde_json::from_value::<ConstraintRedlinesPayload>(v.clone()) {
            p.constraint_redlines = Some(x);
        }
    }
}

fn find_knowledge_id_by_slot(tree: &NovelTree, slot: &str) -> Option<String> {
    tree.nodes
        .iter()
        .find(|n| {
            matches!(n.kind, NodeKind::Knowledge)
                && n.knowledge
                    .as_ref()
                    .map(|k| k.slot.trim() == slot)
                    .unwrap_or(false)
        })
        .map(|n| n.id.clone())
}

fn sync_knowledge_node_extracted(tree: &mut NovelTree, node_id: &str) {
    let slot = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .and_then(|n| n.knowledge.as_ref())
        .map(|k| k.slot.trim().to_string())
        .unwrap_or_default();
    let body = {
        let n = tree.nodes.iter().find(|x| x.id == node_id).expect("node");
        crate::core_laws_fmt::knowledge_card_inject_body(tree, n)
    };
    if let Some(n) = tree.nodes.iter_mut().find(|x| x.id == node_id) {
        if let Some(k) = n.knowledge.as_mut() {
            k.extracted = body.clone();
        }
        n.outline = body.chars().take(200).collect();
    }
    if slot == "story_rules"
        || slot.starts_with("sr_")
    {
        crate::story_rules_ops::sync_story_rules_parent_extracted(tree);
    }
}

/// 新建人物/剧情/知识卡宿主：显式 `link_to` 优先；否则当前选中（卡则取其宿主）；若落到根且已有章节则改挂末章。
fn card_create_host(
    tree: &NovelTree,
    created: bool,
    link_to: Option<&str>,
    selected_id: Option<&str>,
) -> Result<Option<String>, String> {
    let explicit = link_to.map(str::trim).filter(|s| !s.is_empty());
    if let Some(s) = explicit {
        return Ok(Some(resolve_link_host(tree, s)?));
    }
    if !created {
        return Ok(None);
    }
    let host = knowledge_attach_host(tree, selected_id)?;
    Ok(Some(prefer_chapter_host(tree, host)))
}

fn prefer_chapter_host(tree: &NovelTree, host: String) -> String {
    let on_root = tree
        .nodes
        .iter()
        .any(|n| n.id == host && matches!(n.kind, NodeKind::Novel));
    if !on_root {
        return host;
    }
    last_chapter_anchor(tree).0
}

fn maybe_link_new_card(
    ctx: &McpCtx,
    tree: &mut NovelTree,
    created: bool,
    card_id: &str,
    kind: &str,
    link_to: Option<&str>,
) -> Result<(), String> {
    let sel = workspace_sel(ctx).map(|(_, id)| id);
    if let Some(host) = card_create_host(tree, created, link_to, sel.as_deref())? {
        if host != card_id {
            link_pair(tree, &host, card_id, kind)?;
        }
    }
    Ok(())
}

fn upsert_knowledge_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let title_arg = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let slot_arg = args
        .get("slot")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut tree = get_tree(novel_id)?;
    let mut node_id = optional_resolved_node_id(&tree, &args)?;
    let mut created = node_id.is_none();

    if let Some(slot) = slot_arg {
        if created && is_fixed_root_knowledge_slot(slot) {
            crate::worldview_ops::ensure_worldview_cards(&mut tree);
            if slot == "story_rules" || slot.starts_with("sr_") {
                crate::story_rules_ops::ensure_story_rules_fan_cards(&mut tree);
            }
            node_id = find_knowledge_id_by_slot(&tree, slot);
            created = false;
        } else if node_id.is_none() {
            if let Some(id) = find_knowledge_id_by_slot(&tree, slot) {
                node_id = Some(id);
                created = false;
            }
        }
    }

    let fixed_slot = slot_arg
        .filter(|s| is_fixed_root_knowledge_slot(s))
        .or_else(|| {
            node_id.as_ref().and_then(|nid| {
                tree.nodes
                    .iter()
                    .find(|n| n.id == *nid)
                    .and_then(|n| n.knowledge.as_ref())
                    .map(|k| k.slot.trim())
                    .filter(|s| is_fixed_root_knowledge_slot(s))
            })
        });
    let skip_chapter_link = fixed_slot.is_some();

    let id = if let Some(id) = node_id {
        let title = if title_arg.is_empty() {
            tree.nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.label.clone())
                .unwrap_or_default()
        } else {
            title_arg.to_string()
        };
        let mut payload = tree
            .nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.knowledge.clone())
            .unwrap_or_default();
        patch_knowledge_payload(&mut payload, &args);
        let outline_preview = payload.extracted.chars().take(200).collect::<String>();
        if let Some(n) = tree.nodes.iter_mut().find(|n| n.id == id) {
            if !matches!(n.kind, NodeKind::Knowledge) {
                return Err("不是知识卡".into());
            }
            n.label = title;
            n.knowledge = Some(payload);
            n.outline = outline_preview;
        } else {
            return Err("节点不存在".to_string());
        }
        sync_knowledge_node_extracted(&mut tree, &id);
        id
    } else {
        if title_arg.is_empty() {
            return Err("新建知识卡需要 title".into());
        }
        let mut payload = KnowledgeCardPayload::default();
        patch_knowledge_payload(&mut payload, &args);
        let id = format!("kb-{}", Uuid::new_v4());
        let kbs = tree
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Knowledge))
            .count();
        tree.nodes.push(TreeNode {
            id: id.clone(),
            kind: NodeKind::Knowledge,
            label: title_arg.to_string(),
            outline: payload.extracted.chars().take(200).collect(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(payload),
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition {
                x: 40.0,
                y: 80.0 + kbs as f64 * 140.0,
            },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        });
        sync_knowledge_node_extracted(&mut tree, &id);
        id
    };

    if !skip_chapter_link {
        maybe_link_new_card(
            ctx,
            &mut tree,
            created,
            &id,
            "knowledge",
            args.get("link_to").and_then(|v| v.as_str()),
        )?;
    }
    save_tree_notify(ctx, tree.clone(), created)?;
    let n = tree
        .nodes
        .iter()
        .find(|n| n.id == id)
        .ok_or_else(|| "节点不存在".to_string())?;
    let slot = n
        .knowledge
        .as_ref()
        .map(|k| k.slot.clone())
        .unwrap_or_default();
    Ok(upsert_card_ack(
        "knowledge",
        &id,
        &n.label,
        created,
        json!({ "slot": slot }),
    ))
}

fn upsert_public_knowledge_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let mut title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mut book_ids = arg_str_vec(&args, "book_ids").unwrap_or_default();
    let mut extract_prompt = args
        .get("extract_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut extracted = args
        .get("extracted")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if title.is_empty() || extracted.is_empty() {
        if let Ok(novel_id) = arg_novel_id(ctx, &args) {
            if let Ok(node_id) = arg_node_id(ctx, &args) {
                let tree = get_tree(novel_id)?;
                if let Some(n) = tree
                    .nodes
                    .iter()
                    .find(|n| n.id == node_id && matches!(n.kind, NodeKind::Knowledge))
                {
                    if title.is_empty() {
                        title = n.label.clone();
                    }
                    if let Some(p) = &n.knowledge {
                        if book_ids.is_empty() {
                            book_ids = p.book_ids.clone();
                        }
                        if extract_prompt.trim().is_empty() {
                            extract_prompt = p.extract_prompt.clone();
                        }
                        if extracted.is_empty() {
                            extracted = p.extracted.clone();
                        }
                    }
                }
            }
        }
    }
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let card = upsert_public_knowledge_card_inner(
        &ctx.db,
        id,
        title,
        book_ids,
        extract_prompt,
        extracted,
    )?;
    Ok(serde_json::to_string_pretty(&card).unwrap_or_default())
}

fn add_public_knowledge_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let public_id = arg_str(&args, "public_id")?;
    let host = args
        .get("link_to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| workspace_sel(ctx).map(|(_, id)| id));
    let (tree, kid) = add_public_knowledge_card_inner(
        &ctx.db,
        &novel_id,
        &public_id,
        host.as_deref(),
    )?;
    emit_tree_changed(ctx, &tree.novel_id);
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == kid)
        .cloned()
        .ok_or_else(|| "添加失败".to_string())?;
    Ok(serde_json::to_string_pretty(&node).unwrap_or_default())
}

fn fill_knowledge_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let node_id = arg_node_id(ctx, &args)?;
    let book_ids = arg_str_vec(&args, "book_ids");
    let max_chars = args
        .get("max_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(14_000) as usize;
    let tree = fill_knowledge_card_inner(
        &ctx.db,
        &novel_id,
        &node_id,
        book_ids,
        max_chars,
    )?;
    emit_tree_changed(ctx, &tree.novel_id);
    let n = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    let extracted_chars = n
        .knowledge
        .as_ref()
        .map(|k| k.extracted.chars().count())
        .unwrap_or(0);
    Ok(upsert_card_ack(
        "knowledge",
        &node_id,
        &n.label,
        false,
        json!({ "extracted_chars": extracted_chars }),
    ))
}

fn link_nodes(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let source = arg_str(&args, "source_id")?;
    let target = arg_str(&args, "target_id")?;
    let kind = args
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut tree = get_tree(novel_id)?;
    let source = resolve_link_host(&tree, &source)?;
    let target = resolve_link_host(&tree, &target)?;
    let kind = if kind.is_empty() {
        infer_link_kind(&tree, &source, &target)?
    } else {
        kind
    };
    // host is chapter/root; card is the other
    let (host, card) = pick_host_card(&tree, &source, &target, &kind)?;
    link_pair(&mut tree, &host, &card, &kind)?;
    save_tree_notify(ctx, tree, false)?;
    Ok(json!({"ok": true, "host": host, "card": card, "kind": kind}).to_string())
}

fn is_fixed_root_knowledge_slot(slot: &str) -> bool {
    matches!(
        slot,
        "wv_core_laws"
            | "wv_spatiotemporal"
            | "wv_social_power"
            | "wv_history_culture"
            | "wv_existence"
            | "wv_info_flow"
            | "story_rules"
            | "sr_surface_setting"
            | "sr_story_engine"
            | "sr_fulfillment_system"
            | "sr_constraint_redlines"
            | "write_prompts"
    )
}

fn node_fixed_worldview(tree: &NovelTree, id: &str) -> bool {
    tree.nodes.iter().any(|n| {
        n.id == id
            && matches!(n.kind, NodeKind::Knowledge)
            && n.knowledge
                .as_ref()
                .map(|k| {
                    let slot = k.slot.trim();
                    is_fixed_root_knowledge_slot(slot)
                        && !crate::write_prompts::is_write_prompts_slot(slot)
                })
                .unwrap_or(false)
    })
}

fn unlink_locked_edge(tree: &NovelTree, source: &str, target: &str) -> bool {
    crate::write_prompts::is_write_prompts_root_edge(tree, source, target)
        || node_fixed_worldview(tree, source)
        || node_fixed_worldview(tree, target)
}

fn unlink_nodes(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    if let Some(eid) = args.get("edge_id").and_then(|v| v.as_str()) {
        if let Some(e) = tree.edges.iter().find(|e| e.id == eid) {
            if unlink_locked_edge(&tree, &e.source, &e.target) {
                return Err("固定连线不可断开".into());
            }
        }
        tree.edges.retain(|e| e.id != eid);
    } else {
        let a = arg_str(&args, "source_id")?;
        let b = arg_str(&args, "target_id")?;
        if unlink_locked_edge(&tree, &a, &b) {
            return Err("固定连线不可断开".into());
        }
        tree.edges.retain(|e| {
            !((e.source == a && e.target == b) || (e.source == b && e.target == a))
        });
        for n in &mut tree.nodes {
            if n.id == a {
                n.linked_character_ids.retain(|id| id != &b);
                n.linked_side_plot_ids.retain(|id| id != &b);
                n.linked_knowledge_ids.retain(|id| id != &b);
            }
            if n.id == b {
                n.linked_character_ids.retain(|id| id != &a);
                n.linked_side_plot_ids.retain(|id| id != &a);
                n.linked_knowledge_ids.retain(|id| id != &a);
            }
        }
    }
    save_tree_notify(ctx, tree, false)?;
    Ok(json!({"ok": true}).to_string())
}

fn infer_link_kind(tree: &crate::models::NovelTree, a: &str, b: &str) -> Result<String, String> {
    let ka = tree
        .nodes
        .iter()
        .find(|n| n.id == a)
        .map(|n| &n.kind)
        .ok_or_else(|| "source 不存在".to_string())?;
    let kb = tree
        .nodes
        .iter()
        .find(|n| n.id == b)
        .map(|n| &n.kind)
        .ok_or_else(|| "target 不存在".to_string())?;
    for k in [ka, kb] {
        match k {
            NodeKind::Character => return Ok("character".into()),
            NodeKind::SidePlot => return Ok("side_plot".into()),
            NodeKind::Knowledge => return Ok("knowledge".into()),
            _ => {}
        }
    }
    Ok("chapter".into())
}

fn pick_host_card(
    tree: &crate::models::NovelTree,
    a: &str,
    b: &str,
    kind: &str,
) -> Result<(String, String), String> {
    let na = tree
        .nodes
        .iter()
        .find(|n| n.id == a)
        .ok_or_else(|| "source 不存在".to_string())?;
    let nb = tree
        .nodes
        .iter()
        .find(|n| n.id == b)
        .ok_or_else(|| "target 不存在".to_string())?;
    let card_kind = match kind {
        "character" => NodeKind::Character,
        "side_plot" => NodeKind::SidePlot,
        "knowledge" => NodeKind::Knowledge,
        _ => {
            // 章节链：靠上（y 更小）→ 靠下，root.bottom → 首章.top
            if na.position.y <= nb.position.y {
                return Ok((a.to_string(), b.to_string()));
            }
            return Ok((b.to_string(), a.to_string()));
        }
    };
    if na.kind == card_kind && crate::write_prompts::is_write_prompts_slot(crate::write_prompts::knowledge_slot(na)) {
        return Ok((a.to_string(), b.to_string()));
    }
    if nb.kind == card_kind && crate::write_prompts::is_write_prompts_slot(crate::write_prompts::knowledge_slot(nb)) {
        return Ok((b.to_string(), a.to_string()));
    }
    if na.kind == card_kind {
        Ok((b.to_string(), a.to_string()))
    } else if nb.kind == card_kind {
        Ok((a.to_string(), b.to_string()))
    } else {
        Ok((a.to_string(), b.to_string()))
    }
}

fn link_handles<'a>(
    tree: &'a crate::models::NovelTree,
    host_id: &str,
    card_id: &str,
    kind: &str,
) -> (&'static str, &'static str) {
    match kind {
        "knowledge" => {
            let host_is_novel = tree
                .nodes
                .iter()
                .any(|n| n.id == host_id && matches!(n.kind, NodeKind::Novel));
            if host_is_novel {
                if let Some(card) = tree.nodes.iter().find(|n| n.id == card_id) {
                    let slot = crate::write_prompts::knowledge_slot(card);
                    if crate::write_prompts::is_write_prompts_slot(slot) {
                        return ("wp", "right");
                    }
                    if slot == "story_rules" {
                        return ("sr", "left");
                    }
                    if crate::worldview_ops::worldview_json_key_for_slot(slot).is_some() {
                        return ("wv", "bottom");
                    }
                }
            }
            ("left", "right")
        }
        "character" => ("left", "right"),
        "chapter" | "volume" => ("bottom", "top"),
        _ => ("right", "left"),
    }
}

fn link_pair(
    tree: &mut crate::models::NovelTree,
    host_id: &str,
    card_id: &str,
    kind: &str,
) -> Result<(), String> {
    let host_id = resolve_link_host(tree, host_id)?;
    if host_id == card_id {
        return Err("不能关联自身".into());
    }
    if !tree.nodes.iter().any(|n| n.id == card_id) {
        return Err("卡片不存在".into());
    }
    if !tree.nodes.iter().any(|n| n.id == host_id) {
        return Err("宿主节点不存在".into());
    }
    let exists = tree
        .edges
        .iter()
        .any(|e| e.source == host_id && e.target == card_id || e.source == card_id && e.target == host_id);
    let (sh, th) = link_handles(tree, &host_id, card_id, kind);
    if !exists {
        tree.edges.push(TreeEdge {
            id: format!("e-{host_id}-{card_id}"),
            source: host_id.clone(),
            target: card_id.to_string(),
            kind: kind.to_string(),
            source_handle: Some(sh.into()),
            target_handle: Some(th.into()),
            label: String::new(),
        });
    }
    if let Some(host) = tree.nodes.iter_mut().find(|n| n.id == host_id) {
        match kind {
            "character" => {
                if !host.linked_character_ids.iter().any(|x| x == card_id) {
                    host.linked_character_ids.push(card_id.to_string());
                }
            }
            "side_plot" => {
                if !host.linked_side_plot_ids.iter().any(|x| x == card_id) {
                    host.linked_side_plot_ids.push(card_id.to_string());
                }
            }
            "knowledge" => {
                if !host.linked_knowledge_ids.iter().any(|x| x == card_id) {
                    host.linked_knowledge_ids.push(card_id.to_string());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: NodeKind) -> TreeNode {
        TreeNode {
            id: id.into(),
            kind,
            label: id.into(),
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
        }
    }

    #[test]
    fn union_linked_ids_listed_then_edges() {
        let mut root = node("root", NodeKind::Novel);
        root.linked_character_ids = vec!["char-listed".into()];
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                root,
                node("char-listed", NodeKind::Character),
                node("char-edge", NodeKind::Character),
                node("plot-edge", NodeKind::SidePlot),
            ],
            edges: vec![
                TreeEdge {
                    id: "e1".into(),
                    source: "root".into(),
                    target: "char-edge".into(),
                    kind: "character".into(),
                    source_handle: None,
                    target_handle: None,
                    label: String::new(),
                },
                TreeEdge {
                    id: "e2".into(),
                    source: "plot-edge".into(),
                    target: "root".into(),
                    kind: "side_plot".into(),
                    source_handle: None,
                    target_handle: None,
                    label: String::new(),
                },
            ],
        };
        assert_eq!(
            tree_links::union_linked_ids(
                &tree,
                "root",
                &tree.nodes[0].linked_character_ids,
                NodeKind::Character
            ),
            vec!["char-listed".to_string(), "char-edge".to_string()]
        );
        assert_eq!(
            tree_links::union_linked_ids(&tree, "root", &[], NodeKind::SidePlot),
            vec!["plot-edge".to_string()]
        );
        assert!(tree_links::union_linked_ids(&tree, "root", &[], NodeKind::Knowledge).is_empty());
    }

    #[test]
    fn clear_selection_if_only_matching_novel() {
        let rt = McpRuntime::new(17832);
        rt.set_workspace_selection("novel-a", "node-1");
        rt.clear_workspace_selection_if("novel-b");
        assert_eq!(
            rt.workspace_selection(),
            Some(("novel-a".into(), "node-1".into()))
        );
        rt.clear_workspace_selection_if("novel-a");
        assert!(rt.workspace_selection().is_none());
    }

    #[test]
    fn arg_or_prefers_arg_then_selection() {
        let empty = json!({});
        assert!(arg_or(&empty, "novel_id", None).is_err());
        assert_eq!(
            arg_or(&empty, "novel_id", Some("sel-n".into())).unwrap(),
            "sel-n"
        );
        let with = json!({ "novel_id": "arg-n" });
        assert_eq!(
            arg_or(&with, "novel_id", Some("sel-n".into())).unwrap(),
            "arg-n"
        );
    }

    #[test]
    fn json_node_spec_accepts_string_or_number() {
        assert_eq!(
            json_node_spec(Some(&json!("第3章"))),
            Some("第3章".into())
        );
        assert_eq!(json_node_spec(Some(&json!(3))), Some("3".into()));
        assert_eq!(json_node_spec(Some(&json!(""))), None);
        assert_eq!(json_node_spec(None), None);
    }

    #[test]
    fn patch_knowledge_keeps_omitted_fields() {
        let mut p = KnowledgeCardPayload {
            book_ids: vec!["a".into()],
            extract_prompt: "节奏".into(),
            extracted: "旧摘要".into(),
            from_canon: true,
            slot: String::new(),
            core_laws: None,
            spatiotemporal: None,
            world_axiom: None,
            key_location: None,
            social_power: None,
            world_race: None,
            major_faction: None,
            existence: None,
            info_flow: None,
            history_culture: None,
            world_religion: None,
            major_event: None,
            surface_setting: None,
            story_engine: None,
            fulfillment_system: None,
            constraint_redlines: None,
        };
        patch_knowledge_payload(&mut p, &json!({ "extracted": "新要点" }));
        assert_eq!(p.book_ids, ["a"]);
        assert_eq!(p.extract_prompt, "节奏");
        assert_eq!(p.extracted, "新要点");
        assert!(p.from_canon);
        patch_knowledge_payload(&mut p, &json!({ "book_ids": ["b", "c"] }));
        assert_eq!(p.book_ids, ["b", "c"]);
        assert_eq!(p.extracted, "新要点");
    }

    #[test]
    #[test]
    fn mcp_shot_briefs_tell_agent_to_write_without_app_llm() {
        for g in [shot_split_ai_guidance(), shot_comfy_prompt_ai_guidance()] {
            assert!(g.contains("不调用应用内置 LLM"), "{g}");
            assert!(g.contains("set_chapter_shots"), "{g}");
        }
        assert!(
            shot_split_ai_guidance().contains("禁止把整章收成 1 条"),
            "{}",
            shot_split_ai_guidance()
        );
    }

    #[test]
    fn chapter_write_ai_guidance_has_coherence_fields() {
        let g = chapter_write_ai_guidance();
        for k in [
            "plots",
            "characters",
            "knowledge",
            "lore",
            "narrative_coherence",
            "forbidden",
            "length",
            "output",
            "content_usage",
        ] {
            assert!(
                g.get(k)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty()),
                "missing {k}"
            );
        }
    }

    #[test]
    fn merge_batch_item_inherits_novel_and_link() {
        let parent = json!({"novel_id": "n", "link_to": "root", "title": "ignore"});
        let item = json!({"name": "李四"});
        let m = merge_batch_item(&parent, &item);
        assert_eq!(m["novel_id"], "n");
        assert_eq!(m["link_to"], "root");
        assert_eq!(m["name"], "李四");
        assert!(m.get("title").is_none());
    }

    #[test]
    fn merge_batch_item_keeps_item_link_to() {
        let m = merge_batch_item(&json!({"link_to": "root"}), &json!({"link_to": "ch-1"}));
        assert_eq!(m["link_to"], "ch-1");
    }

    #[test]
    fn run_maybe_batch_passthrough_without_items() {
        let out = run_maybe_batch(json!({"name": "x"}), |a| {
            Ok(format!("one:{}", a["name"].as_str().unwrap()))
        })
        .unwrap();
        assert_eq!(out, "one:x");
    }

    #[test]
    fn run_maybe_batch_empty_and_over_max() {
        assert!(run_maybe_batch(json!({"items": []}), |_| Ok("{}".into())).is_err());
        let items: Vec<Value> = (0..BATCH_MAX + 1).map(|i| json!({"n": i})).collect();
        let err = run_maybe_batch(json!({"items": items}), |_| Ok("{}".into())).unwrap_err();
        assert!(err.contains("50"), "{err}");
    }

    #[test]
    fn run_maybe_batch_partial_success() {
        let args = json!({"novel_id": "n", "items": [{"name": "ok"}, {"name": "bad"}]});
        let out = run_maybe_batch(args, |a| {
            if a["name"] == "bad" {
                Err("nope".into())
            } else {
                Ok(r#"{"ok":true,"label":"ok"}"#.into())
            }
        })
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 2);
        assert_eq!(v["ok"], 1);
        assert_eq!(v["failed"], 1);
        assert_eq!(v["results"][0]["ok"], true);
        assert_eq!(v["results"][1]["error"], "nope");
    }

    #[test]
    fn with_batch_items_adds_items_property() {
        let s = with_batch_items(json!({"type":"object","properties":{"title":{"type":"string"}}}));
        assert!(s["properties"]["items"].is_object());
        assert_eq!(s["properties"]["title"]["type"], "string");
    }

    #[test]
    fn new_card_defaults_link_to_last_chapter() {
        let mut ch = node("ch-1", NodeKind::Chapter);
        ch.position.y = 120.0;
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![node("root", NodeKind::Novel), ch],
            edges: vec![],
        };
        assert_eq!(
            card_create_host(&tree, true, None, None).unwrap().as_deref(),
            Some("ch-1")
        );
        assert_eq!(
            card_create_host(&tree, true, None, Some("root"))
                .unwrap()
                .as_deref(),
            Some("ch-1")
        );
        assert_eq!(
            card_create_host(&tree, true, Some("ch-1"), None)
                .unwrap()
                .as_deref(),
            Some("ch-1")
        );
        assert_eq!(card_create_host(&tree, false, None, None).unwrap(), None);
    }

    #[test]
    fn new_card_without_chapters_hangs_on_root() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![node("root", NodeKind::Novel)],
            edges: vec![],
        };
        assert_eq!(
            card_create_host(&tree, true, None, None).unwrap().as_deref(),
            Some("root")
        );
    }

    #[test]
    fn link_to_root_alias_uses_novel_uuid() {
        let mut tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("nv-1", NodeKind::Novel),
                node("kb-1", NodeKind::Knowledge),
            ],
            edges: vec![],
        };
        assert_eq!(
            card_create_host(&tree, true, Some("root"), None)
                .unwrap()
                .as_deref(),
            Some("nv-1")
        );
        link_pair(&mut tree, "root", "kb-1", "knowledge").unwrap();
        let root = tree.nodes.iter().find(|n| n.id == "nv-1").unwrap();
        assert!(root.linked_knowledge_ids.iter().any(|id| id == "kb-1"));
        let e = tree.edges.iter().find(|e| e.source == "nv-1" && e.target == "kb-1").unwrap();
        assert_eq!(e.source_handle.as_deref(), Some("left"));
        assert_eq!(e.target_handle.as_deref(), Some("right"));
        assert!(link_pair(&mut tree, "root", "missing-kb", "knowledge").is_err());
    }

    #[test]
    fn link_pair_handles_by_kind() {
        let mut ch = node("ch-1", NodeKind::Chapter);
        ch.position.y = 120.0;
        let mut tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel),
                ch,
                node("plot-1", NodeKind::SidePlot),
                node("char-1", NodeKind::Character),
                node("ch-2", NodeKind::Chapter),
            ],
            edges: vec![],
        };
        tree.nodes.last_mut().unwrap().position.y = 260.0;
        link_pair(&mut tree, "ch-1", "plot-1", "side_plot").unwrap();
        link_pair(&mut tree, "ch-1", "char-1", "character").unwrap();
        link_pair(&mut tree, "root", "ch-1", "chapter").unwrap();
        let plot = tree.edges.iter().find(|e| e.target == "plot-1").unwrap();
        assert_eq!(
            (
                plot.source.as_str(),
                plot.source_handle.as_deref(),
                plot.target_handle.as_deref()
            ),
            ("ch-1", Some("right"), Some("left"))
        );
        let ch_edge = tree.edges.iter().find(|e| e.target == "char-1").unwrap();
        assert_eq!(
            (ch_edge.source_handle.as_deref(), ch_edge.target_handle.as_deref()),
            (Some("left"), Some("right"))
        );
        let spine = tree.edges.iter().find(|e| e.kind == "chapter").unwrap();
        assert_eq!(
            (
                spine.source.as_str(),
                spine.target.as_str(),
                spine.source_handle.as_deref(),
                spine.target_handle.as_deref()
            ),
            ("root", "ch-1", Some("bottom"), Some("top"))
        );
        let (host, card) = pick_host_card(&tree, "ch-2", "ch-1", "chapter").unwrap();
        assert_eq!((host.as_str(), card.as_str()), ("ch-1", "ch-2"));
    }

    #[test]
    fn last_chapter_anchor_sorts_by_y() {
        let mut late = node("ch-late", NodeKind::Chapter);
        late.position.y = 400.0;
        let mut early = node("ch-early", NodeKind::Chapter);
        early.position.y = 80.0;
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![node("root", NodeKind::Novel), late, early],
            edges: vec![],
        };
        assert_eq!(last_chapter_anchor(&tree).0, "ch-late");
    }

    #[test]
    fn repair_retargets_literal_root_edges() {
        let mut tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("nv-1", NodeKind::Novel),
                node("kb-1", NodeKind::Knowledge),
            ],
            edges: vec![TreeEdge {
                id: "e-bad".into(),
                source: "root".into(),
                target: "kb-1".into(),
                kind: "knowledge".into(),
                source_handle: None,
                target_handle: None,
                label: String::new(),
            }],
        };
        assert!(crate::commands::repair_root_alias_links(&mut tree));
        assert_eq!(tree.edges[0].source, "nv-1");
        assert!(tree.nodes[0].linked_knowledge_ids.iter().any(|id| id == "kb-1"));
        assert!(!crate::commands::repair_root_alias_links(&mut tree));
    }

    #[test]
    fn character_json_entry_has_formatted_and_relations() {
        use crate::models::CharacterCard;
        let mut ch = node("char-1", NodeKind::Character);
        ch.label = "李四".into();
        ch.character = Some(CharacterCard {
            gender: "男".into(),
            age: "中年".into(),
            aliases: "老李".into(),
            ..Default::default()
        });
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                ch,
                node("char-2", NodeKind::Character),
                node("root", NodeKind::Novel),
            ],
            edges: vec![TreeEdge {
                id: "e-c".into(),
                source: "char-1".into(),
                target: "char-2".into(),
                kind: "character".into(),
                source_handle: None,
                target_handle: None,
                label: "旧识".into(),
            }],
        };
        let entry = character_json_entry(&tree, &tree.nodes[0], Some(0));
        let obj = entry.as_object().expect("object");
        assert!(obj.get("formatted").and_then(|v| v.as_str()).unwrap_or("").contains("李四"));
        assert!(obj.get("character").is_some());
        let rels = obj
            .get("character_relations")
            .and_then(|v| v.as_array())
            .expect("relations");
        assert_eq!(rels.len(), 1);
    }

    #[test]
    fn patch_character_merges_partial_and_unwraps_entry() {
        use crate::models::{CharacterCard, CharacterWorldPosition};
        let mut card = CharacterCard {
            gender: "男".into(),
            age: "中年".into(),
            world_position: CharacterWorldPosition {
                faction: "商会".into(),
                social_role: "账房".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        // 部分更新：不丢 faction
        patch_character_card_from_args(
            &mut card,
            &json!({ "character": { "gender": "女", "world_position": { "social_role": "掌柜" } } }),
            "李四",
        )
        .unwrap();
        assert_eq!(card.gender, "女");
        assert_eq!(card.age, "中年");
        assert_eq!(card.world_position.faction, "商会");
        assert_eq!(card.world_position.social_role, "掌柜");

        // get 返回的条目包装可直接回写
        let entry = json!({
            "id": "char-1",
            "label": "李四",
            "true_name": "李四",
            "formatted": "## 真名\n李四",
            "character": { "aliases": "老李" }
        });
        patch_character_card_from_args(&mut card, &json!({ "character": entry }), "李四").unwrap();
        assert_eq!(card.aliases, "老李");
        assert_eq!(card.gender, "女");

        // 顶层结构化字段
        patch_character_card_from_args(
            &mut card,
            &json!({ "world_position": { "faction": "帮会" } }),
            "李四",
        )
        .unwrap();
        assert_eq!(card.world_position.faction, "帮会");
        assert_eq!(card.world_position.social_role, "掌柜");
    }

    #[test]
    fn mcp_character_drops_personality_and_takes_body_language() {
        use crate::models::CharacterCard;
        let mut card = CharacterCard {
            personality: "旧性格摘要".into(),
            ..Default::default()
        };
        patch_character_card_from_args(
            &mut card,
            &json!({
                "personality": "不要写这个",
                "character": { "personality": "也不要", "body_language": "说话时转笔" }
            }),
            "李四",
        )
        .unwrap();
        assert_ne!(card.personality, "不要写这个");
        assert_ne!(card.personality, "也不要");
        assert_eq!(card.voice.body_language, "说话时转笔");

        patch_character_card_from_args(
            &mut card,
            &json!({ "body_language": "紧张时摸袖口" }),
            "李四",
        )
        .unwrap();
        assert_eq!(card.voice.body_language, "紧张时摸袖口");

        let mut ch = node("char-1", NodeKind::Character);
        ch.label = "李四".into();
        ch.character = Some(CharacterCard {
            personality: "画布摘要".into(),
            voice: crate::models::CharacterVoice {
                body_language: "转笔".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![ch],
            edges: vec![],
        };
        let entry = character_json_entry(&tree, &tree.nodes[0], None);
        let inner = entry.get("character").and_then(|v| v.as_object()).expect("character");
        assert!(!inner.contains_key("personality"));
        assert_eq!(
            inner
                .get("voice")
                .and_then(|v| v.get("body_language"))
                .and_then(|v| v.as_str()),
            Some("转笔")
        );
    }

    #[test]
    fn write_context_dedupes_and_skips_seen_root_ids() {
        let mut root = node("root", NodeKind::Novel);
        root.linked_character_ids = vec!["c-root".into(), "c-both".into()];
        root.linked_knowledge_ids = vec!["k-root".into()];
        root.linked_side_plot_ids = vec!["p-root".into()];
        let mut vol = node("vol", NodeKind::Volume);
        vol.linked_character_ids = vec!["c-vol".into(), "c-both".into()];
        vol.linked_knowledge_ids = vec!["k-vol".into()];
        let mut ch = node("ch", NodeKind::Chapter);
        ch.linked_character_ids = vec!["c-ch".into(), "c-both".into()];
        ch.linked_knowledge_ids = vec!["k-ch".into(), "k-root".into()];
        ch.outline = "简纲".into();
        ch.detailed_outline = vec!["细1".into()];
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                root,
                vol,
                ch,
                node("c-root", NodeKind::Character),
                node("c-both", NodeKind::Character),
                node("c-vol", NodeKind::Character),
                node("c-ch", NodeKind::Character),
                node("k-root", NodeKind::Knowledge),
                node("k-vol", NodeKind::Knowledge),
                node("k-ch", NodeKind::Knowledge),
                node("p-root", NodeKind::SidePlot),
            ],
            edges: vec![TreeEdge {
                id: "e-vc".into(),
                source: "vol".into(),
                target: "ch".into(),
                kind: "chapter".into(),
                source_handle: None,
                target_handle: None,
                label: String::new(),
            }],
        };
        let mut seen_c = HashSet::new();
        let mut seen_k = HashSet::new();
        let root_c = take_unseen(&root_character_ids(&tree), &mut seen_c);
        let vol_c = take_unseen(&volume_character_ids(&tree, "vol"), &mut seen_c);
        let ch_c = take_unseen(
            &tree_links::union_linked_ids(
                &tree,
                "ch",
                &tree.nodes.iter().find(|n| n.id == "ch").unwrap().linked_character_ids,
                NodeKind::Character,
            ),
            &mut seen_c,
        );
        assert_eq!(root_c, vec!["c-root", "c-both"]);
        assert_eq!(vol_c, vec!["c-vol"]);
        assert_eq!(ch_c, vec!["c-ch"]);

        let root_k = take_unseen(&root_knowledge_ids(&tree), &mut seen_k);
        let vol_k = take_unseen(&volume_local_knowledge_ids(&tree, "vol"), &mut seen_k);
        let ch_k = take_unseen(&chapter_local_knowledge_ids(&tree, "ch"), &mut seen_k);
        assert_eq!(root_k, vec!["k-root"]);
        assert_eq!(vol_k, vec!["k-vol"]);
        assert_eq!(ch_k, vec!["k-ch"]);

        let mut skip_root = HashSet::new();
        for id in root_knowledge_ids(&tree) {
            skip_root.insert(id);
        }
        let ch_only = take_unseen(&chapter_local_knowledge_ids(&tree, "ch"), &mut skip_root);
        assert_eq!(ch_only, vec!["k-ch"]);
    }
}
