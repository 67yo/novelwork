use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub deepseek_api_key: String,
    pub chatgpt_api_key: String,
    pub gemini_api_key: String,
    pub claude_api_key: String,
    pub grok_api_key: String,
    pub deepseek_base_url: String,
    /// llm::complete 未指定模型时的回退
    pub default_model: String,
    pub create_model: String,
    pub generate_model: String,
    pub chat_model: String,
    pub refine_model: String,
    /// UI 语言：system | en | zh-CN | zh-TW | ja | de | fr
    pub ui_locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCatalog {
    #[serde(default)]
    pub deepseek: Vec<String>,
    #[serde(default)]
    pub gemini: Vec<String>,
    #[serde(default)]
    pub claude: Vec<String>,
    #[serde(default)]
    pub grok: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub errors: std::collections::HashMap<String, String>,
}

impl ModelCatalog {
    pub fn all_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        ids.extend(self.deepseek.iter().cloned());
        ids.extend(self.gemini.iter().cloned());
        ids.extend(self.claude.iter().cloned());
        ids.extend(self.grok.iter().cloned());
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn seed() -> Self {
        Self {
            deepseek: vec![
                "deepseek-v4-flash".into(),
                "deepseek-v4-pro".into(),
                "deepseek-chat".into(),
                "deepseek-reasoner".into(),
            ],
            gemini: vec![],
            claude: vec![],
            grok: vec![],
            updated_at: String::new(),
            errors: Default::default(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            deepseek_api_key: String::new(),
            chatgpt_api_key: String::new(),
            gemini_api_key: String::new(),
            claude_api_key: String::new(),
            grok_api_key: String::new(),
            deepseek_base_url: "https://api.deepseek.com".into(),
            default_model: "deepseek-chat".into(),
            create_model: "deepseek-v4-flash".into(),
            generate_model: "deepseek-v4-flash".into(),
            chat_model: "deepseek-v4-flash".into(),
            refine_model: "deepseek-reasoner".into(),
            ui_locale: "system".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub deepseek_api_key_masked: String,
    pub deepseek_api_key_configured: bool,
    pub chatgpt_api_key_masked: String,
    pub chatgpt_api_key_configured: bool,
    pub gemini_api_key_masked: String,
    pub gemini_api_key_configured: bool,
    pub claude_api_key_masked: String,
    pub claude_api_key_configured: bool,
    pub grok_api_key_masked: String,
    pub grok_api_key_configured: bool,
    pub deepseek_base_url: String,
    pub default_model: String,
    pub create_model: String,
    pub generate_model: String,
    pub chat_model: String,
    pub refine_model: String,
    pub model_catalog: ModelCatalog,
    pub ui_locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsInput {
    #[serde(default, alias = "deepseek_api_key")]
    pub deepseek_api_key: Option<String>,
    #[serde(default, alias = "chatgpt_api_key")]
    pub chatgpt_api_key: Option<String>,
    #[serde(default, alias = "gemini_api_key")]
    pub gemini_api_key: Option<String>,
    #[serde(default, alias = "claude_api_key")]
    pub claude_api_key: Option<String>,
    #[serde(default, alias = "grok_api_key")]
    pub grok_api_key: Option<String>,
    #[serde(default, alias = "deepseek_base_url")]
    pub deepseek_base_url: Option<String>,
    #[serde(default, alias = "default_model")]
    pub default_model: Option<String>,
    #[serde(default, alias = "create_model")]
    pub create_model: Option<String>,
    #[serde(default, alias = "generate_model")]
    pub generate_model: Option<String>,
    #[serde(default, alias = "chat_model")]
    pub chat_model: Option<String>,
    #[serde(default, alias = "refine_model")]
    pub refine_model: Option<String>,
    #[serde(default, alias = "ui_locale")]
    pub ui_locale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBook {
    pub id: String,
    pub title: String,
    pub author: String,
    pub genres: Vec<String>,
    pub source_path: String,
    pub extract_prompt: String,
    pub created_at: String,
    pub chunk_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelProject {
    pub id: String,
    pub title: String,
    pub synopsis: String,
    pub cover_path: Option<String>,
    pub knowledge_ids: Vec<String>,
    pub knowledge_strategy: String,
    #[serde(default)]
    pub archived: bool,
    /// 每章目标字数下限
    #[serde(default = "default_word_min")]
    pub word_count_min: u32,
    /// 每章目标字数上限
    #[serde(default = "default_word_max")]
    pub word_count_max: u32,
    /// 全书计划章节数（剧情节奏约束）
    #[serde(default = "default_chapter_count")]
    pub chapter_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

fn default_word_min() -> u32 {
    2000
}
fn default_word_max() -> u32 {
    3000
}
fn default_chapter_count() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNovelInput {
    pub title: String,
    pub synopsis: String,
    #[serde(default)]
    pub knowledge_ids: Vec<String>,
    #[serde(default)]
    pub knowledge_strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NovelCreateChatInput {
    pub messages: Vec<ChatTurn>,
    /// 用户点「生成并进入工作台」时为 true，强制收束并建书
    #[serde(default)]
    pub force_create: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelCreateChatResult {
    pub reply: String,
    pub used_mock: bool,
    pub novel: Option<NovelProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Novel,
    Chapter,
    Character,
    SidePlot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub outline: String,
    pub character: Option<CharacterCard>,
    pub linked_character_ids: Vec<String>,
    pub linked_side_plot_ids: Vec<String>,
    pub position: NodePosition,
    /// 本章已生成正文的字数（非空白字符）
    #[serde(default)]
    pub word_count: u32,
    /// 根节点：目标字数下限（章节节点通常为 0）
    #[serde(default)]
    pub word_count_min: u32,
    /// 根节点：目标字数上限
    #[serde(default)]
    pub word_count_max: u32,
    /// 根节点：全书计划章节数
    #[serde(default)]
    pub chapter_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterCard {
    pub role: String,
    pub personality: String,
    pub motto: String,
    pub gender: String,
    pub style: String,
    pub alignment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    #[serde(default)]
    pub source_handle: Option<String>,
    #[serde(default)]
    pub target_handle: Option<String>,
    /// 人物↔人物：关系说明（供 AI 参考）
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelTree {
    pub novel_id: String,
    pub nodes: Vec<TreeNode>,
    pub edges: Vec<TreeEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub novel_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateResult {
    pub content: String,
    pub used_mock: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardChatResult {
    pub reply: String,
    pub tree: NovelTree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageHourRow {
    pub hour: u8,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageNovelRow {
    pub novel_id: String,
    pub title: String,
    pub day_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageDay {
    pub date: String,
    pub rows: Vec<TokenUsageHourRow>,
    pub models: Vec<String>,
    pub novels: Vec<TokenUsageNovelRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageModelRow {
    pub model: String,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageMonth {
    /// YYYY-MM
    pub month: String,
    pub total_tokens: i64,
    pub models: Vec<TokenUsageModelRow>,
}

pub const GENRES: &[&str] = &[
    "玄幻",
    "奇幻",
    "武侠",
    "仙侠",
    "都市",
    "言情",
    "古代言情",
    "现代言情",
    "豪门",
    "重生",
    "穿越",
    "快穿",
    "系统",
    "无限流",
    "末日",
    "科幻",
    "游戏",
    "电竞",
    "灵异",
    "悬疑",
    "推理",
    "历史",
    "军事",
    "竞技",
    "同人",
    "耽美",
    "百合",
    "情色",
    "种田",
    "宫斗",
    "宅斗",
    "职场",
    "青春",
    "校园",
    "娱乐圈",
    "美食",
    "治愈",
    "暗黑",
    "克苏鲁",
];
