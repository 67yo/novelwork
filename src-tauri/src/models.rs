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
    /// MCP Streamable HTTP 端口
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
    /// 是否随应用启动 MCP 服务
    #[serde(default = "default_true")]
    pub mcp_enabled: bool,
    /// 是否允许局域网访问 MCP（监听 0.0.0.0）
    #[serde(default)]
    pub mcp_lan: bool,
}

fn default_mcp_port() -> u16 {
    17832
}

fn default_true() -> bool {
    true
}

/// `compat:{providerId}:{modelId}` — providerId 后只拆第一个 `:`。
pub fn parse_compat_model_ref(model: &str) -> Option<(&str, &str)> {
    let rest = model.trim().strip_prefix("compat:")?;
    let (pid, mid) = rest.split_once(':')?;
    if pid.is_empty() || mid.is_empty() {
        return None;
    }
    Some((pid, mid))
}

/// 发给上游 API 的裸模型名（去掉 compat/gemini/claude 前缀）。
pub fn api_model_id(model: &str) -> &str {
    let m = model.trim();
    if let Some((_, mid)) = parse_compat_model_ref(m) {
        return mid;
    }
    if let Some(mid) = m.strip_prefix("gemini:") {
        if !mid.is_empty() {
            return mid;
        }
    }
    if let Some(mid) = m.strip_prefix("claude:") {
        if !mid.is_empty() {
            return mid;
        }
    }
    m
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
        // 固定提供商：compat:{providerId}:{modelId}
        if let Some((pid, mid)) = parse_compat_model_ref(m) {
            return self.compat_providers.iter().find(|p| {
                p.id == pid && !p.api_key.trim().is_empty() && p.models.iter().any(|x| x == mid)
            }).or_else(|| {
                // 提供商仍在但模型列表已变：仍钉住该 Key
                self.compat_providers
                    .iter()
                    .find(|p| p.id == pid && !p.api_key.trim().is_empty())
            });
        }
        let bare = api_model_id(m);
        if let Some(p) = self.compat_providers.iter().find(|p| {
            !p.api_key.trim().is_empty() && p.models.iter().any(|x| x == bare)
        }) {
            return Some(p);
        }
        self.compat_providers
            .iter()
            .find(|p| !p.api_key.trim().is_empty())
    }
}

#[cfg(test)]
mod model_ref_tests {
    use super::*;

    #[test]
    fn parse_and_api_model_id() {
        assert_eq!(
            parse_compat_model_ref("compat:abc-123:deepseek-chat"),
            Some(("abc-123", "deepseek-chat"))
        );
        assert_eq!(
            parse_compat_model_ref("compat:abc:foo:bar"),
            Some(("abc", "foo:bar"))
        );
        assert_eq!(api_model_id("compat:u1:gpt-4"), "gpt-4");
        assert_eq!(api_model_id("gemini:gemini-pro"), "gemini-pro");
        assert_eq!(api_model_id("claude:claude-3"), "claude-3");
        assert_eq!(api_model_id("deepseek-chat"), "deepseek-chat");
    }

