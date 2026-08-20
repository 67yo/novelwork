//! Local MCP (Model Context Protocol) via `rmcp` Streamable HTTP — tools only.
//! Endpoint: `http://127.0.0.1:{port}/mcp` (or `0.0.0.0` when LAN enabled).

use crate::commands::{
    add_public_knowledge_card_inner, create_novel_with_tree, delete_tree_card_inner,
    fill_knowledge_card_inner, get_chapter, get_tree, ingest_knowledge_source, knowledge_attach_host,
    last_chapter_anchor, resolve_link_host, save_chapter, save_tree,
    upsert_public_knowledge_card_inner,
};
use crate::db::Db;
use crate::kb_context::retrieve_knowledge;
use crate::models::{
    CharacterCard, KnowledgeCardPayload, NodeKind, NodePosition, NovelProject, NovelTree,
    SidePlotMeta, TreeEdge, TreeNode,
};
use crate::paths::novel_meta_path;
use crate::tree_links::{
    self, chapter_effective_knowledge_ids, chapter_local_plot_ids, chapter_parent_volume_id,
    root_plot_ids, volume_plot_ids,
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
            "Create a novel with title, synopsis, optional chapter_count and per-chapter word range.",
            json!({
                "type":"object",
                "properties":{
                    "title":{"type":"string"},
                    "synopsis":{"type":"string"},
                    "chapter_count":{"type":"integer"},
                    "word_count_min":{"type":"integer"},
                    "word_count_max":{"type":"integer"}
                },
                "required":["title"]
            }),
        ),
        tool(
            "update_novel",
            "Update novel title, synopsis, chapter_count, and/or per-chapter word range.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "synopsis":{"type":"string"},
                    "chapter_count":{"type":"integer"},
                    "word_count_min":{"type":"integer"},
                    "word_count_max":{"type":"integer"}
                }
            }),
        ),
        tool(
            "get_novel_info",
            "Novel snapshot: title, synopsis, word/chapter plan, root-linked characters, plot cards, knowledge cards, and bound public knowledge books. Prefer this over get_tree for orientation.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "get_selected_card",
            "The card currently selected in the workspace UI (process memory). Returns the live tree node; chapter/plot cards include Markdown body. Empty if nothing is selected.",
            json!({
                "type":"object",
                "properties":{
                    "include_content":{"type":"boolean","default":true}
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
            "Run one-click canvas layout (same as workspace 一键排版). Writes node positions. Adding cards already layouts once; call this to re-layout after manual moves.",
            json!({
                "type":"object",
                "properties":{"novel_id":{"type":"string","description":"省略则用工作台当前选中"}}
            }),
        ),
        tool(
            "add_volume",
            "Add an optional volume (分卷) under the novel root (root.bottom → volume.top). Volumes can link characters/plots/knowledge; chapters under a volume inherit root ∪ volume.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "outline":{"type":"string","description":"分卷纲要"}
                }
            }),
        ),
        tool(
            "add_chapter",
            "Add a chapter card. Hang on volume if link_to is a volume (or last chapter under that volume); else first chapter on root, later chapters chain by canvas y.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "outline":{"type":"string"},
                    "link_to":{"type":"string","description":"宿主：volume / chapter / root；省略则挂末章或根"}
                }
            }),
        ),
        tool(
            "update_chapter_outline",
            "Update a chapter (or plot) card title and/or outline.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "title":{"type":"string"},
                    "outline":{"type":"string"}
                }
            }),
        ),
        tool(
            "delete_node",
            "Delete a chapter/volume/character/plot/knowledge card (not root). Deleting a volume reparents its chapters to root.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "get_chapter_content",
            "Read chapter/plot Markdown body only. Use for refine/edit; do not call when generating fresh body from outline (use get_chapter_info instead).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "get_chapter_info",
            "Chapter snapshot for writing (no body text): outline, linked cards, ai_guidance. For refine/edit of existing body use get_chapter_content separately.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"省略则用工作台当前选中"}
                }
            }),
        ),
        tool(
            "set_chapter_content",
            "Write chapter Markdown body.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "content":{"type":"string"}
                },
                "required":["content"]
            }),
        ),
        tool(
            "upsert_character_card",
            "Create or update a character card. New cards hang on a chapter (chapter.left ← character.right). Omit link_to: current selection, else last chapter, else root.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"有则更新该人物卡；省略则新建"},
                    "name":{"type":"string"},
                    "role":{"type":"string"},
                    "gender":{"type":"string"},
                    "alignment":{"type":"string"},
                    "personality":{"type":"string"},
                    "style":{"type":"string"},
                    "motto":{"type":"string"},
                    "link_to":{"type":"string","description":"章/根 id，或 root/novel。新建省略则挂当前选中章，否则末章，再否则根"}
                },
                "required":["name"]
            }),
        ),
        tool(
            "upsert_plot_card",
            "Create or update a plot card. New cards hang on a chapter (chapter.right → plot.left). Omit link_to: selection, else last chapter, else root.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"有则更新该剧情卡；省略则新建"},
                    "title":{"type":"string"},
                    "outline":{"type":"string"},
                    "status":{"type":"string"},
                    "link_to":{"type":"string","description":"章/根 id，或 root/novel。新建省略则挂当前选中章，否则末章，再否则根"}
                },
                "required":["title"]
            }),
        ),
        tool(
            "upsert_knowledge_card",
            "Create or update a tree knowledge card. Update is partial: omitted book_ids / extract_prompt / extracted are kept. Full import of linked public books → fill_knowledge_card. AI refine → search_knowledge then set extracted here.",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"有则更新该知识卡；省略则新建"},
                    "title":{"type":"string"},
                    "book_ids":{"type":"array","items":{"type":"string"}},
                    "extract_prompt":{"type":"string"},
                    "extracted":{"type":"string"},
                    "link_to":{"type":"string","description":"章/根 id，或 root/novel。新建省略则挂当前选中章，否则末章，再否则根"}
                },
                "required":["title"]
            }),
        ),
        tool(
            "fill_knowledge_card",
            "Full-import: copy associated public knowledge books into this knowledge card's extracted field (capped). Requires book_ids on the card or in args. For AI-processed write-back use search_knowledge then upsert_knowledge_card(extracted).",
            json!({
                "type":"object",
                "properties":{
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "node_id":{"type":"string","description":"省略则用工作台当前选中"},
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
            "Create or update a shared knowledge card in the catalog. Pass title+extracted, or omit them to copy the current/selected tree knowledge card (node_id). Does not attach to a novel.",
            json!({
                "type":"object",
                "properties":{
                    "id":{"type":"string","description":"有则更新该公共知识卡"},
                    "title":{"type":"string"},
                    "book_ids":{"type":"array","items":{"type":"string"}},
                    "extract_prompt":{"type":"string"},
                    "extracted":{"type":"string"},
                    "novel_id":{"type":"string","description":"从树上知识卡复制时用"},
                    "node_id":{"type":"string","description":"树上知识卡；省略则用工作台当前选中"}
                }
            }),
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
            "Copy a shared catalog knowledge card onto the current novel tree and link it to the selected node (chapter/root, or the host of the selected card). Does not bind public libraries as novel canon.",
            json!({
                "type":"object",
                "properties":{
                    "public_id":{"type":"string","description":"公共知识卡 id"},
                    "novel_id":{"type":"string","description":"省略则用工作台当前选中"},
                    "link_to":{"type":"string","description":"章/根节点 id，或 root/novel；省略则用当前选中"}
                },
                "required":["public_id"]
            }),
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
            "Remove edge(s) between two nodes (or by edge_id).",
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
            let _ = crate::knowledge::delete_chunks(&id).await;
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
            )?;
            Ok(serde_json::to_string_pretty(&novel).unwrap_or_default())
        }
        "update_novel" => update_novel(&ctx, args),
        "get_novel_info" => get_novel_info(&ctx, args),
        "get_selected_card" => get_selected_card(&ctx, args),
        "get_tree" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let tree = get_tree(novel_id)?;
            Ok(serde_json::to_string_pretty(&tree).unwrap_or_default())
        }
        "layout_tree" => layout_tree(&ctx, args),
        "add_volume" => add_volume(&ctx, args),
        "add_chapter" => add_chapter(&ctx, args),
        "update_chapter_outline" => update_outline(&ctx, args),
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
        "set_chapter_content" => {
            let novel_id = arg_novel_id(&ctx, &args)?;
            let node_id = arg_node_id(&ctx, &args)?;
            let content = arg_str(&args, "content")?;
            let words = save_chapter(novel_id.clone(), node_id, content)?;
            emit_tree_changed(&ctx, &novel_id);
            Ok(json!({"ok": true, "word_count": words}).to_string())
        }
        "upsert_character_card" => upsert_character(&ctx, args),
        "upsert_plot_card" => upsert_plot(&ctx, args),
        "upsert_knowledge_card" => upsert_knowledge_card(&ctx, args),
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
        "upsert_public_knowledge_card" => upsert_public_knowledge_card(&ctx, args),
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
        "add_public_knowledge_card" => add_public_knowledge_card(&ctx, args),
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
    arg_or(args, "node_id", workspace_sel(ctx).map(|(_, id)| id))
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
    let outline = args
        .get("outline")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
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
        outline,
        character: None,
        knowledge: None,
        side_plot: None,
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
    save_tree_notify(ctx, tree.clone(), true)?;
    let added = tree.nodes.iter().find(|n| n.id == id).cloned();
    Ok(serde_json::to_string_pretty(&added).unwrap_or_default())
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
        character: None,
        knowledge: None,
        side_plot: None,
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
    save_tree_notify(ctx, tree.clone(), true)?;
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
            Some(json!({
                "order": order,
                "id": c.id,
                "label": c.label,
                "character": c.character,
            }))
        })
        .collect()
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
            Some(json!({
                "order": order,
                "id": k.id,
                "label": k.label,
                "outline": k.outline,
                "extract_prompt": kn.map(|x| x.extract_prompt.clone()).unwrap_or_default(),
                "extracted": kn.map(|x| x.extracted.clone()).unwrap_or_default(),
                "book_ids": kn.map(|x| x.book_ids.clone()).unwrap_or_default(),
            }))
        })
        .collect()
}

