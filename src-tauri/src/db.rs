use crate::models::{AppSettings, ChatMessage, KnowledgeBook, ModelCatalog, NovelProject};
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
              created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS token_usage (
              day TEXT NOT NULL,
              hour INTEGER NOT NULL,
              model TEXT NOT NULL,
              prompt_tokens INTEGER NOT NULL DEFAULT 0,
              completion_tokens INTEGER NOT NULL DEFAULT 0,
              total_tokens INTEGER NOT NULL DEFAULT 0,
              PRIMARY KEY (day, hour, model)
            );
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
                    "deepseek_api_key" | "chatgpt_api_key" | "gemini_api_key" | "claude_api_key"
                    | "grok_api_key" => {
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
        let s = AppSettings {
            deepseek_api_key: decrypt("deepseek_api_key")?,
            chatgpt_api_key: decrypt("chatgpt_api_key")?,
            gemini_api_key: decrypt("gemini_api_key")?,
            claude_api_key: decrypt("claude_api_key")?,
            grok_api_key: decrypt("grok_api_key")?,
            deepseek_base_url: get("deepseek_base_url", &d.deepseek_base_url),
            default_model: get("default_model", &d.default_model),
            create_model: get("create_model", &d.create_model),
            generate_model: get("generate_model", &d.generate_model),
            chat_model: get("chat_model", &d.chat_model),
            refine_model: get("refine_model", &d.refine_model),
            ui_locale: get("ui_locale", &d.ui_locale),
        };
        // migrate legacy plaintext → encrypted on read
        let needs_migrate = raws.values().any(|v| !v.is_empty() && !v.starts_with("nw1:"));
        if needs_migrate {
            let _ = self.save_settings(&s);
        }
        Ok(s)
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        let enc = |plain: &str| crate::secret::encrypt_secret(plain);
        let pairs = [
            ("deepseek_api_key", enc(&s.deepseek_api_key)?),
            ("chatgpt_api_key", enc(&s.chatgpt_api_key)?),
            ("gemini_api_key", enc(&s.gemini_api_key)?),
            ("claude_api_key", enc(&s.claude_api_key)?),
            ("grok_api_key", enc(&s.grok_api_key)?),
            ("deepseek_base_url", s.deepseek_base_url.clone()),
            ("default_model", s.default_model.clone()),
            ("create_model", s.create_model.clone()),
            ("generate_model", s.generate_model.clone()),
            ("chat_model", s.chat_model.clone()),
            ("refine_model", s.refine_model.clone()),
            ("ui_locale", s.ui_locale.clone()),
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
            "SELECT id, title, author, genres, source_path, extract_prompt, created_at, chunk_count
             FROM knowledge_books ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let genres: String = row.get(3)?;
            Ok(KnowledgeBook {
                id: row.get(0)?,
                title: row.get(1)?,
                author: row.get(2)?,
                genres: serde_json::from_str(&genres).unwrap_or_default(),
                source_path: row.get(4)?,
                extract_prompt: row.get(5)?,
                created_at: row.get(6)?,
                chunk_count: row.get(7)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
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

    pub fn list_novels(&self) -> Result<Vec<NovelProject>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, synopsis, cover_path, knowledge_ids, knowledge_strategy, created_at, updated_at,
                    COALESCE(archived, 0), COALESCE(word_count_min, 2000), COALESCE(word_count_max, 3000)
             FROM novels ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], map_novel)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_novel(&self, id: &str) -> Result<Option<NovelProject>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, synopsis, cover_path, knowledge_ids, knowledge_strategy, created_at, updated_at,
                    COALESCE(archived, 0), COALESCE(word_count_min, 2000), COALESCE(word_count_max, 3000)
             FROM novels WHERE id=?1",
        )?;
        let mut rows = stmt.query_map(params![id], map_novel)?;
        Ok(rows.next().transpose()?)
    }

    pub fn upsert_novel(&self, n: &NovelProject) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO novels(id, title, synopsis, cover_path, knowledge_ids, knowledge_strategy, created_at, updated_at, archived, word_count_min, word_count_max)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title,
               synopsis=excluded.synopsis,
               cover_path=excluded.cover_path,
               knowledge_ids=excluded.knowledge_ids,
               knowledge_strategy=excluded.knowledge_strategy,
               updated_at=excluded.updated_at,
               archived=excluded.archived,
               word_count_min=excluded.word_count_min,
               word_count_max=excluded.word_count_max",
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
            ],
        )?;
        Ok(())
    }

    pub fn delete_novel(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM chat_messages WHERE novel_id=?1", params![id])?;
        conn.execute("DELETE FROM novels WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn list_chat(&self, novel_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, novel_id, role, content, created_at FROM chat_messages
             WHERE novel_id=?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![novel_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                novel_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn insert_chat(&self, m: &ChatMessage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_messages(id, novel_id, role, content, created_at) VALUES(?1,?2,?3,?4,?5)",
            params![m.id, m.novel_id, m.role, m.content, m.created_at],
        )?;
        Ok(())
    }

    pub fn add_token_usage(
        &self,
        day: &str,
        hour: u8,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO token_usage(day, hour, model, prompt_tokens, completion_tokens, total_tokens)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(day, hour, model) DO UPDATE SET
               prompt_tokens = prompt_tokens + excluded.prompt_tokens,
               completion_tokens = completion_tokens + excluded.completion_tokens,
               total_tokens = total_tokens + excluded.total_tokens",
            params![
                day,
                hour as i64,
                model,
                prompt_tokens as i64,
                completion_tokens as i64,
                total_tokens as i64
            ],
        )?;
        Ok(())
    }

    pub fn list_token_usage_day(
        &self,
        day: &str,
    ) -> Result<Vec<crate::models::TokenUsageHourRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT hour, model, prompt_tokens, completion_tokens, total_tokens
             FROM token_usage WHERE day=?1 ORDER BY hour ASC, model ASC",
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
}

fn map_novel(row: &rusqlite::Row<'_>) -> rusqlite::Result<NovelProject> {
    let kids: String = row.get(4)?;
    let archived_i: i64 = row.get(8).unwrap_or(0);
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
    })
}