    #[test]
    fn resolve_pins_provider_id() {
        let s = AppSettings {
            compat_providers: vec![
                CompatProvider {
                    id: "a".into(),
                    label: "A".into(),
                    protocol: "openai".into(),
                    base_url: "https://a.example".into(),
                    api_key: "ka".into(),
                    models: vec!["gpt-4".into()],
                },
                CompatProvider {
                    id: "b".into(),
                    label: "B".into(),
                    protocol: "openai".into(),
                    base_url: "https://b.example".into(),
                    api_key: "kb".into(),
                    models: vec!["gpt-4".into()],
                },
            ],
            ..AppSettings::default()
        };
        assert_eq!(s.resolve_compat("compat:b:gpt-4").unwrap().id, "b");
        assert_eq!(s.resolve_compat("gpt-4").unwrap().id, "a");
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
            mcp_port: default_mcp_port(),
            mcp_enabled: true,
            mcp_lan: false,
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
    pub mcp_port: u16,
    pub mcp_enabled: bool,
    pub mcp_lan: bool,
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
    #[serde(default, alias = "mcp_port")]
    pub mcp_port: Option<u16>,
    #[serde(default, alias = "mcp_enabled")]
    pub mcp_enabled: Option<bool>,
    #[serde(default, alias = "mcp_lan")]
    pub mcp_lan: Option<bool>,
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

/// 题材 / 核心玩法 / 风格 / 关系 / 受众（多选；自定义为自由文本）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NovelFeatures {
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub core_play: Vec<String>,
    #[serde(default)]
    pub styles: Vec<String>,
    #[serde(default)]
    pub relationships: Vec<String>,
    #[serde(default)]
    pub audiences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelProject {
    pub id: String,
    pub title: String,
    pub synopsis: String,
    pub cover_path: Option<String>,
    /// 遗留字段：不再作为小说设定源（写作只看树上知识卡）。
    pub knowledge_ids: Vec<String>,
    pub knowledge_strategy: String,
    /// 遗留字段：不再注入写作。
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
    #[serde(default)]
    pub features: NovelFeatures,
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
    #[serde(default)]
    pub features: NovelFeatures,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Novel,
    /// 可选分卷：挂在根下，章节可挂在分卷下
    Volume,
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
    /// 简纲（章节卡创建 / AI 章纲生成）；剧情卡等其它 kind 仍用此字段作要点
    pub outline: String,
    /// 细纲：由简纲进化而来的分条场景要点（仅章节有意义；写作前生成，正文据此扩充）
    #[serde(default)]
    pub detailed_outline: Vec<String>,
    pub character: Option<CharacterCard>,
    /// 知识卡载荷（仅 kind=knowledge）
    #[serde(default)]
    pub knowledge: Option<KnowledgeCardPayload>,
    /// 剧情卡状态（仅 kind=side_plot；缺省=进行中且未吸收）
    #[serde(default)]
    pub side_plot: Option<SidePlotMeta>,
    /// 分卷结构化设定（仅 kind=volume）
    #[serde(default)]
    pub volume: Option<VolumePayload>,
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

/// 分卷：本卷定位 + 三层结构 + 冲突层级 + 关键节点。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VolumePayload {
    /// 本卷定位（一句话）
    #[serde(default)]
    pub positioning: String,
    #[serde(default)]
    pub layer_setup: VolumeLayerBlock,
    #[serde(default)]
    pub layer_confrontation: VolumeLayerBlock,
    #[serde(default)]
    pub layer_resolution: VolumeLayerBlock,
    #[serde(default)]
    pub conflict_external: String,
    #[serde(default)]
    pub conflict_internal: String,
    #[serde(default)]
    pub conflict_deep: String,
    #[serde(default)]
    pub key_beats: Vec<VolumeKeyBeat>,
}

/// 三层结构之一：前置 / 对抗 / 收束。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VolumeLayerBlock {
    /// 对应章节，如「1-15 章」
    #[serde(default)]
    pub chapters: String,
    #[serde(default)]
    pub description: String,
}

/// 关键节点（剧情卡形似）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VolumeKeyBeat {
    /// 排序编号
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub cost: String,
    #[serde(default)]
    pub description: String,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterWorldPosition {
    #[serde(default)]
    pub birth_class: String,
    #[serde(default)]
    pub faction: String,
    #[serde(default)]
    pub social_role: String,
    #[serde(default)]
    pub conflict_stance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterWorldAnchors {
    #[serde(default)]
    pub embodies_law: String,
    #[serde(default)]
    pub embodies_note: String,
    #[serde(default)]
    pub shaped_by_law: String,
    #[serde(default)]
    pub shaped_by_note: String,
    #[serde(default)]
    pub will_challenge: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterRelation {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub relation: String,
    #[serde(default)]
    pub definition: String,
    #[serde(default)]
    pub default_attitude: String,
    #[serde(default)]
    pub hidden_tension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterCoreBelief {
    #[serde(default)]
    pub belief: String,
    /// prove | disprove | unresolved | ""
    #[serde(default)]
    pub author_verdict: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterDeep {
    #[serde(default)]
    pub desire_surface: String,
    #[serde(default)]
    pub desire_deep: String,
    #[serde(default)]
    pub fear: String,
    #[serde(default)]
    pub fear_source: String,
    #[serde(default)]
    pub secret_content: String,
    #[serde(default)]
    pub secret_who_knows: String,
    #[serde(default)]
    pub secret_should_know: String,
    #[serde(default)]
    pub secret_exposure: String,
    #[serde(default)]
    pub line_trigger: String,
    #[serde(default)]
    pub line_source: String,
    #[serde(default)]
    pub line_reaction: String,
    #[serde(default)]
    pub trauma_wound: String,
    #[serde(default)]
    pub trauma_trigger: String,
    #[serde(default)]
    pub trauma_stress: String,
    #[serde(default)]
    pub trauma_imprint: String,
    #[serde(default)]
    pub contradiction_poles: String,
    #[serde(default)]
    pub contradiction_source: String,
    #[serde(default)]
    pub contradiction_trajectory: String,
    #[serde(default)]
    pub arc_growth: String,
    #[serde(default)]
    pub arc_fall: String,
    #[serde(default)]
    pub arc_choice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterVoice {
    #[serde(default)]
    pub positioning: String,
    #[serde(default)]
    pub cognitive_filter: String,
    #[serde(default)]
    pub sentence_length: String,
    #[serde(default)]
    pub pause: String,
    #[serde(default)]
    pub patterns: String,
    #[serde(default)]
    pub catchphrases: Vec<String>,
    #[serde(default)]
    pub emotion_anger: String,
    #[serde(default)]
    pub emotion_tense: String,
    #[serde(default)]
    pub emotion_mask: String,
    #[serde(default)]
    pub emotion_sad: String,
    #[serde(default)]
    pub emotion_happy: String,
    #[serde(default)]
    pub banned: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterCard {
    pub role: String,
    pub personality: String,
    pub motto: String,
    pub gender: String,
    pub style: String,
    pub alignment: String,
    /// 年龄（自由文本，如「28」或「约二十」）
    #[serde(default)]
    pub age: String,
    /// 写该人物时的硬约束（长文本，用户自填；结构化保存后常同步为完整人设正文）
    #[serde(default)]
    pub constraints: String,
    #[serde(default)]
    pub aliases: String,
    #[serde(default)]
    pub world_position: CharacterWorldPosition,
    #[serde(default)]
    pub world_anchors: CharacterWorldAnchors,
    #[serde(default)]
    pub relations: Vec<CharacterRelation>,
    #[serde(default)]
    pub core_belief: CharacterCoreBelief,
    #[serde(default)]
    pub deep: CharacterDeep,
    #[serde(default)]
    pub voice: CharacterVoice,
}

/// 树上的知识卡：勾选公共库后，完整导入或 AI 提炼后写入 `extracted`。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeCardPayload {
    #[serde(default)]
    pub book_ids: Vec<String>,
    #[serde(default)]
    pub extract_prompt: String,
    /// AI 提取后的主要特征（写作时注入）
    #[serde(default)]
    pub extracted: String,
    /// 遗留：旧「同步设定卡」标记；写作不再按此注入公共库。
    #[serde(default)]
    pub from_canon: bool,
    /// 根节点固定槽位（世界观扇形 / 故事规则）；空 = 普通知识卡。
    #[serde(default)]
    pub slot: String,
    /// 核心法则结构化内容（`slot=wv_core_laws`）；写作仍读同步后的 `extracted`。
    #[serde(default)]
    pub core_laws: Option<CoreLawsPayload>,
    /// 时空地理结构化（`slot=wv_spatiotemporal`）；写作仍读同步后的 `extracted`。
    #[serde(default)]
    pub spatiotemporal: Option<SpatiotemporalPayload>,
    /// 世界公理子卡（`slot=wv_axiom`），挂在核心法则上方扇形。
    #[serde(default)]
    pub world_axiom: Option<WorldAxiom>,
    /// 关键地点子卡（`slot=wv_location`），挂在时空地理上方扇形。
    #[serde(default)]
    pub key_location: Option<KeyLocation>,
    /// 社会权力结构化（`slot=wv_social_power`）；写作仍读同步后的 `extracted`。
    #[serde(default)]
    pub social_power: Option<SocialPowerPayload>,
    /// 种族子卡（`slot=wv_race`），挂在社会权力上方扇形。
    #[serde(default)]
    pub world_race: Option<WorldRace>,
    /// 主要势力子卡（`slot=wv_faction`），挂在社会权力上方扇形。
    #[serde(default)]
    pub major_faction: Option<MajorFaction>,
    /// 存在基础结构化（`slot=wv_existence`）；写作仍读同步后的 `extracted`。
    #[serde(default)]
    pub existence: Option<ExistencePayload>,
    /// 信息传播结构化（`slot=wv_info_flow`）；写作仍读同步后的 `extracted`。
    #[serde(default)]
    pub info_flow: Option<InfoFlowPayload>,
    /// 历史文化结构化（`slot=wv_history_culture`）；写作仍读同步后的 `extracted`。
    #[serde(default)]
    pub history_culture: Option<HistoryCulturePayload>,
    /// 宗教子卡（`slot=wv_religion`），挂在历史文化上方扇形。
    #[serde(default)]
    pub world_religion: Option<WorldReligion>,
    /// 重大事件子卡（`slot=wv_major_event`），挂在历史文化上方扇形。
    #[serde(default)]
    pub major_event: Option<MajorEvent>,
    /// 表层设定（`slot=sr_surface_setting`），挂在故事规则右侧扇形。
    #[serde(default)]
    pub surface_setting: Option<SurfaceSettingPayload>,
    /// 故事引擎（`slot=sr_story_engine`）。
    #[serde(default)]
    pub story_engine: Option<StoryEnginePayload>,
    /// 兑现系统（`slot=sr_fulfillment_system`）。
    #[serde(default)]
    pub fulfillment_system: Option<FulfillmentSystemPayload>,
    /// 约束红线（`slot=sr_constraint_redlines`）。
    #[serde(default)]
    pub constraint_redlines: Option<ConstraintRedlinesPayload>,
}

/// 表层设定（故事规则子卡）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SurfaceSettingPayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub core_conflict: String,
    #[serde(default)]
    pub reader_promise: String,
    #[serde(default)]
    pub target_audience: String,
    #[serde(default)]
    pub tone_reference: String,
    #[serde(default)]
    pub commercial_tags: String,
    #[serde(default)]
    pub extended_premise: String,
}

/// 故事引擎（故事规则子卡）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoryEnginePayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub bright_line: String,
    #[serde(default)]
    pub dark_line: String,
    #[serde(default)]
    pub suspense_setup: String,
    #[serde(default)]
    pub conflict_engine: String,
    #[serde(default)]
    pub external_conflict: String,
    #[serde(default)]
    pub internal_conflict: String,
    #[serde(default)]
    pub relational_conflict: String,
    #[serde(default)]
    pub progression_cycle: String,
    #[serde(default)]
    pub protagonist_dilemma: String,
}

/// 兑现系统（故事规则子卡）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FulfillmentSystemPayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub growth_path: String,
    #[serde(default)]
    pub ending_texture: String,
    #[serde(default)]
    pub payoff_syntax: Vec<String>,
    #[serde(default)]
    pub emotional_rhythm: String,
    #[serde(default)]
    pub tension_circles: Vec<String>,
}

/// 约束红线（故事规则子卡）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConstraintRedlinesPayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub redlines: Vec<String>,
}

/// 世界公理一条：名称 + 表述 / 边界 / 代价 / 执行机制。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldAxiom {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub statement: String,
    #[serde(default)]
    pub boundary: String,
    #[serde(default)]
    pub cost: String,
    #[serde(default)]
    pub mechanism: String,
}

/// 核心法则固定项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreLawsPayload {
    /// 一句话立意
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub axioms: Vec<WorldAxiom>,
    /// 禁忌红线（严禁…否则…）
    #[serde(default)]
    pub taboos: Vec<String>,
    #[serde(default)]
    pub power_system: String,
    #[serde(default)]
    pub power_expression: String,
}

