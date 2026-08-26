use crate::models::{AppSettings, KnowledgeBook, ModelCatalog, NovelProject, PublicKnowledgeCard};
use crate::paths::db_path;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open() -> Result<Self> {
        let conn = Connection::open(db_path()).context("open sqlite")?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS knowledge_books (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              author TEXT NOT NULL,
              genres TEXT NOT NULL,
              source_path TEXT NOT NULL,
              extract_prompt TEXT NOT NULL,
              created_at TEXT NOT NULL,
              chunk_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS knowledge_chunks (
              id TEXT PRIMARY KEY,
              book_id TEXT NOT NULL,
              idx INTEGER NOT NULL,
              content TEXT NOT NULL,
              FOREIGN KEY(book_id) REFERENCES knowledge_books(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS knowledge_embeddings (
              book_id TEXT NOT NULL,
              idx INTEGER NOT NULL,
              dim INTEGER NOT NULL,
              vector BLOB NOT NULL,
              PRIMARY KEY(book_id, idx)
            );
            CREATE TABLE IF NOT EXISTS novels (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              synopsis TEXT NOT NULL,
              cover_path TEXT,
              knowledge_ids TEXT NOT NULL,
              knowledge_strategy TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chat_messages (
              id TEXT PRIMARY KEY,
              novel_id TEXT NOT NULL,
              role TEXT NOT NULL,
              content TEXT NOT NULL,
              created_at TEXT NOT NULL,
              node_id TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS token_usage (
              day TEXT NOT NULL,
              hour INTEGER NOT NULL,
              model TEXT NOT NULL,
              novel_id TEXT NOT NULL DEFAULT '',
              prompt_tokens INTEGER NOT NULL DEFAULT 0,
              completion_tokens INTEGER NOT NULL DEFAULT 0,
              total_tokens INTEGER NOT NULL DEFAULT 0,
              PRIMARY KEY (day, hour, model, novel_id)
            );
            CREATE TABLE IF NOT EXISTS chapter_memory (
              id TEXT PRIMARY KEY,
              novel_id TEXT NOT NULL,
              node_id TEXT NOT NULL,
              idx INTEGER NOT NULL,
              content TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chapter_memory_novel ON chapter_memory(novel_id);
            CREATE INDEX IF NOT EXISTS idx_chapter_memory_node ON chapter_memory(novel_id, node_id);
            "#,
        )?;
        // ponytail: additive migrate; ignore if column exists
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN model TEXT NOT NULL DEFAULT 'deepseek-v4-flash'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN word_count_min INTEGER NOT NULL DEFAULT 2000",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN word_count_max INTEGER NOT NULL DEFAULT 3000",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN chapter_count INTEGER NOT NULL DEFAULT 20",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN canon_mode TEXT NOT NULL DEFAULT 'reference'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE novels ADD COLUMN features TEXT NOT NULL DEFAULT '{}'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE knowledge_books ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE chat_messages ADD COLUMN node_id TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_messages_node ON chat_messages(novel_id, node_id)",
            [],
        );
        let _ = conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS public_knowledge_cards (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              book_ids TEXT NOT NULL,
              extract_prompt TEXT NOT NULL,
              extracted TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              archived INTEGER NOT NULL DEFAULT 0
            );
            "#,
        );
        let _ = conn.execute(
            "ALTER TABLE public_knowledge_cards ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        );
        migrate_token_usage_novel_id(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_settings(&self) -> Result<AppSettings> {
        let (raws, plain) = {
            let conn = self.conn.lock().unwrap();
            let mut raws = std::collections::HashMap::<String, String>::new();
            let mut plain = std::collections::HashMap::<String, String>::new();
            let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (k, v) = row?;
                match k.as_str() {
                    "deepseek_api_key" | "chatgpt_api_key" | "gemini_api_key"
                    | "claude_api_key" | "grok_api_key" | "kimi_api_key" => {
                        raws.insert(k, v);
                    }
                    _ => {
                        plain.insert(k, v);
                    }
                }
            }
            (raws, plain)
        };

        let decrypt = |k: &str| -> Result<String> {
            crate::secret::decrypt_secret(raws.get(k).map(|s| s.as_str()).unwrap_or(""))
        };
        let d = AppSettings::default();
        let get = |k: &str, fallback: &str| {
            plain
                .get(k)
                .map(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback)
                .to_string()
        };

        // 仅当从未写过 compat_providers 行时，才从旧单项 Key 迁移一次。
        // 已保存过空列表 `[]` 的，视为用户主动清空，禁止再次灌回。
        let compat_row = plain.get("compat_providers");
        let compat_ever_saved = compat_row.is_some();
        let mut compat_providers = Self::load_compat_providers(compat_row)?;

        let mut deepseek_api_key = decrypt("deepseek_api_key")?;
        let mut chatgpt_api_key = decrypt("chatgpt_api_key")?;
        let gemini_api_key = decrypt("gemini_api_key")?;
        let claude_api_key = decrypt("claude_api_key")?;
        let mut grok_api_key = decrypt("grok_api_key")?;
        let mut kimi_api_key = decrypt("kimi_api_key")?;
        let deepseek_base_url = get("deepseek_base_url", &d.deepseek_base_url);
        let kimi_base_url = get("kimi_base_url", &d.kimi_base_url);

        let mut migrated = false;
        if !compat_ever_saved && compat_providers.is_empty() {
            if !deepseek_api_key.trim().is_empty() {
                compat_providers.push(crate::models::CompatProvider {
                    id: uuid::Uuid::new_v4().to_string(),
                    label: "DeepSeek".into(),
                    protocol: "openai".into(),
                    base_url: deepseek_base_url.clone(),
                    api_key: deepseek_api_key.clone(),
                    models: vec![],
                });
                migrated = true;
            }
            if !kimi_api_key.trim().is_empty() {
                compat_providers.push(crate::models::CompatProvider {
                    id: uuid::Uuid::new_v4().to_string(),
                    label: "Kimi".into(),
                    protocol: "openai".into(),
                    base_url: kimi_base_url.clone(),
                    api_key: kimi_api_key.clone(),
                    models: vec![],
                });
                migrated = true;
            }
            if !chatgpt_api_key.trim().is_empty() {
                compat_providers.push(crate::models::CompatProvider {
                    id: uuid::Uuid::new_v4().to_string(),
                    label: "ChatGPT".into(),
                    protocol: "openai".into(),
                    base_url: "https://api.openai.com".into(),
                    api_key: chatgpt_api_key.clone(),
                    models: vec![],
                });
                migrated = true;
            }
            if !grok_api_key.trim().is_empty() {
                compat_providers.push(crate::models::CompatProvider {
                    id: uuid::Uuid::new_v4().to_string(),
                    label: "Grok".into(),
                    protocol: "openai".into(),
                    base_url: "https://api.x.ai".into(),
                    api_key: grok_api_key.clone(),
                    models: vec![],
                });
                migrated = true;
            }
            // 迁入列表后清掉旧单项，避免删光列表后又被迁移灌回
            if migrated {
                deepseek_api_key.clear();
                kimi_api_key.clear();
                chatgpt_api_key.clear();
                grok_api_key.clear();
            }
        }

        let s = AppSettings {
            compat_providers,
            deepseek_api_key,
            chatgpt_api_key,
            gemini_api_key,
            claude_api_key,
            grok_api_key,
            kimi_api_key,
            deepseek_base_url,
            kimi_base_url,
            default_model: get("default_model", &d.default_model),
            create_model: get("create_model", &d.create_model),
            generate_model: get("generate_model", &d.generate_model),
            chat_model: get("chat_model", &d.chat_model),
            refine_model: get("refine_model", &d.refine_model),
            knowledge_model: get("knowledge_model", &d.knowledge_model),
            ui_locale: get("ui_locale", &d.ui_locale),
            mcp_port: get("mcp_port", &d.mcp_port.to_string())
                .parse()
                .unwrap_or(d.mcp_port),
            mcp_enabled: match get("mcp_enabled", if d.mcp_enabled { "1" } else { "0" }).as_str()
            {
                "0" | "false" | "False" | "no" => false,
                _ => true,
            },
            mcp_lan: matches!(
                get("mcp_lan", if d.mcp_lan { "1" } else { "0" }).as_str(),
                "1" | "true" | "True" | "yes"
            ),
        };
        // migrate legacy plaintext → encrypted on read
        let needs_migrate = migrated
            || raws
                .values()
                .any(|v| !v.is_empty() && !v.starts_with("nw1:"));
        if needs_migrate {
            let _ = self.save_settings(&s);
        }
        Ok(s)
    }

    fn load_compat_providers(raw: Option<&String>) -> Result<Vec<crate::models::CompatProvider>> {
        let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
            return Ok(vec![]);
        };
        #[derive(serde::Deserialize)]
        struct Stored {
            id: String,
            label: String,
            protocol: String,
            base_url: String,
            api_key: String,
            #[serde(default)]
            models: Vec<String>,
        }
        let stored: Vec<Stored> = serde_json::from_str(raw).unwrap_or_default();
        let mut out = Vec::with_capacity(stored.len());
        for p in stored {
            out.push(crate::models::CompatProvider {
                id: p.id,
                label: p.label,
                protocol: p.protocol,
                base_url: p.base_url,
                api_key: crate::secret::decrypt_secret(&p.api_key)?,
                models: p.models,
            });
        }
        Ok(out)
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        let enc = |plain: &str| crate::secret::encrypt_secret(plain);
        let compat_json = {
            let stored: Vec<serde_json::Value> = s
                .compat_providers
                .iter()
                .map(|p| {
                    Ok(serde_json::json!({
                        "id": p.id,
                        "label": p.label,
                        "protocol": p.protocol,
                        "base_url": p.base_url,
                        "api_key": enc(&p.api_key)?,
                        "models": p.models,
                    }))
                })
                .collect::<Result<Vec<_>>>()?;
            serde_json::to_string(&stored)?
        };
        let pairs = [
            ("compat_providers", compat_json),
            ("deepseek_api_key", enc(&s.deepseek_api_key)?),
            ("chatgpt_api_key", enc(&s.chatgpt_api_key)?),
            ("gemini_api_key", enc(&s.gemini_api_key)?),
            ("claude_api_key", enc(&s.claude_api_key)?),
            ("grok_api_key", enc(&s.grok_api_key)?),
            ("kimi_api_key", enc(&s.kimi_api_key)?),
            ("deepseek_base_url", s.deepseek_base_url.clone()),
            ("kimi_base_url", s.kimi_base_url.clone()),
            ("default_model", s.default_model.clone()),
            ("create_model", s.create_model.clone()),
            ("generate_model", s.generate_model.clone()),
            ("chat_model", s.chat_model.clone()),
            ("refine_model", s.refine_model.clone()),
            ("knowledge_model", s.knowledge_model.clone()),
            ("ui_locale", s.ui_locale.clone()),
            ("mcp_port", s.mcp_port.to_string()),
            (
                "mcp_enabled",
                if s.mcp_enabled { "1" } else { "0" }.to_string(),
            ),
            ("mcp_lan", if s.mcp_lan { "1" } else { "0" }.to_string()),
        ];
        let conn = self.conn.lock().unwrap();
        for (k, v) in pairs {
            conn.execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![k, v],
            )?;
        }
        Ok(())
    }

    pub fn get_model_catalog(&self) -> Result<ModelCatalog> {
        let conn = self.conn.lock().unwrap();
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key='model_catalog'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match raw {
            Some(s) if !s.trim().is_empty() => {
                serde_json::from_str(&s).unwrap_or_else(|_| ModelCatalog::seed())
            }
            _ => ModelCatalog::seed(),
        })
    }

    pub fn save_model_catalog(&self, c: &ModelCatalog) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let v = serde_json::to_string(c)?;
        conn.execute(
            "INSERT INTO settings(key, value) VALUES('model_catalog', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![v],
        )?;
        Ok(())
    }

    pub fn list_knowledge(&self) -> Result<Vec<KnowledgeBook>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, author, genres, source_path, extract_prompt, created_at, chunk_count,
                    COALESCE(archived, 0)
             FROM knowledge_books ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let genres: String = row.get(3)?;
            let archived_i: i64 = row.get(8).unwrap_or(0);
            Ok(KnowledgeBook {
                id: row.get(0)?,
                title: row.get(1)?,
                author: row.get(2)?,
                genres: serde_json::from_str(&genres).unwrap_or_default(),
                source_path: row.get(4)?,
                extract_prompt: row.get(5)?,
                created_at: row.get(6)?,
                chunk_count: row.get(7)?,
                archived: archived_i != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_knowledge(&self, id: &str) -> Result<Option<KnowledgeBook>> {
        Ok(self
            .list_knowledge()?
            .into_iter()
            .find(|b| b.id == id))
    }

    pub fn list_public_knowledge_cards(&self) -> Result<Vec<PublicKnowledgeCard>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, book_ids, extract_prompt, extracted, created_at, updated_at,
                    COALESCE(archived, 0)
             FROM public_knowledge_cards ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let book_ids: String = row.get(2)?;
            let archived_i: i64 = row.get(7).unwrap_or(0);
            Ok(PublicKnowledgeCard {
                id: row.get(0)?,
                title: row.get(1)?,
                book_ids: serde_json::from_str(&book_ids).unwrap_or_default(),
                extract_prompt: row.get(3)?,
                extracted: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                archived: archived_i != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_public_knowledge_card(&self, id: &str) -> Result<Option<PublicKnowledgeCard>> {
        Ok(self
            .list_public_knowledge_cards()?
            .into_iter()
            .find(|c| c.id == id))
    }

    pub fn upsert_public_knowledge_card(&self, card: &PublicKnowledgeCard) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let books = serde_json::to_string(&card.book_ids)?;
        conn.execute(
            "INSERT INTO public_knowledge_cards(id, title, book_ids, extract_prompt, extracted, created_at, updated_at, archived)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title,
               book_ids=excluded.book_ids,
               extract_prompt=excluded.extract_prompt,
               extracted=excluded.extracted,
               updated_at=excluded.updated_at",
            params![
                card.id,
                card.title,
                books,
                card.extract_prompt,
                card.extracted,
                card.created_at,
                card.updated_at,
                card.archived as i64
            ],
        )?;
        Ok(())
    }

    pub fn set_public_knowledge_card_archived(&self, id: &str, archived: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE public_knowledge_cards SET archived=?1 WHERE id=?2",
            params![archived as i64, id],
        )?;
        if n == 0 {
            anyhow::bail!("public knowledge card not found");
        }
        Ok(())
    }

    pub fn delete_public_knowledge_card(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM public_knowledge_cards WHERE id=?1", params![id])?;
        if n == 0 {
            anyhow::bail!("public knowledge card not found");
        }
        Ok(())
    }

    pub fn set_knowledge_archived(&self, id: &str, archived: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE knowledge_books SET archived=?1 WHERE id=?2",
            params![archived as i64, id],
        )?;
        if n == 0 {
            anyhow::bail!("knowledge book not found");
        }
        Ok(())
    }

    pub fn delete_knowledge(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "DELETE FROM knowledge_embeddings WHERE book_id=?1",
            params![id],
        );
        // chunks CASCADE via FK
        let n = conn.execute("DELETE FROM knowledge_books WHERE id=?1", params![id])?;
        if n == 0 {
            anyhow::bail!("knowledge book not found");
        }
        Ok(())
    }

    /// 从所有小说的 knowledge_ids 里摘掉已删书目。
    pub fn unlink_knowledge_from_novels(&self, book_id: &str) -> Result<()> {
        let novels = self.list_novels()?;
        for mut n in novels {
            let before = n.knowledge_ids.len();
            n.knowledge_ids.retain(|id| id != book_id);
            if n.knowledge_ids.len() != before {
                self.upsert_novel(&n)?;
            }
        }
        Ok(())
    }

    pub fn rename_knowledge(&self, id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            anyhow::bail!("title empty");
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE knowledge_books SET title=?1 WHERE id=?2",
            params![title, id],
        )?;
        if n == 0 {
            anyhow::bail!("knowledge book not found");
        }
        Ok(())
    }

    pub fn update_knowledge(
        &self,
        id: &str,
        title: &str,
        author: &str,
        extract_prompt: &str,
        genres: &[String],
    ) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            anyhow::bail!("title empty");
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE knowledge_books SET title=?1, author=?2, extract_prompt=?3, genres=?4 WHERE id=?5",
            params![
                title,
                author.trim(),
                extract_prompt,
                serde_json::to_string(genres)?,
                id
            ],
        )?;
        if n == 0 {
            anyhow::bail!("knowledge book not found");
        }
        Ok(())
    }

    pub fn list_knowledge_chunks(
        &self,
        book_id: &str,
    ) -> Result<Vec<crate::models::KnowledgeChunk>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT idx, content FROM knowledge_chunks WHERE book_id=?1 ORDER BY idx ASC",
        )?;
        let rows = stmt.query_map(params![book_id], |row| {
            Ok(crate::models::KnowledgeChunk {
                idx: row.get::<_, i64>(0)? as u32,
                content: row.get(1)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn upsert_knowledge_embedding(&self, book_id: &str, idx: i64, vector: &[f32]) -> Result<()> {
        let mut bytes = Vec::with_capacity(vector.len() * 4);
        for v in vector {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO knowledge_embeddings(book_id, idx, dim, vector) VALUES(?1,?2,?3,?4)
             ON CONFLICT(book_id, idx) DO UPDATE SET dim=excluded.dim, vector=excluded.vector",
            params![book_id, idx, vector.len() as i64, bytes],
        )?;
        Ok(())
    }

    pub fn get_knowledge_embedding(&self, book_id: &str, idx: i64) -> Result<Option<Vec<f32>>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(i64, Vec<u8>)> = conn
            .query_row(
                "SELECT dim, vector FROM knowledge_embeddings WHERE book_id=?1 AND idx=?2",
                params![book_id, idx],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((dim, bytes)) = row else {
            return Ok(None);
        };
        if dim <= 0 || bytes.len() != (dim as usize) * 4 {
            return Ok(None);
        }
        let mut out = Vec::with_capacity(dim as usize);
        for chunk in bytes.chunks_exact(4) {
            out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(Some(out))
    }

    pub fn count_knowledge_embeddings(&self, book_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge_embeddings WHERE book_id=?1",
            params![book_id],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    pub fn delete_knowledge_embeddings(&self, book_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM knowledge_embeddings WHERE book_id=?1",
            params![book_id],
        )?;
        Ok(())
    }

    pub fn insert_knowledge(&self, book: &KnowledgeBook, chunks: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO knowledge_books(id, title, author, genres, source_path, extract_prompt, created_at, chunk_count)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                book.id,
                book.title,
                book.author,
                serde_json::to_string(&book.genres)?,
                book.source_path,
                book.extract_prompt,
                book.created_at,
                chunks.len() as i64
            ],
        )?;
        for (i, c) in chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO knowledge_chunks(id, book_id, idx, content) VALUES(?1,?2,?3,?4)",
                params![uuid::Uuid::new_v4().to_string(), book.id, i as i64, c],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Replace stored chunks + refresh book meta (keeps id / created_at / archived).
    pub fn replace_knowledge_content(
        &self,
        id: &str,
        title: &str,
        author: &str,
        genres: &[String],
        source_path: &str,
        extract_prompt: &str,
        chunks: &[String],
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE knowledge_books SET title=?1, author=?2, genres=?3, source_path=?4,
             extract_prompt=?5, chunk_count=?6 WHERE id=?7",
            params![
                title,
                author,
                serde_json::to_string(genres)?,
                source_path,
                extract_prompt,
                chunks.len() as i64,
                id
            ],
        )?;
        if n == 0 {
            anyhow::bail!("knowledge book not found");
        }
        tx.execute("DELETE FROM knowledge_chunks WHERE book_id=?1", params![id])?;
        for (i, c) in chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO knowledge_chunks(id, book_id, idx, content) VALUES(?1,?2,?3,?4)",
                params![uuid::Uuid::new_v4().to_string(), id, i as i64, c],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_novels(&self) -> Result<Vec<NovelProject>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, synopsis, cover_path, knowledge_ids, knowledge_strategy, created_at, updated_at,
                    COALESCE(archived, 0), COALESCE(word_count_min, 2000), COALESCE(word_count_max, 3000),
                    COALESCE(chapter_count, 20), COALESCE(canon_mode, 'reference'), COALESCE(features, '{}')
             FROM novels ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], map_novel)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_novel(&self, id: &str) -> Result<Option<NovelProject>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, synopsis, cover_path, knowledge_ids, knowledge_strategy, created_at, updated_at,
                    COALESCE(archived, 0), COALESCE(word_count_min, 2000), COALESCE(word_count_max, 3000),
                    COALESCE(chapter_count, 20), COALESCE(canon_mode, 'reference'), COALESCE(features, '{}')
             FROM novels WHERE id=?1",
        )?;
        let mut rows = stmt.query_map(params![id], map_novel)?;
        Ok(rows.next().transpose()?)
    }

    pub fn upsert_novel(&self, n: &NovelProject) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mode = normalize_canon_mode(&n.canon_mode);
        let features = serde_json::to_string(&n.features)?;
        conn.execute(
            "INSERT INTO novels(id, title, synopsis, cover_path, knowledge_ids, knowledge_strategy, created_at, updated_at, archived, word_count_min, word_count_max, chapter_count, canon_mode, features)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title,
               synopsis=excluded.synopsis,
               cover_path=excluded.cover_path,
               knowledge_ids=excluded.knowledge_ids,
               knowledge_strategy=excluded.knowledge_strategy,
               updated_at=excluded.updated_at,
               archived=excluded.archived,
               word_count_min=excluded.word_count_min,
               word_count_max=excluded.word_count_max,
               chapter_count=excluded.chapter_count,
               canon_mode=excluded.canon_mode,
               features=excluded.features",
            params![
                n.id,
                n.title,
                n.synopsis,
                n.cover_path,
                serde_json::to_string(&n.knowledge_ids)?,
                n.knowledge_strategy,
                n.created_at,
                n.updated_at,
                n.archived as i64,
                n.word_count_min as i64,
                n.word_count_max as i64,
                n.chapter_count as i64,
                mode,
                features,
            ],
        )?;
        Ok(())
    }

    pub fn delete_novel(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM chapter_memory WHERE novel_id=?1", params![id])?;
        conn.execute("DELETE FROM chat_messages WHERE novel_id=?1", params![id])?;
        conn.execute("DELETE FROM novels WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn replace_chapter_memory(
        &self,
        novel_id: &str,
        node_id: &str,
        chunks: &[String],
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM chapter_memory WHERE novel_id=?1 AND node_id=?2",
            params![novel_id, node_id],
        )?;
        for (i, c) in chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO chapter_memory(id, novel_id, node_id, idx, content) VALUES(?1,?2,?3,?4,?5)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    novel_id,
                    node_id,
                    i as i64,
                    c
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_chapter_memory(&self, novel_id: &str, node_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM chapter_memory WHERE novel_id=?1 AND node_id=?2",
            params![novel_id, node_id],
        )?;
        Ok(())
    }

    /// `(node_id, content)` ordered by idx within each node.
    pub fn list_chapter_memory_for_nodes(
        &self,
        novel_id: &str,
        node_ids: &[String],
    ) -> Result<Vec<(String, String)>> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();
        for nid in node_ids {
            let mut stmt = conn.prepare(
                "SELECT node_id, content FROM chapter_memory WHERE novel_id=?1 AND node_id=?2 ORDER BY idx ASC",
            )?;
            let rows = stmt.query_map(params![novel_id, nid], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    /// All memory rows for a novel (for keyword ranking within that novel only).
    pub fn list_chapter_memory(&self, novel_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT node_id, content FROM chapter_memory WHERE novel_id=?1 ORDER BY node_id, idx",
        )?;
        let rows = stmt.query_map(params![novel_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_chat_for_node(&self, novel_id: &str, node_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM chat_messages WHERE novel_id=?1 AND COALESCE(node_id, '')=?2",
            params![novel_id, node_id],
        )?;
        Ok(())
    }

    pub fn add_token_usage(
        &self,
        day: &str,
        hour: u8,
        model: &str,
        novel_id: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO token_usage(day, hour, model, novel_id, prompt_tokens, completion_tokens, total_tokens)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(day, hour, model, novel_id) DO UPDATE SET
               prompt_tokens = prompt_tokens + excluded.prompt_tokens,
               completion_tokens = completion_tokens + excluded.completion_tokens,
               total_tokens = total_tokens + excluded.total_tokens",
            params![
                day,
                hour as i64,
                model,
                novel_id,
                prompt_tokens as i64,
                completion_tokens as i64,
                total_tokens as i64
            ],
        )?;
        Ok(())
    }

    pub fn list_token_usage_day(&self, day: &str) -> Result<Vec<crate::models::TokenUsageHourRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT hour, model,
                    SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens)
             FROM token_usage WHERE day=?1
             GROUP BY hour, model
             ORDER BY hour ASC, model ASC",
        )?;
        let rows = stmt.query_map(params![day], |row| {
            Ok(crate::models::TokenUsageHourRow {
                hour: row.get::<_, i64>(0)? as u8,
                model: row.get(1)?,
                prompt_tokens: row.get(2)?,
                completion_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 按小说汇总：当日 + 累计（仅 novel_id 非空的记录）。
    pub fn list_token_usage_by_novel(
        &self,
        day: &str,
    ) -> Result<Vec<crate::models::TokenUsageNovelRow>> {
        let conn = self.conn.lock().unwrap();
        let mut titles = std::collections::HashMap::<String, String>::new();
        {
            let mut stmt = conn.prepare("SELECT id, title FROM novels")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, title) = row?;
                titles.insert(id, title);
            }
        }
        // novel_id -> (day_prompt, day_completion, day_total, total_prompt, total_completion, total)
        let mut totals =
            std::collections::HashMap::<String, (i64, i64, i64, i64, i64, i64)>::new();
        {
            let mut stmt = conn.prepare(
                "SELECT novel_id,
                        SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens)
                 FROM token_usage
                 WHERE novel_id != '' GROUP BY novel_id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (id, p, c, t) = row?;
                totals.insert(id, (0, 0, 0, p, c, t));
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT novel_id,
                        SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens)
                 FROM token_usage
                 WHERE novel_id != '' AND day=?1 GROUP BY novel_id",
            )?;
            let rows = stmt.query_map(params![day], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (id, p, c, t) = row?;
                let e = totals.entry(id).or_insert((0, 0, 0, 0, 0, 0));
                e.0 = p;
                e.1 = c;
                e.2 = t;
            }
        }
        let mut out: Vec<crate::models::TokenUsageNovelRow> = totals
            .into_iter()
            .map(
                |(
                    novel_id,
                    (
                        day_prompt_tokens,
                        day_completion_tokens,
                        day_tokens,
                        total_prompt_tokens,
                        total_completion_tokens,
                        total_tokens,
                    ),
                )| {
                    let title = titles
                        .get(&novel_id)
                        .cloned()
                        .unwrap_or_else(|| format!("({novel_id})"));
                    crate::models::TokenUsageNovelRow {
                        novel_id,
                        title,
                        day_prompt_tokens,
                        day_completion_tokens,
                        day_tokens,
                        total_prompt_tokens,
                        total_completion_tokens,
                        total_tokens,
                    }
                },
            )
            .collect();
        out.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        Ok(out)
    }

    /// 按月汇总各模型 token（month = YYYY-MM）。
    pub fn list_token_usage_month(
        &self,
        month: &str,
    ) -> Result<Vec<crate::models::TokenUsageModelRow>> {
        let like = format!("{month}-%");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT model,
                    SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens)
             FROM token_usage
             WHERE day LIKE ?1
             GROUP BY model
             ORDER BY SUM(total_tokens) DESC",
        )?;
        let rows = stmt.query_map(params![like], |row| {
            Ok(crate::models::TokenUsageModelRow {
                model: row.get(0)?,
                prompt_tokens: row.get(1)?,
                completion_tokens: row.get(2)?,
                total_tokens: row.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

/// 旧表无 novel_id 时重建（历史行归到 novel_id=''）。
fn migrate_token_usage_novel_id(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(token_usage)")?;
    let has = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|name| name == "novel_id");
    if has {
        return Ok(());
    }
    conn.execute_batch(
        r#"
        CREATE TABLE token_usage_v2 (
          day TEXT NOT NULL,
          hour INTEGER NOT NULL,
          model TEXT NOT NULL,
          novel_id TEXT NOT NULL DEFAULT '',
          prompt_tokens INTEGER NOT NULL DEFAULT 0,
          completion_tokens INTEGER NOT NULL DEFAULT 0,
          total_tokens INTEGER NOT NULL DEFAULT 0,
          PRIMARY KEY (day, hour, model, novel_id)
        );
        INSERT INTO token_usage_v2(day, hour, model, novel_id, prompt_tokens, completion_tokens, total_tokens)
          SELECT day, hour, model, '', prompt_tokens, completion_tokens, total_tokens FROM token_usage;
        DROP TABLE token_usage;
        ALTER TABLE token_usage_v2 RENAME TO token_usage;
        "#,
    )?;
    Ok(())
}

fn map_novel(row: &rusqlite::Row<'_>) -> rusqlite::Result<NovelProject> {
    let kids: String = row.get(4)?;
    let archived_i: i64 = row.get(8).unwrap_or(0);
    let mode: String = row.get(12).unwrap_or_else(|_| "reference".into());
    let features_raw: String = row.get(13).unwrap_or_else(|_| "{}".into());
    Ok(NovelProject {
        id: row.get(0)?,
        title: row.get(1)?,
        synopsis: row.get(2)?,
        cover_path: row.get(3)?,
        knowledge_ids: serde_json::from_str(&kids).unwrap_or_default(),
        knowledge_strategy: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        archived: archived_i != 0,
        word_count_min: row.get::<_, i64>(9).unwrap_or(2000) as u32,
        word_count_max: row.get::<_, i64>(10).unwrap_or(3000) as u32,
        chapter_count: row.get::<_, i64>(11).unwrap_or(20) as u32,
        canon_mode: normalize_canon_mode(&mode),
        features: serde_json::from_str(&features_raw).unwrap_or_default(),
    })
}

fn normalize_canon_mode(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().as_str() {
        "strict" | "locked" => "strict".into(),
        _ => "reference".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_like_aggregates_models() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (
               day TEXT, hour INTEGER, model TEXT, novel_id TEXT DEFAULT '',
               prompt_tokens INTEGER, completion_tokens INTEGER, total_tokens INTEGER
             );
             INSERT INTO token_usage VALUES
               ('2026-08-01', 10, 'm1', '', 1, 1, 10),
               ('2026-08-15', 11, 'm1', '', 1, 1, 20),
               ('2026-07-01', 9, 'm2', '', 1, 1, 99);",
        )
        .unwrap();
        let like = "2026-08-%";
        let mut stmt = conn
            .prepare(
                "SELECT model, SUM(total_tokens) FROM token_usage
                 WHERE day LIKE ?1 GROUP BY model ORDER BY SUM(total_tokens) DESC",
            )
            .unwrap();
        let rows: Vec<(String, i64)> = stmt
            .query_map(params![like], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows, vec![("m1".into(), 30)]);
    }
}
