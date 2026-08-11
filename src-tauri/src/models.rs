use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatProvider {
    pub id: String,
    pub label: String,
    /// currently only "openai"
    pub protocol: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatProviderView {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key_masked: String,
    pub api_key_configured: bool,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCompatProviderInput {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub base_url: String,
    /// empty / null / masked → keep existing key on update
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// OpenAI-compatible endpoints (DeepSeek / Kimi / custom). Primary LLM path.
    #[serde(default)]
    pub compat_providers: Vec<CompatProvider>,
    /// Legacy keys — migration only; LLM routes via compat_providers.
    #[serde(default)]
    pub deepseek_api_key: String,
    #[serde(default)]
    pub chatgpt_api_key: String,
    pub gemini_api_key: String,
    pub claude_api_key: String,
    #[serde(default)]
    pub grok_api_key: String,
    #[serde(default)]
    pub kimi_api_key: String,
    #[serde(default)]
    pub deepseek_base_url: String,
    #[serde(default)]
    pub kimi_base_url: String,
    /// llm::complete 未指定模型时的回退
    pub default_model: String,
    pub create_model: String,
    pub generate_model: String,
    pub chat_model: String,
    pub refine_model: String,
    pub knowledge_model: String,
    /// UI 语言：system | en | zh-CN | zh-TW | ja | de | fr
    pub ui_locale: String,
}

impl AppSettings {
    pub fn any_compat_key(&self) -> bool {
        self.compat_providers
            .iter()
            .any(|p| !p.api_key.trim().is_empty())
    }

    pub fn all_compat_model_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for p in &self.compat_providers {
            ids.extend(p.models.iter().cloned());
        }
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn resolve_compat<'a>(&'a self, model: &str) -> Option<&'a CompatProvider> {
        let m = model.trim();
        if let Some(p) = self
            .compat_providers
            .iter()
            .find(|p| !p.api_key.trim().is_empty() && p.models.iter().any(|x| x == m))
        {
            return Some(p);
        }
        self.compat_providers
            .iter()
            .find(|p| !p.api_key.trim().is_empty())
    }
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
    pub kimi: Vec<String>,
    /// Flattened ids from all compat providers (for task dropdowns).
    #[serde(default)]
    pub compat: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub errors: std::collections::HashMap<String, String>,
}