/// 时空地理：关键地点。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyLocation {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub features: String,
    #[serde(default)]
    pub terrain: String,
    #[serde(default)]
    pub faction: String,
}

/// 时空地理固定项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpatiotemporalPayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub era: String,
    #[serde(default)]
    pub ecology: String,
    #[serde(default)]
    pub world_pattern: String,
    #[serde(default)]
    pub locations: Vec<KeyLocation>,
    #[serde(default)]
    pub atmosphere: String,
}

/// 社会权力：种族一条。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldRace {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub features: String,
    #[serde(default)]
    pub population: String,
    #[serde(default)]
    pub social_status: String,
}

/// 社会权力：主要势力一条。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MajorFaction {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub faction_type: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub means: String,
    #[serde(default)]
    pub power_base: String,
}

/// 存在基础固定项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExistencePayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub death: String,
    #[serde(default)]
    pub calendar: String,
    #[serde(default)]
    pub lifespan: String,
    #[serde(default)]
    pub disease_reproduction: String,
}

/// 信息传播固定项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InfoFlowPayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub info_speed: String,
    #[serde(default)]
    pub info_barrier: String,
    #[serde(default)]
    pub message_truth: String,
    #[serde(default)]
    pub knowledge_carrier: String,
}

/// 历史文化：宗教一条。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldReligion {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub core_belief: String,
    #[serde(default)]
    pub followers_scope: String,
}

/// 历史文化：重大事件一条。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MajorEvent {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub long_term_impact: String,
}

/// 历史文化固定项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryCulturePayload {
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub customs: String,
    #[serde(default)]
    pub economy: String,
    /// 兼容旧内嵌；迁移后清空。
    #[serde(default)]
    pub religions: Vec<WorldReligion>,
    #[serde(default)]
    pub daily_slices: String,
    #[serde(default)]
    pub major_events: Vec<MajorEvent>,
}

/// 社会权力固定项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SocialPowerPayload {
    #[serde(default)]
    pub premise: String,
    /// 兼容旧内嵌数据；迁移后清空。
    #[serde(default)]
    pub races: Vec<WorldRace>,
    #[serde(default)]
    pub factions: Vec<MajorFaction>,
    #[serde(default)]
    pub class_structure: String,
    #[serde(default)]
    pub political_system: String,
    #[serde(default)]
    pub power_visibility: String,
}

/// 跨小说共用的知识卡目录（无版本号；挂到树上时复制一份）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKnowledgeCard {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub book_ids: Vec<String>,
    #[serde(default)]
    pub extract_prompt: String,
    #[serde(default)]
    pub extracted: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub archived: bool,
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