fn get_selected_card(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let Some((novel_id, node_id)) = ctx.selection.lock().unwrap().clone() else {
        return Ok(json!({ "selected": false }).to_string());
    };
    let include_content = args
        .get("include_content")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
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
                    "outline": v.outline,
                })
            })
            .collect()
    };
    Ok(serde_json::to_string_pretty(&json!({
        "ai_guidance": {
            "plots": "linked_plots 为根节点关联的跨章剧情卡，按 order（0 最先）理解全书主线；写章时与分卷/章节卡上的剧情一并遵守，不得改写既定要点。分卷可选，见 volumes。",
            "characters": "linked_characters 为根上贯穿人物；规划与写章须符合其人设（role / personality / style 等）。",
            "knowledge": "linked_knowledge 为根上知识卡（全书写作约束）。优先 extracted；若为空则遵守 extract_prompt 与 outline。公共知识库不是小说设定源，只有挂到树上的知识卡才约束写作。"
        },
        "novel": novel_json_for_mcp(&novel),
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

fn chapter_write_ai_guidance() -> Value {
    json!({
        "plots": "linked_plots 的 inherited_from 为 root|volume|chapter：根/分卷继承按各自 order；本章剧情严格按 linked_side_plot_ids 的 order 0→N。不得混序或改写要点。章节继承根 ∪ 所属分卷（若有）。",
        "characters": "linked_characters 为本章必须出场的人物；正文中须全部出现，并符合其人设（role / personality / style / alignment 等）与关系设定。",
        "knowledge": "linked_knowledge 为本章写作硬约束（知识储备），生成正文必须严格遵循。约束范围包括但不限于：写作手法与叙述节奏、文风与语气、用词习惯与禁用词、关键词/专名替换、视角与时态、对话风格、修辞与氛围、信息密度、敏感表达规避、句式偏好等。优先遵守 extracted；若为空则遵守 extract_prompt 与 outline。禁止另起风格或忽略上述约束。",
        "narrative_coherence": "生成本章正文须保证叙事合理、连贯、可读，并与 node.outline、前序章节记忆（若已提供）一致。①时间与因果：事件按可理解的时间顺序展开；后文不得推翻前文已确立的事实；场景/视角切换须有可感知过渡。②人物一致：决策与言行须符合人设与当前处境、认知；禁止无铺垫的性格、立场、关系或能力突变。③细节一致：人名、称谓、地名、物品、数量、伤势、天气、时辰等须前后统一。④段落衔接：相邻段落须有因果、时间或空间上的延续；禁止硬切、跳剪式跳跃导致读者无法重建过程。⑤逻辑自洽：禁止为推剧情而强行降智、反常行为或违背常识的设定（除非章纲/知识卡明确为世界观规则）。⑥节奏与情绪：变化须符合章纲与剧情卡推进，禁止情绪或基调无源反转。⑦信息有效：每段应推进情节或刻画人物/氛围，禁止无意义同义反复与凑字数。",
        "forbidden": "严禁：前后段落、场景或时间线逻辑冲突；同一事实在章内前后矛盾；人物言行与人设/当前处境不符且无解释；未在大纲或 linked_plots 中出现的重大新设定、无关支线或另起主线；缺乏因果铺垫的「机械降神」式巧合解决核心矛盾（除非大纲明确要求）；场景硬切、对话说明文式生硬灌设定；与 linked_plots 顺序或要点相悖的叙述；复制粘贴式重复段落；打破第四面墙；输出写作过程、自我评价、提纲清单或评分。",
        "length": "正文字数须符合 get_novel_info 返回的 word_count_min / word_count_max（应用内允许 ±60 字误差）；不得明显低于下限或超过上限。",
        "output": "只输出本章 Markdown 正文（可用 `# 章标题` 开头，或直接正文）；禁止输出注释、写作说明、「本章完」、检查清单或自我评分。",
        "content_usage": "本接口不含正文。生成新章：仅依据 node.outline、linked_* 与 ai_guidance 写全新正文，禁止调用 get_chapter_content，禁止参考磁盘旧稿。精修/改稿已有正文时：先 get_chapter_info 取约束，再 get_chapter_content 读旧稿，改完后 set_chapter_content。"
    })
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
                        "outline": v.outline,
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
        n.outline = o.to_string();
    }
    let out = n.clone();
    save_tree_notify(ctx, tree, false)?;
    Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
}

fn upsert_character(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let name = arg_str(&args, "name")?;
    let mut tree = get_tree(novel_id)?;
    let card = CharacterCard {
        role: args
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        gender: args
            .get("gender")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        alignment: args
            .get("alignment")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        personality: args
            .get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        style: args
            .get("style")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        motto: args
            .get("motto")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };
    let node_id = args
        .get("node_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
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
        n.label = name;
        n.character = Some(card);
        id
    } else {
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
            character: Some(card),
            knowledge: None,
            side_plot: None,
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
    let out = tree.nodes.iter().find(|n| n.id == id).cloned();
    Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
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
    let node_id = args
        .get("node_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
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
            character: None,
            knowledge: None,
            side_plot: Some(SidePlotMeta {
                status,
                absorbed: false,
            }),
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
    let out = tree.nodes.iter().find(|n| n.id == id).cloned();
    Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
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
    let title = arg_str(&args, "title")?;
    let mut tree = get_tree(novel_id)?;
    let node_id = args
        .get("node_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let created = node_id.is_none();
    let id = if let Some(id) = node_id {
        let n = tree
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| "节点不存在".to_string())?;
        if !matches!(n.kind, NodeKind::Knowledge) {
            return Err("不是知识卡".into());
        }
        let mut payload = n.knowledge.clone().unwrap_or_default();
        patch_knowledge_payload(&mut payload, &args);
        n.label = title;
        n.outline = payload.extracted.chars().take(200).collect();
        n.knowledge = Some(payload);
        id
    } else {
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
            label: title,
            outline: payload.extracted.chars().take(200).collect(),
            character: None,
            knowledge: Some(payload),
            side_plot: None,
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
        id
    };
    maybe_link_new_card(
        ctx,
        &mut tree,
        created,
        &id,
        "knowledge",
        args.get("link_to").and_then(|v| v.as_str()),
    )?;
    save_tree_notify(ctx, tree.clone(), created)?;
    let out = tree.nodes.iter().find(|n| n.id == id).cloned();
    Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
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
    let out = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .cloned()
        .ok_or_else(|| "节点不存在".to_string())?;
    Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
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

fn unlink_nodes(ctx: &McpCtx, args: Value) -> Result<String, String> {
    let novel_id = arg_novel_id(ctx, &args)?;
    let mut tree = get_tree(novel_id)?;
    if let Some(eid) = args.get("edge_id").and_then(|v| v.as_str()) {
        tree.edges.retain(|e| e.id != eid);
    } else {
        let a = arg_str(&args, "source_id")?;
        let b = arg_str(&args, "target_id")?;
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
    if na.kind == card_kind {
        Ok((b.to_string(), a.to_string()))
    } else if nb.kind == card_kind {
        Ok((a.to_string(), b.to_string()))
    } else {
        Ok((a.to_string(), b.to_string()))
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
    let (sh, th) = match kind {
        "knowledge" | "character" => ("left", "right"), // 章左 ← 卡右
        "chapter" | "volume" => ("bottom", "top"),      // 上卡底 → 下卡顶
        _ => ("right", "left"),                         // 章右 → 剧情左
    };
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
            character: None,
            knowledge: None,
            side_plot: None,
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
    fn patch_knowledge_keeps_omitted_fields() {
        let mut p = KnowledgeCardPayload {
            book_ids: vec!["a".into()],
            extract_prompt: "节奏".into(),
            extracted: "旧摘要".into(),
            from_canon: true,
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
    fn chapter_write_ai_guidance_has_coherence_fields() {
        let g = chapter_write_ai_guidance();
        for k in [
            "plots",
            "characters",
            "knowledge",
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
}