impl ModelCatalog {
    pub fn all_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        ids.extend(self.compat.iter().cloned());
        ids.extend(self.deepseek.iter().cloned());
        ids.extend(self.gemini.iter().cloned());
        ids.extend(self.claude.iter().cloned());
        ids.extend(self.grok.iter().cloned());
        ids.extend(self.kimi.iter().cloned());
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn seed() -> Self {
        Self {
            deepseek: vec![],
            gemini: vec![],
            claude: vec![],
            grok: vec![],
            kimi: vec![],
            compat: vec![],
            updated_at: String::new(),
            errors: Default::default(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            compat_providers: vec![],
            deepseek_api_key: String::new(),
            chatgpt_api_key: String::new(),
            gemini_api_key: String::new(),
            claude_api_key: String::new(),
            grok_api_key: String::new(),
            kimi_api_key: String::new(),
            deepseek_base_url: "https://api.deepseek.com".into(),
            kimi_base_url: "https://api.moonshot.ai".into(),
            default_model: "deepseek-chat".into(),
            create_model: "deepseek-v4-flash".into(),
            generate_model: "deepseek-v4-flash".into(),
            chat_model: "deepseek-v4-flash".into(),
            refine_model: "deepseek-reasoner".into(),
            knowledge_model: "deepseek-v4-flash".into(),
            ui_locale: "system".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub compat_providers: Vec<CompatProviderView>,
    pub gemini_api_key_masked: String,
    pub gemini_api_key_configured: bool,
    pub claude_api_key_masked: String,
    pub claude_api_key_configured: bool,
    pub default_model: String,
    pub create_model: String,
    pub generate_model: String,
    pub chat_model: String,
    pub refine_model: String,
    pub knowledge_model: String,
    pub model_catalog: ModelCatalog,
    pub ui_locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsInput {
    #[serde(default, alias = "compat_providers")]
    pub compat_providers: Option<Vec<SaveCompatProviderInput>>,
    #[serde(default, alias = "gemini_api_key")]
    pub gemini_api_key: Option<String>,
    #[serde(default, alias = "claude_api_key")]
    pub claude_api_key: Option<String>,
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
    #[serde(default, alias = "knowledge_model")]
    pub knowledge_model: Option<String>,
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
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    pub idx: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelProject {
    pub id: String,
    pub title: String,
    pub synopsis: String,
    pub cover_path: Option<String>,
    /// 绑定的公共知识库（设定源）
    pub knowledge_ids: Vec<String>,
    pub knowledge_strategy: String,
    /// `reference`（可参考）| `strict`（严格遵循，禁止发明冲突设定）
    #[serde(default = "default_canon_mode")]
    pub canon_mode: String,
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

fn default_canon_mode() -> String {
    "reference".into()
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
    /// 面板所选模型；空则用 settings.create_model
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelCreateChatResult {
    pub reply: String,
    pub used_mock: bool,
    pub novel: Option<NovelProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExtractChatInput {
    pub messages: Vec<ChatTurn>,
    /// 网址导入 vs 本地文件，影响默认提取侧重点说明
    #[serde(default)]
    pub from_url: bool,
    /// 面板所选模型；空则用 settings.knowledge_model
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeExtractChatResult {
    pub reply: String,
    pub used_mock: bool,
    /// 模型输出 JSON 中的提取需求；无则 null
    pub extract_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Novel,
    Chapter,
    Character,
    SidePlot,
    Knowledge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub outline: String,
    pub character: Option<CharacterCard>,
    /// 知识卡载荷（仅 kind=knowledge）
    #[serde(default)]
    pub knowledge: Option<KnowledgeCardPayload>,
    /// 剧情卡状态（仅 kind=side_plot；缺省=进行中且未吸收）
    #[serde(default)]
    pub side_plot: Option<SidePlotMeta>,
    pub linked_character_ids: Vec<String>,
    pub linked_side_plot_ids: Vec<String>,
    /// 本章/根节点挂载的知识卡 id
    #[serde(default)]
    pub linked_knowledge_ids: Vec<String>,
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

/// 剧情卡元数据：跨章整理用状态；已吸收的章内卡保留回查但不注入生成。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SidePlotMeta {
    /// `active` | `resolved` | `deferred`；空串视为 active
    #[serde(default)]
    pub status: String,
    /// 已被「整理剧情」吸收进根跨章卡
    #[serde(default)]
    pub absorbed: bool,
}

impl SidePlotMeta {
    pub fn active() -> Self {
        Self {
            status: "active".into(),
            absorbed: false,
        }
    }

    pub fn is_injectable(&self) -> bool {
        if self.absorbed {
            return false;
        }
        let s = self.status.trim();
        s.is_empty() || s.eq_ignore_ascii_case("active")
    }
}

#[cfg(test)]
mod side_plot_meta_tests {
    use super::SidePlotMeta;

    #[test]
    fn injectable_rules() {
        assert!(SidePlotMeta::active().is_injectable());
        assert!(SidePlotMeta {
            status: String::new(),
            absorbed: false
        }
        .is_injectable());
        assert!(!SidePlotMeta {
            status: "resolved".into(),
            absorbed: false
        }
        .is_injectable());
        assert!(!SidePlotMeta {
            status: "active".into(),
            absorbed: true
        }
        .is_injectable());
    }
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

/// 树上的知识卡：选中若干知识库 + 提取需求 → AI 写出可参考特征。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeCardPayload {
    #[serde(default)]
    pub book_ids: Vec<String>,
    #[serde(default)]
    pub extract_prompt: String,
    /// AI 提取后的主要特征（写作时注入）
    #[serde(default)]
    pub extracted: String,
    /// 由「同步设定卡」从绑定知识库生成
    #[serde(default)]
    pub from_canon: bool,
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
    /// 空 = 根创作 Chat；非空 = 该卡片 Chat
    #[serde(default)]
    pub node_id: String,
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

/// 左侧「生成章节卡 / 生成下一章」确认框：可编辑的考虑材料 + 期望。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineGenBrief {
    pub brief: String,
    pub from: u32,
    pub to: u32,
}

/// 预生成确认：可选作记忆参考的前序章节。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateMemoryPick {
    pub node_id: String,
    pub label: String,
    pub has_memory: bool,
}

/// 全书记忆面板：按章节分组的记忆条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterMemoryGroup {
    pub node_id: String,
    pub label: String,
    pub items: Vec<String>,
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
    pub day_prompt_tokens: i64,
    pub day_completion_tokens: i64,
    pub day_tokens: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageDay {
    pub date: String,
    pub rows: Vec<TokenUsageHourRow>,
    pub models: Vec<String>,
    pub novels: Vec<TokenUsageNovelRow>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageModelRow {
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageMonth {
    /// YYYY-MM
    pub month: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
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
