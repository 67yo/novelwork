//! LLM prompts: language follows user input when detectable, else UI/system locale.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptLocale {
    ZhCn,
    ZhTw,
    En,
    Ja,
    De,
    Fr,
}

impl PromptLocale {
    pub fn from_preference(pref: &str) -> Self {
        let p = pref.trim();
        if p.is_empty() || p.eq_ignore_ascii_case("system") {
            return Self::from_system();
        }
        Self::parse(p)
    }

    /// 交互语言：优先根据用户输入脚本识别；识别不出时回退 UI/系统语言。
    /// 避免「界面是英文、用户打中文，却整篇回英文」。
    pub fn for_interaction(ui_pref: &str, samples: &[&str]) -> Self {
        let fallback = Self::from_preference(ui_pref);
        let joined = samples.iter().copied().collect::<Vec<_>>().join("\n");
        match Self::detect_from_text(&joined) {
            Some(Self::ZhCn) if fallback == Self::ZhTw => Self::ZhTw,
            // 拉丁字母难分德/法/英：沿用界面语言
            Some(Self::En) if matches!(fallback, Self::De | Self::Fr | Self::En) => fallback,
            Some(loc) => loc,
            None => fallback,
        }
    }

    /// 根据字符脚本粗检语言。太短或混杂时返回 None。
    pub fn detect_from_text(text: &str) -> Option<Self> {
        let mut han = 0u32;
        let mut hira = 0u32;
        let mut kata = 0u32;
        let mut latin = 0u32;
        for c in text.chars() {
            match c {
                '\u{3040}'..='\u{309F}' => hira += 1,
                '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' => kata += 1,
                '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' => han += 1,
                c if c.is_ascii_alphabetic() => latin += 1,
                _ => {}
            }
        }
        let kana = hira + kata;
        let scored = han + kana + latin;
        if scored < 4 {
            return None;
        }
        // 日文：假名是强信号
        if kana >= 3 && kana.saturating_mul(2) >= han {
            return Some(Self::Ja);
        }
        // 中文：汉字为主且不少于拉丁
        if han >= 4 && han >= latin {
            return Some(Self::ZhCn);
        }
        // 拉丁文为主 → 英（德法由 for_interaction 回退 UI）
        if latin >= 8 && latin > han.saturating_mul(2) {
            return Some(Self::En);
        }
        None
    }

    fn parse(code: &str) -> Self {
        let lower = code.to_ascii_lowercase().replace('_', "-");
        if lower.starts_with("zh-tw")
            || lower.starts_with("zh-hk")
            || lower.starts_with("zh-hant")
            || lower == "zh-mo"
        {
            return Self::ZhTw;
        }
        if lower.starts_with("zh") {
            return Self::ZhCn;
        }
        if lower.starts_with("ja") {
            return Self::Ja;
        }
        if lower.starts_with("de") {
            return Self::De;
        }
        if lower.starts_with("fr") {
            return Self::Fr;
        }
        Self::En
    }

    fn from_system() -> Self {
        let raw = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        // e.g. zh_TW.UTF-8 / en_US.UTF-8
        let code = raw.split('.').next().unwrap_or("en").replace('_', "-");
        Self::parse(&code)
    }

    pub fn is_zh(self) -> bool {
        matches!(self, Self::ZhCn | Self::ZhTw)
    }

    /// Natural-language name of the required output language.
    pub fn output_language(self) -> &'static str {
        match self {
            Self::ZhCn => "Simplified Chinese (简体中文)",
            Self::ZhTw => "Traditional Chinese (繁體中文)",
            Self::En => "English",
            Self::Ja => "Japanese (日本語)",
            Self::De => "German (Deutsch)",
            Self::Fr => "French (Français)",
        }
    }

    /// Appended to every system prompt.
    pub fn language_rule(self) -> String {
        if self.is_zh() {
            let which = if self == Self::ZhTw {
                "繁體中文"
            } else {
                "简体中文"
            };
            format!(
                "语言规则（最高优先级）：根据用户输入识别，本次须使用{which}撰写所有自然语言回复与生成正文。\
                 即使系统/界面语言不同，也不得擅自改成英文或其他语言；仅当用户明确要求换语言时才切换。\
                 JSON 键名保持英文；JSON 字符串值使用{which}。"
            )
        } else {
            format!(
                "Language rule (highest priority): Based on the user's input, write ALL natural-language replies \
                 and generated prose in {}. Even if the app UI uses another language, do NOT switch away \
                 unless the user explicitly asks. Keep JSON keys in English; JSON string values in {}.",
                self.output_language(),
                self.output_language()
            )
        }
    }
}

fn split_genre_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| "、,，/;；|".contains(c))
        .map(|p| p.trim().trim_matches(|c: char| c == '*' || c == '`' || c == '·').to_string())
        .filter(|p| !p.is_empty() && p.chars().count() <= 12)
        .take(8)
        .collect()
}

/// Merge user / auto tags; reuse known spellings; append unknowns (capped).
pub fn merge_genre_tags(base: Vec<String>, extra: Vec<String>, known: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in base.into_iter().chain(extra) {
        for part in split_genre_tokens(&g) {
            let canon = known
                .iter()
                .find(|k| k.as_str() == part || k.eq_ignore_ascii_case(&part))
                .cloned()
                .unwrap_or(part);
            if !out.iter().any(|x| x == &canon) {
                out.push(canon);
            }
        }
    }
    out.truncate(8);
    out
}

#[cfg(test)]
mod genre_tests {
    use super::*;


    #[test]
    fn merge_reuses_known_and_appends_new() {
        let known = vec!["玄幻".into(), "都市".into()];
        let out = merge_genre_tags(
            vec!["玄幻".into()],
            vec!["赛博朋克".into(), "XuanHuan".into()],
            &known,
        );
        // XuanHuan won't match 玄幻; 赛博朋克 is new
        assert!(out.contains(&"玄幻".to_string()));
        assert!(out.contains(&"赛博朋克".to_string()));
    }
}

pub fn create_novel_system(loc: PromptLocale, force: bool) -> String {
    let force_hint = if force {
        if loc.is_zh() {
            "\n用户已要求立即创建。你必须在回复中输出一行 create JSON，不要再追问。"
        } else {
            "\nThe user demanded immediate creation. You MUST output one create JSON line; do not ask more questions."
        }
    } else {
        ""
    };
    let body = if loc.is_zh() {
        format!(
            "你是 Novel Work 开书助手。通过简短对话帮用户确定：书名、简介、主要角色、每章目标字数范围、全书计划总章数。\n\
             必须询问：每章大约多少字（如 2000–3000），以及这本小说大概写多少章（如 20、30）。不要现在生成章节大纲或章节列表。\n\
             信息不足时用一两句追问。信息足够，或用户说「创建/生成/开始」时，先用一两句确认，\
             然后输出完整 JSON（可多行或代码块）：\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"chapter_count\":20,\"characters\":[{{\"label\":\"\",\"role\":\"主角\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"正派\"}}]}}}}\n\
             characters 至少主角；word_count_min/max 为每章目标字数；chapter_count 为全书计划章数。\n\
             若用户消息附带【网页正文】，以正文为准作答，勿臆造页面未提供的内容。{force_hint}"
        )
    } else {
        format!(
            "You are the Novel Work novel-creation assistant. Through short dialogue, settle: title, synopsis, main characters, target words per chapter, and planned total chapter count.\n\
             Always ask for approximate words per chapter (e.g. 2000–3000) and how many chapters the novel will have (e.g. 20, 30). Do NOT generate chapter outlines or chapter lists yet.\n\
             If info is missing, ask one or two brief follow-ups. When enough—or the user says create/generate/start—confirm in one or two sentences, \
             then output complete JSON (multi-line or fenced OK):\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"chapter_count\":20,\"characters\":[{{\"label\":\"\",\"role\":\"protagonist\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}}}\n\
             Include at least the protagonist; word_count_min/max are per-chapter targets; chapter_count is the planned total chapters.\n\
             If the user message includes 【网页正文】/fetched page text, rely on it; do not invent page content.{force_hint}"
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn create_novel_empty_user(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "你好，我想开一本新小说。"
    } else {
        "Hi, I want to start a new novel."
    }
}

pub fn untitled_novel(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "未命名小说"
    } else {
        "Untitled novel"
    }
}

pub fn chat_created_novel(loc: PromptLocale) -> (&'static str, &'static str, &'static str) {
    if loc.is_zh() {
        (
            "对话生成的小说",
            "由开书对话生成的初稿设定。",
            "按对话约束展开。",
        )
    } else {
        (
            "Novel from chat",
            "Draft settings from the create-novel chat.",
            "Expand under the chat constraints.",
        )
    }
}

pub struct ChapterContextLabels {
    pub name: &'static str,
    pub role: &'static str,
    pub gender: &'static str,
    pub age: &'static str,
    pub alignment: &'static str,
    pub personality: &'static str,
    pub style: &'static str,
    pub motto: &'static str,
    pub constraints: &'static str,
    pub card_incomplete: &'static str,
    pub no_characters: &'static str,
    pub empty_plot: &'static str,
    pub no_plots: &'static str,
    pub empty_outline: &'static str,
    pub chapter_title: &'static str,
    pub chapter_outline: &'static str,
    pub chapter_detailed_outline: &'static str,
    pub empty_detailed_outline: &'static str,
    pub chars_header: &'static str,
    pub relations_header: &'static str,
    pub relation_unset: &'static str,
    pub plots_header: &'static str,
    pub knowledge_header: &'static str,
    pub no_knowledge: &'static str,
    pub empty_knowledge: &'static str,
}

pub fn chapter_context_labels(loc: PromptLocale) -> ChapterContextLabels {
    if loc.is_zh() {
        ChapterContextLabels {
            name: "姓名",
            role: "身份",
            gender: "性别",
            age: "年龄",
            alignment: "阵营",
            personality: "性格",
            style: "行事风格",
            motto: "座右铭",
            constraints: "约束（写该人物必须遵守）",
            card_incomplete: "（卡面未补全）",
            no_characters: "（本章未链接人物卡）\n",
            empty_plot: "（剧情卡大纲为空）",
            no_plots: "（本章未链接剧情卡）\n",
            empty_outline:
                "（本章简纲为空，请仅依据已链接人物/剧情与小说简介合理推进，勿偏离已有设定）",
            chapter_title: "章节标题",
            chapter_outline: "本章简纲（主线走向，不可丢关键节拍）",
            chapter_detailed_outline: "本章细纲（分条场景要点——正文须据此扩充落地）",
            empty_detailed_outline: "（细纲为空：先由简纲进化细纲，再写正文）",
            chars_header: "【链接人物卡（含根节点贯穿全书人物）——言行必须符合；人物关系见下】",
            relations_header: "【人物关系——互动与称呼须符合；含根节点人物之间及与本章人物的关系】",
            relation_unset: "（关系未标注）",
            plots_header: "【链接剧情卡——关键情节点必须在本章正文中实际发生或明确推进，不可只提一句带过】",
            knowledge_header: "【本章知识卡——写作硬约束（知识储备）：手法/节奏、文风语气、用词与禁用词、关键词与专名替换、视角时态、对话风格、修辞氛围、信息密度等；生成正文必须严格遵守，优先 extracted，否则 extract_prompt/outline；禁止另起风格】",
            no_knowledge: "（本章未链接知识卡）\n",
            empty_knowledge: "（尚未提取特征，请仅按提取需求/卡片说明约束）",
        }
    } else {
        ChapterContextLabels {
            name: "Name",
            role: "Role",
            gender: "Gender",
            age: "Age",
            alignment: "Alignment",
            personality: "Personality",
            style: "Style",
            motto: "Motto",
            constraints: "Constraints (must follow when writing this character)",
            card_incomplete: "(card incomplete)",
            no_characters: "(no character cards linked)\n",
            empty_plot: "(plot outline empty)",
            no_plots: "(no plot cards linked)\n",
            empty_outline:
                "(brief outline empty — advance only from linked characters/plots and the synopsis; do not invent conflicting lore)",
            chapter_title: "Chapter title",
            chapter_outline: "Brief outline (main arc — must follow)",
            chapter_detailed_outline: "Detailed outline (scene beats — expand body from these)",
            empty_detailed_outline: "(detailed outline empty — evolve from brief outline before writing body)",
            chars_header: "[Linked character cards (incl. root book-wide cast) — speech/actions must match; see relations below]",
            relations_header: "[Character relations — incl. among root cast and with chapter characters]",
            relation_unset: "(relation not set)",
            plots_header: "[Linked plot cards — key beats MUST occur or clearly advance in this chapter body; do not name-drop only]",
            knowledge_header: "[Chapter knowledge cards — HARD craft constraints (technique/pacing, style/tone, diction/banned words, keyword & proper-noun substitution, POV/tense, dialogue, rhetoric/mood, density, etc.); follow extracted first, else extract_prompt/outline; do not invent another style]",
            no_knowledge: "(no knowledge cards linked)\n",
            empty_knowledge: "(features not extracted yet — honor extract request / card notes only)",
        }
    }
}

pub fn generate_chapter_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    wmin: u32,
    wmax: u32,
    chapter_count: u32,
) -> String {
    let wmin_lo = wmin.saturating_sub(60);
    let wmax_hi = wmax.saturating_add(60);
    let conflict = if loc.is_zh() {
        "未链接到本章的设定不要硬塞；冲突时：前序记忆与大纲定历史事实 → 本章细纲定场景落地 → 本章简纲定主线 → 剧情卡补齐本章事件 → 人物约束行为 → **本章知识卡硬约束写作手法/文风/用词** → 简介定调。"
    } else {
        "Do not force unlinked lore. On conflicts: prior memory/outlines = history → detailed outline drives scene landings → brief outline drives the arc → plot cards supply events → characters constrain behavior → **chapter knowledge cards hard-constrain craft/style/diction** → synopsis sets tone."
    };
    let body = if loc.is_zh() {
        format!(
            "你是强约束小说写作引擎 Novel Work。生成本章时必须同时遵守：\n\
             1）全书故事简介与根节点关联人物卡（总体设定参考，勿偏离）；\n\
             2）前序章节的大纲与章节记忆（若有）：已是既定事实与因果，本章必须衔接，严禁推翻、改写或无视记忆中的人物状态/承诺/事件结果；\n\
             3）本章简纲（主线走向）与本章细纲（分条场景要点）：**正文从细纲扩充**，简纲保主线；细纲每条须在正文中实际发生或明确推进；\n\
             4）本章已链接人物卡（性格、身份、行事风格、关系，言行不得出戏）；\n\
             5）本章已链接剧情卡（含根节点贯穿剧情）：卡内要点/补充正文是本章情节来源之一，须结合大纲写入正文，使事件落地，禁止只点名不推进。\n\
             6）本章已链接知识卡（写作硬约束/知识储备：手法节奏、文风语气、用词与禁用词、关键词与专名替换、视角时态、对话与修辞等；须严格遵守 extracted，空则遵守 extract_prompt/outline；禁止另起风格）。\n\
             7）全书计划共 {chapter_count} 章：本章信息量与悬念投放须符合所处位置，勿按无限连载节奏注水；勿抢写后续章大纲中才应发生的高潮。\n\
             {conflict}\n\
             【契约】用户消息中的「必须落地」列表是硬性验收项：每一项都须在正文中实际发生或明确推进；写完在脑中逐项勾选，缺项则补写后再输出。\n\
             【预生成禁令】**禁止参考、续写或改写本章已有正文/旧稿**（即使磁盘上已有正文也不注入）；须按细纲、链接卡与前序记忆**重新写出**完整本章。\n\
             小说：《{title}》\n简介：{synopsis}\n\
             【硬性篇幅】以树根节点每章目标为准：正文非空白字符数目标 {wmin}–{wmax} 字，允许上下浮动 60 字\
             （有效区间 {wmin_lo}–{wmax_hi}）。上下限相同时按该单点目标 ±60。\
             **禁止低于 {wmin_lo}，禁止超过 {wmax_hi}**。写完自行点数，偏短续写、偏长删冗，落在有效区间内再交卷。"
        )
    } else {
        format!(
            "You are Novel Work, a strongly constrained novel-writing engine. When generating this chapter you MUST obey:\n\
             1) Novel synopsis and root-linked character cards (global setting — do not contradict);\n\
             2) Prior chapter outlines + chapter memory (if any): established facts/causality—this chapter must continue them; never overturn, rewrite, or ignore remembered states/promises/outcomes;\n\
             3) Brief outline (arc) + detailed outline (scene beats): **expand body from the detailed outline**; every detailed beat must land; brief keeps the arc;\n\
             4) Chapter-linked character cards (traits, role, style — stay in character);\n\
             5) Linked plot cards (incl. root book-wide plots): their beats/extra text are chapter plot sources—combine with the outline and make events land in the body; do not name-drop without advancing them.\n\
             6) Chapter-linked knowledge cards (HARD craft constraints: technique/pacing, style/tone, diction/banned words, keyword & proper-noun substitution, POV/tense, dialogue/rhetoric, etc.—follow extracted; else extract_prompt/outline; do not invent another style).\n\
             7) Planned total: {chapter_count} chapters—pace for this place in the arc; do not steal climaxes reserved for later chapter outlines.\n\
             {conflict}\n\
             [Contract] The “Must land” list in the user message is hard acceptance criteria—each item must occur or clearly advance; mentally check every item and fill gaps before outputting.\n\
             [Generate ban] **Do not reference, continue, or revise any existing body/draft of THIS chapter** (none is injected even if on disk); write a full new chapter from detailed outline, linked cards, and prior memory only.\n\
             Novel: “{title}”\nSynopsis: {synopsis}\n\
             [Hard length] Root per-chapter target: {wmin}–{wmax} non-whitespace chars, ±60 float allowed \
             (valid band {wmin_lo}–{wmax_hi}). If min=max, that single target ±60. \
             **Must not go below {wmin_lo} or above {wmax_hi}.** Count before submit; expand if short, trim if long."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn generate_chapter_user(
    loc: PromptLocale,
    root_ref: &str,
    prior_hist: &str,
    chapter_info: &str,
    cards: &str,
    contract: &str,
    user_brief: &str,
    wmin: u32,
    wmax: u32,
) -> String {
    let hist = if prior_hist.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!(
            "——— 前序章节大纲 + 章节记忆（既定事实，必须严格衔接，禁止推翻）———\n{prior_hist}\n\n"
        )
    } else {
        format!(
            "——— Prior outlines + chapter memory (established facts — continue strictly, do not overturn) ———\n{prior_hist}\n\n"
        )
    };
    let brief = if user_brief.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("——— 预生成条件（用户编辑，须遵守）———\n{user_brief}\n\n")
    } else {
        format!("——— Generate conditions (user-edited — follow) ———\n{user_brief}\n\n")
    };
    let wmin_lo = wmin.saturating_sub(60);
    let wmax_hi = wmax.saturating_add(60);
    if loc.is_zh() {
        format!(
            "{root_ref}\n\n{brief}{hist}{chapter_info}\n\n{cards}\n\n{contract}\n\
             请在衔接前序记忆/大纲与预生成条件的前提下，严格按「必须落地」列表**重新生成本章正文**（Markdown）。\n\
             **禁止参考本章已有正文**；勿续写或改写旧稿。\n\
             篇幅硬约束：目标 {wmin}–{wmax}，允许 ±60（有效 {wmin_lo}–{wmax_hi}）；**禁止低于 {wmin_lo} 或超过 {wmax_hi}**。\n\
             只输出正文，不要输出清单或自我评分。缺项或字数越界禁止交卷。"
        )
    } else {
        format!(
            "{root_ref}\n\n{brief}{hist}{chapter_info}\n\n{cards}\n\n{contract}\n\
             Continue prior memory/outlines and generate conditions, then **write this chapter’s body from scratch** (Markdown) covering every Must-land item.\n\
             **Do not use any existing body of this chapter**; do not continue or revise an old draft.\n\
             Hard length: target {wmin}–{wmax}, ±60 (band {wmin_lo}–{wmax_hi}); **must not go below {wmin_lo} or above {wmax_hi}**.\n\
             Output body only — no checklist or self-score. Do not submit with gaps or out-of-band length."
        )
    }
}

pub fn repair_chapter_system(loc: PromptLocale) -> String {
    // 落地补写不做字数验收：字数由首轮预生成/精修 Prompt 交给 AI；此处为凑字数重写全文会浪费整章。
    let body = if loc.is_zh() {
        "你是 Novel Work 约束补写引擎。任务：在不大改已有情节骨架的前提下，把「未落地项」自然织入当前章正文。\n\
         禁止另起炉灶；禁止删除已有合理段落；可增补场景/对白/过渡使缺项发生。\n\
         **不做字数校验**：勿为凑字数删改或重写全文；篇幅随补写略增即可。只输出完整正文 Markdown。"
    } else {
        "You are Novel Work’s constraint-repair engine. Weave every Missing item into the current chapter without overhauling the plot skeleton.\n\
         Do not restart the chapter; do not delete sound existing passages; add scenes/dialogue/transitions so gaps land.\n\
         **No length check**: do not rewrite the whole chapter to hit a word count; slight growth from patches is fine. Output full chapter Markdown only."
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn repair_chapter_user(
    loc: PromptLocale,
    missing: &str,
    current: &str,
) -> String {
    if loc.is_zh() {
        format!(
            "{missing}\n——— 当前正文 ———\n{current}\n\n请输出补写后的完整本章正文。"
        )
    } else {
        format!("{missing}\n——— Current body ———\n{current}\n\nOutput the full repaired chapter body.")
    }
}

pub fn generate_footer(loc: PromptLocale, node_id: &str, model: &str) -> String {
    if loc.is_zh() {
        format!(
             "\n\n---\n> 节点约束校验摘要：已按大纲、人物卡、剧情卡与知识卡生成。章节节点 `{node_id}`。模型：{model}。\n"
        )
    } else {
        format!(
            "\n\n---\n> Constraint check: generated from outline, character, plot, and knowledge cards. Node `{node_id}`. Model: {model}.\n"
        )
    }
}

pub fn word_count_off_note(loc: PromptLocale, words: u32, wmin: u32, wmax: u32) -> String {
    let lo = wmin.saturating_sub(60);
    let hi = wmax.saturating_add(60);
    if loc.is_zh() {
        format!(" ⚠字数 {words} 越出有效区间 {lo}–{hi}（目标 {wmin}–{wmax}，±60）。")
    } else {
        format!(" ⚠ Length {words} outside band {lo}–{hi} (target {wmin}–{wmax}, ±60).")
    }
}

pub fn generate_done_msg(
    loc: PromptLocale,
    model: &str,
    words: u32,
    mock: bool,
    repaired: usize,
) -> String {
    if loc.is_zh() {
        if mock {
            format!("未配置 DeepSeek API Key，已使用 Mock。字数 {words}。")
        } else if repaired > 0 {
            format!("预生成完成（{model}），字数 {words}；已按约束补写未落地项 {repaired} 条。")
        } else {
            format!("预生成完成（{model}），字数 {words}。")
        }
    } else if mock {
        format!("No DeepSeek API key — used mock. Word count {words}.")
    } else if repaired > 0 {
        format!("Pre-generate done ({model}), word count {words}; repaired {repaired} missing constraint(s).")
    } else {
        format!("Pre-generate done ({model}), word count {words}.")
    }
}

pub fn refine_chapter_system(
    loc: PromptLocale,
    title: &str,
    linked_n: u32,
    wmin: u32,
    wmax: u32,
    full_body: bool,
) -> String {
    let wmin_lo = wmin.saturating_sub(60);
    let wmax_hi = wmax.saturating_add(60);
    let body = if loc.is_zh() {
        if full_body {
            format!(
                "你是 Novel Work 章节精修引擎。小说：《{title}》。\n\
                 【精修目的】不大改剧情：在已生成正文的情节骨架上做小幅梳理与润色；必须结合「参考前 N 章」的完整正文来核对历史因果，再梳理当前章，使衔接合理、人物关系正确、结构正常、语句通顺；禁止另起炉灶或大幅改写主线。\n\
                 【必须同时使用的材料】\n\
                 1）参考前 {linked_n} 章的完整正文（标题+大纲+正文；只作历史依据，禁止改写这些历史章）；\n\
                 2）当前章全部链接卡片（大纲、人物卡、剧情卡、知识卡、人物关系等；剧情卡要点应已在正文中落地，缺漏则小幅补写）；\n\
                 3）当前章已生成的完整正文（**唯一精修底稿**：必须在此原文上修改，保留情节骨架与段落顺序；禁止无视原文另起炉灶）。\n\
                 【必须确保】输出是对③的修订版而非重写；与历史章节衔接合理；链接剧情卡关键情节不缺失；无异常/突兀情节；无剧情错误与时间线矛盾；人物关系与卡面一致；结构清楚；语句通顺。\n\
                 【硬性篇幅】以树根节点每章目标为准：目标 {wmin}–{wmax}，允许上下浮动 60 字（有效 {wmin_lo}–{wmax_hi}）。\
                 **禁止低于 {wmin_lo}，禁止超过 {wmax_hi}**。精修后必须仍在有效区间内。"
            )
        } else {
            format!(
                "你是 Novel Work 章节精修引擎。小说：《{title}》。\n\
                 【精修目的】不大改剧情：在已生成正文的情节骨架上做小幅梳理与润色；必须结合「本小说章节记忆库」中与前 {linked_n} 章相关的核心事实，以及各章大纲，来核对历史因果，再梳理当前章，使衔接合理、人物关系正确、结构正常、语句通顺；禁止另起炉灶或大幅改写主线。\n\
                 【必须同时使用的材料】\n\
                 1）前 {linked_n} 章的大纲 + 章节记忆要点（人物出场、行为、目标、承诺、人设；只作历史依据，禁止改写历史章）；\n\
                 2）当前章全部链接卡片（大纲、人物卡、剧情卡、知识卡、人物关系等；剧情卡要点应已在正文中落地，缺漏则小幅补写）；\n\
                 3）当前章已生成的完整正文（**唯一精修底稿**：必须在此原文上修改，保留情节骨架与段落顺序；禁止无视原文另起炉灶）。\n\
                 【必须确保】输出是对③的修订版而非重写；与上述历史记忆/大纲衔接合理；链接剧情卡关键情节不缺失；无异常/突兀情节；无剧情错误与时间线矛盾；人物关系与卡面一致；结构清楚；语句通顺。\n\
                 【硬性篇幅】以树根节点每章目标为准：目标 {wmin}–{wmax}，允许上下浮动 60 字（有效 {wmin_lo}–{wmax_hi}）。\
                 **禁止低于 {wmin_lo}，禁止超过 {wmax_hi}**。精修后必须仍在有效区间内。"
            )
        }
    } else if full_body {
        format!(
            "You are the Novel Work chapter refine engine. Novel: “{title}”.\n\
             [Purpose] Do not majorly rewrite the plot. Use the full text of the prior {linked_n} reference chapters to check continuity, then lightly tidy the current chapter. Do not restart or overhaul the main arc.\n\
             [Required inputs]\n\
             1) The previous {linked_n} chapters in FULL (title + outline + body — history only; never rewrite them);\n\
             2) All cards linked to the current chapter (outline, characters, plot cards, knowledge, relations; plot-card beats should already land—lightly add if missing);\n\
             3) The full already-generated current-chapter body (**the only base text**: you MUST edit this draft in place; keep beats and paragraph order; never ignore it and rewrite from scratch).\n\
             [Must ensure] Output is a revision of (3), not a rewrite; aligns with history; linked plot-card beats present; no absurd/abrupt beats; no plot/timeline errors; relationships match cards; clear structure; fluent prose.\n\
             [Hard length] Root target {wmin}–{wmax} non-whitespace, ±60 (band {wmin_lo}–{wmax_hi}). \
             **Must not go below {wmin_lo} or above {wmax_hi}.** Refined body MUST stay in band."
        )
    } else {
        format!(
            "You are the Novel Work chapter refine engine. Novel: “{title}”.\n\
             [Purpose] Do not majorly rewrite the plot. Use this novel’s chapter memory (facts from the prior {linked_n} chapters) plus chapter outlines to check continuity, then lightly tidy the current chapter. Do not restart or overhaul the main arc.\n\
             [Required inputs]\n\
             1) Outlines + chapter-memory facts for the previous {linked_n} chapters (characters present, actions, goals, promises, personas — history only; never rewrite those chapters);\n\
             2) All cards linked to the current chapter (outline, characters, plot cards, knowledge, relations; plot-card beats should already land—lightly add if missing);\n\
             3) The full already-generated current-chapter body (**the only base text**: you MUST edit this draft in place; keep beats and paragraph order; never ignore it and rewrite from scratch).\n\
             [Must ensure] Output is a revision of (3), not a rewrite; aligns with memory/outlines; linked plot-card beats present; no absurd/abrupt beats; no plot/timeline errors; relationships match cards; clear structure; fluent prose.\n\
             [Hard length] Root target {wmin}–{wmax} non-whitespace, ±60 (band {wmin_lo}–{wmax_hi}). \
             **Must not go below {wmin_lo} or above {wmax_hi}.** Refined body MUST stay in band."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn refine_chapter_user(
    loc: PromptLocale,
    linked_n: u32,
    prev_text: &str,
    chapter_info: &str,
    cards: &str,
    current: &str,
    full_body: bool,
    user_brief: &str,
    wmin: u32,
    wmax: u32,
) -> String {
    let hist = if prev_text.trim().is_empty() {
        if loc.is_zh() {
            if full_body {
                "（未导入历史章节：prev_n=0 或无前序章）\n"
            } else {
                "（无前序章记忆/大纲：prev_n=0 或尚无前序章）\n"
            }
        } else if full_body {
            "(no prior chapters imported: prev_n=0 or none exist)\n"
        } else {
            "(no prior chapter memory/outlines: prev_n=0 or none exist)\n"
        }
    } else {
        prev_text
    };
    let brief = if user_brief.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("——— 精修条件（用户编辑，须遵守；仍不大改剧情）———\n{user_brief}\n\n")
    } else {
        format!("——— Refine conditions (user-edited — follow; still no plot overhaul) ———\n{user_brief}\n\n")
    };
    let wmin_lo = wmin.saturating_sub(60);
    let wmax_hi = wmax.saturating_add(60);
    if loc.is_zh() {
        if full_body {
            format!(
                "{brief}\
                 ——— ① 参考前 {linked_n} 章完整内容（全文；勿改写历史）———\n{hist}\n\
                 ——— ② 当前章链接设定（大纲 + 全部链接卡片）———\n{chapter_info}\n\n{cards}\n\n\
                 ——— ③ 当前章已生成正文（**底稿：必须在此基础上修改**；对照①梳理，不大改剧情）———\n{current}\n\n\
                 请先通读①中各章完整正文，再**以③原文为底稿**对照②中的剧情卡、精修条件做修订，输出精修后的完整当前章 Markdown（不要输出历史章）。\n\
                 **禁止丢弃③另起炉灶**；改动应克制；若剧情卡要点未落地可小幅补写。\n\
                 【硬性篇幅】目标 {wmin}–{wmax}，允许 ±60（有效 {wmin_lo}–{wmax_hi}）；**禁止低于 {wmin_lo} 或超过 {wmax_hi}**；偏短续写、偏长删冗，落在有效区间再交卷。\n\
                 文末用列表列出本次修正点，按类归并：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。"
            )
        } else {
            format!(
                "{brief}\
                 ——— ① 前 {linked_n} 章大纲 + 章节记忆（核对历史；勿改写历史）———\n{hist}\n\
                 ——— ② 当前章链接设定（大纲 + 全部链接卡片）———\n{chapter_info}\n\n{cards}\n\n\
                 ——— ③ 当前章已生成正文（**底稿：必须在此基础上修改**；对照①梳理，不大改剧情）———\n{current}\n\n\
                 请先依据①中的大纲与记忆要点核对因果与人设，再**以③原文为底稿**对照②中的剧情卡、精修条件做修订，输出精修后的完整当前章 Markdown（不要输出历史章）。\n\
                 **禁止丢弃③另起炉灶**；改动应克制；若剧情卡要点未落地可小幅补写。\n\
                 【硬性篇幅】目标 {wmin}–{wmax}，允许 ±60（有效 {wmin_lo}–{wmax_hi}）；**禁止低于 {wmin_lo} 或超过 {wmax_hi}**；偏短续写、偏长删冗，落在有效区间再交卷。\n\
                 文末用列表列出本次修正点，按类归并：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。"
            )
        }
    } else if full_body {
        format!(
            "{brief}\
             ——— ① Prior {linked_n} chapters in FULL (history only; do not rewrite) ———\n{hist}\n\
             ——— ② Current chapter linked setup (outline + all linked cards) ———\n{chapter_info}\n\n{cards}\n\n\
             ——— ③ Current chapter body (**base draft — edit this in place**; refine against ①; no plot overhaul) ———\n{current}\n\n\
             Read every prior chapter body in ① first, then **revise ③ in place** against ①, ② (incl. plot cards), and refine conditions. Output only the refined current chapter in Markdown.\n\
             **Do not discard ③ and rewrite from scratch.** Lightly add missing plot-card beats if needed.\n\
             [Hard length] target {wmin}–{wmax}, ±60 (band {wmin_lo}–{wmax_hi}); **must not go below {wmin_lo} or above {wmax_hi}**; expand if short, trim if long.\n\
             End with a bullet list of fixes by category: continuity / plot-card landing / plot errors / relationships / structure / prose / length."
        )
    } else {
        format!(
            "{brief}\
             ——— ① Prior {linked_n} chapters: outlines + chapter memory (history only; do not rewrite) ———\n{hist}\n\
             ——— ② Current chapter linked setup (outline + all linked cards) ———\n{chapter_info}\n\n{cards}\n\n\
             ——— ③ Current chapter body (**base draft — edit this in place**; refine against ①; no plot overhaul) ———\n{current}\n\n\
             Use outlines and memory in ①, then **revise ③ in place** against ①, ② (incl. plot cards), and refine conditions. Output only the refined current chapter in Markdown.\n\
             **Do not discard ③ and rewrite from scratch.** Lightly add missing plot-card beats if needed.\n\
             [Hard length] target {wmin}–{wmax}, ±60 (band {wmin_lo}–{wmax_hi}); **must not go below {wmin_lo} or above {wmax_hi}**; expand if short, trim if long.\n\
             End with a bullet list of fixes by category: continuity / plot-card landing / plot errors / relationships / structure / prose / length."
        )
    }
}

pub fn refine_done_msg(
    loc: PromptLocale,
    model: &str,
    linked_n: u32,
    words: u32,
    mock: bool,
    full_body: bool,
) -> String {
    if loc.is_zh() {
        if mock {
            format!("未配置 DeepSeek API Key，已使用 Mock。字数 {words}。")
        } else if full_body {
            format!("精修完成（{model}，参考前 {linked_n} 章全文），字数 {words}。")
        } else {
            format!("精修完成（{model}，参考前 {linked_n} 章记忆/大纲），字数 {words}。")
        }
    } else if mock {
        format!("No DeepSeek API key — used mock. Word count {words}.")
    } else if full_body {
        format!("Refine done ({model}, prior {linked_n} chapters full text), word count {words}.")
    } else {
        format!("Refine done ({model}, prior {linked_n} chapters memory/outlines), word count {words}.")
    }
}

pub fn extract_chapter_memory_system(loc: PromptLocale) -> String {
    // 具体抽取要求由用户可编辑的「抽取注意事项」提供；此处只定角色与底线。
    let body = if loc.is_zh() {
        "你是小说章节记忆抽取器。只输出结构化记忆，不要复述全文，不要写创作建议。\n\
         严格按用户给出的「抽取注意事项」决定抽取范围与格式；注意事项被删减的部分可不做；用户补充的要求须遵守。\n\
         若提供「其他章节记忆」：仅供对照（去重、衔接、避免与既定事实冲突）；本章记忆仍只写本章正文确凿信息，禁止把其他章内容原样抄进本章记忆。\n\
         若注意事项为空：仅用短列表写出本章确凿要点（人物/行为/结果），无则写 `- （无核心记忆）`。"
            .to_string()
    } else {
        "You extract structured chapter memory. No full paraphrase, no writing advice.\n\
         Follow the user’s extraction notes for scope and format; omit anything they removed; honor anything they added.\n\
         If other chapters’ memory is provided: use only for cross-check (dedupe, continuity); write facts from THIS chapter’s body only — do not copy other chapters into this memory.\n\
         If notes are empty: output a short bullet list of solid facts only, or `- (no core memory)`."
            .to_string()
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn extract_chapter_memory_user(
    loc: PromptLocale,
    label: &str,
    outline: &str,
    body: &str,
    user_notes: &str,
    other_memory: &str,
) -> String {
    let notes = if user_notes.trim().is_empty() {
        if loc.is_zh() {
            "抽取注意事项：（空——按最简要点抽取）\n\n".to_string()
        } else {
            "Extraction notes: (empty — extract minimal facts only)\n\n".to_string()
        }
    } else if loc.is_zh() {
        format!("抽取注意事项（须优先遵守，写入记忆时体现）：\n{user_notes}\n\n")
    } else {
        format!("Extraction notes (follow these; reflect in memory):\n{user_notes}\n\n")
    };
    let others = if other_memory.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!(
            "——— 其他章节记忆（用户勾选；对照用，勿抄入本章记忆）———\n{other_memory}\n\n"
        )
    } else {
        format!(
            "——— Other chapters’ memory (user-selected; cross-check only — do not copy into this chapter) ———\n{other_memory}\n\n"
        )
    };
    if loc.is_zh() {
        format!("章节：{label}\n大纲：{outline}\n\n{notes}{others}正文：\n{body}")
    } else {
        format!("Chapter: {label}\nOutline: {outline}\n\n{notes}{others}Body:\n{body}")
    }
}


pub fn gen_chapter_cards_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    chapter_count: u32,
    from: u32,
    to: u32,
    nums: &[u32],
    replace: bool,
) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let replace_note = if replace {
        if loc.is_zh() {
            "本批为「覆盖重写」：将替换树上同号章节的标题与大纲；须与范围外已有大纲/记忆衔接，勿与既定事实矛盾。"
        } else {
            "This batch REPLACES existing same-number titles/outlines; stay consistent with chapters outside the range and any given memory."
        }
    } else if loc.is_zh() {
        "本批为新增或补全章节卡；与树上已有章节衔接。"
    } else {
        "This batch adds/fills chapter cards; keep continuity with existing chapters."
    };
    let fmt = chapter_outline_body_format_rule(loc);
    let cards = chapter_linked_cards_hard_rule(loc);
    let example_outline = chapter_outline_body_example(loc);
    let out_contract = outlines_json_output_contract(loc, from, &nums_s, example_outline);
    let body = if loc.is_zh() {
        format!(
            "你是 Novel Work 大纲引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章：请按这一总篇幅分配本批章节在整体故事中的位置与信息量（开篇/发展/高潮/收束），勿把本批写成与总章数无关的独立短篇。\n\
             {replace_note}\n\
             任务：为第 {from}–{to} 章范围内需要生成的章节撰写大纲（编号列表：{nums_s}）。\n\
             要求：每章标题具体；章与章衔接合理；不要输出范围外的章。\n\
             {fmt}\n\
             {cards}\n\
             若材料含根节点剧情卡：本批须为其安排合理推进（勿只点名）；若含知识卡：专名与规则勿与之冲突。\n\
             {out_contract}"
        )
    } else {
        format!(
            "You are Novel Work’s outline engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total length: {chapter_count} chapters—pace this batch within that arc (setup/rising/climax/resolution); do not treat the batch as a standalone short story.\n\
             {replace_note}\n\
             Task: write outlines for chapters in {from}–{to} that need generation (numbers: {nums_s}).\n\
             Each chapter needs a concrete title; keep continuity; no chapters outside the list.\n\
             {fmt}\n\
             {cards}\n\
             If materials include root plot cards: advance them in this batch (no name-drops). If knowledge cards are present: do not contradict names/rules.\n\
             {out_contract}"
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

/// 章节大纲正文（`outline` 字段）固定格式：1–2 必填标签 + 3–N AI 补充。
fn chapter_outline_body_format_rule(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "【大纲正文固定格式 — 必须严格遵守】\n\
         `outline` 字段必须是多行纯文本，按序编号，不得改用散文段落或其它结构：\n\
         1. [本章定位] …（本章在全书弧中的职责：推进/转折/铺垫/收束等，一至两句）\n\
         2. [开篇钩子] …（开场如何抓住读者：冲突/悬念/异象等，具体可写）\n\
         3. …\n\
         4. …（第 3 项起由你补充关键情节、人物动作、信息点、章末钩子等；至少再写 1 条，通常 3–6 条）\n\
         硬性要求：第 1、2 行标签必须分别为 `[本章定位]`、`[开篇钩子]`（方括号与用词不可改）；每行一条；禁止把整章写成无编号长段。"
            .into()
    } else {
        "[Fixed outline body format — mandatory]\n\
         The `outline` field MUST be multi-line plain text with numbered items in order (no free prose block):\n\
         1. [Chapter role] … (this chapter’s job in the arc: advance/turn/setup/payoff; 1–2 sentences)\n\
         2. [Opening hook] … (how the opening grabs the reader: conflict/mystery/anomaly; concrete)\n\
         3. …\n\
         4. … (from item 3 onward: key beats, character moves, reveals, end hook; at least one more, typically 3–6)\n\
         Hard rules: lines 1–2 labels MUST be exactly `[Chapter role]` and `[Opening hook]`; one beat per line; no unnumbered long paragraph."
            .into()
    }
}

/// 本章链接人物卡 / 剧情卡：硬约束（有材料时必须遵守）。
fn chapter_linked_cards_hard_rule(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "【本章链接卡 — 必须严格遵守】\n\
         若材料中出现「本章已链接人物与剧情」或等价区块：\n\
         - **人物卡**：言行、动机、关系、身份必须严格符合所列人设；不得改写核心性格/立场，不得让未链接人物抢戏压过链接人物。\n\
         - **剧情卡**：必须严格按各卡状态落实要点——进行中须在本章推进并写入大纲条目；已收束/已吸收/搁置仅作既定事实，不得推翻或无视。\n\
         - 禁止另起与链接剧情无关的主线；禁止只点名不落地；链接卡冲突时优先保证链接卡全部被覆盖。"
            .into()
    } else {
        "[Chapter-linked cards — mandatory when present]\n\
         If materials include a chapter-linked characters/plots section:\n\
         - **Characters**: speech, motives, relations, identity MUST match listed cards; do not rewrite core personality/stance; do not let unlinked cast overshadow linked ones.\n\
         - **Plot cards**: land every listed beat by status—advance active plots in this chapter’s outline items; treat resolved/absorbed/deferred as established facts (do not overturn or ignore).\n\
         - Do not invent an unrelated main line; no name-drops without landing; if conflict, covering all linked cards wins."
            .into()
    }
}

fn chapter_outline_body_example(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "\"1. [本章定位] …\\n2. [开篇钩子] …\\n3. …\\n4. …\""
    } else {
        "\"1. [Chapter role] …\\n2. [Opening hook] …\\n3. …\\n4. …\""
    }
}

/// 所有模型共用的 outlines JSON 输出契约（章节卡 / 刷新大纲 / 下一章）。
fn outlines_json_output_contract(loc: PromptLocale, from: u32, nums_s: &str, example_outline: &str) -> String {
    if loc.is_zh() {
        format!(
            "【输出契约 — 所有模型必须遵守，否则无法创建章节卡】\n\
             整段回复只能是一个 JSON 对象：不要 markdown 代码块、不要思维链、不要前后说明文字。\n\
             结构必须恰好为：{{\"outlines\":[{{\"n\":{from},\"label\":\"第{from}章 · 标题\",\"outline\":{example_outline}}}]}}\n\
             硬性要求：\n\
             - 顶层键名必须是英文字符串 outlines（禁止 chapters / 章节 / 其它键名）\n\
             - 每项必须含：数字 n、字符串 label、字符串 outline（outline 为多行纯文本，禁止数组/对象）\n\
             - n 必须属于编号列表：{nums_s}；label 建议含「第n章」\n\
             - outlines 数组须覆盖列表中的全部编号（可多条）；不得只输出散文"
        )
    } else {
        format!(
            "[Output contract — mandatory for all models or cards cannot be created]\n\
             Reply with ONE JSON object only: no markdown fences, no chain-of-thought, no prose before/after.\n\
             Exact shape: {{\"outlines\":[{{\"n\":{from},\"label\":\"Chapter {from} · title\",\"outline\":{example_outline}}}]}}\n\
             Hard rules:\n\
             - Top-level key MUST be English string outlines (not chapters / other names)\n\
             - Each item MUST have number n, string label, string outline (outline = multiline plain text, not array/object)\n\
             - Each n MUST be in: {nums_s}; prefer labels that include the chapter number\n\
             - Cover every listed number; no prose-only replies"
        )
    }
}

/// 解析失败时：把原文收成标准 outlines JSON（一次重试）。
pub fn repair_outlines_json_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        format!(
            "你是 JSON 格式修复器。用户会给出一次不规范的模型输出。\n\
             你的唯一任务：提取其中的章节大纲，输出标准 JSON 对象（不要其它文字）。\n\
             形状：{{\"outlines\":[{{\"n\":1,\"label\":\"第1章 · 标题\",\"outline\":\"1. [本章定位] …\\n2. [开篇钩子] …\\n3. …\"}}]}}\n\
             规则：顶层键只能是 outlines；n 为数字；label/outline 为字符串；outline 若是数组则拼成多行文本。\n\
             {}",
            loc.language_rule()
        )
    } else {
        format!(
            "You are a JSON repairer. The user provides a non-conforming model reply.\n\
             Task: extract chapter outlines into ONE JSON object only (no other text).\n\
             Shape: {{\"outlines\":[{{\"n\":1,\"label\":\"Chapter 1 · title\",\"outline\":\"1. [Chapter role] …\\n2. [Opening hook] …\\n3. …\"}}]}}\n\
             Rules: top-level key outlines only; n is a number; label/outline are strings; if outline is an array, join into multiline text.\n\
             {}",
            loc.language_rule()
        )
    }
}

pub fn repair_outlines_json_user(loc: PromptLocale, nums: &[u32], raw: &str) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let snip: String = raw.chars().take(12000).collect();
    if loc.is_zh() {
        format!(
            "目标章号：{nums_s}\n\
             请把下面内容改写成唯一合法 JSON：{{\"outlines\":[...]}}（覆盖这些章号）。\n\n\
             ——原文——\n{snip}"
        )
    } else {
        format!(
            "Target chapter numbers: {nums_s}\n\
             Rewrite the text below as the only valid JSON {{\"outlines\":[...]}} covering those numbers.\n\n\
             ——raw——\n{snip}"
        )
    }
}

/// 左侧生成确认框初稿：根设定 → 前序大纲+记忆 →（可选当前正文）→ 本批目标 → 期望占位。
pub fn outline_gen_brief(
    loc: PromptLocale,
    root_ref: &str,
    prior_block: &str,
    cur_body: Option<&str>,
    from: u32,
    to: u32,
    nums: &[u32],
) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let prior = if prior_block.trim().is_empty() {
        if loc.is_zh() {
            "（尚无前序章节：仅依据根节点大纲与全书设定。）\n".to_string()
        } else {
            "(No prior chapters — use root outline and novel setup only.)\n".to_string()
        }
    } else {
        prior_block.to_string()
    };
    let body_sec = match cur_body.map(str::trim).filter(|s| !s.is_empty()) {
        Some(b) if loc.is_zh() => format!("## 当前章正文（续写参考，可删改）\n{b}\n\n"),
        Some(b) => format!("## Current chapter body (continue from; editable)\n{b}\n\n"),
        None => String::new(),
    };
    if loc.is_zh() {
        format!(
            "## 根节点与全书设定\n{root_ref}\n\n\
             ## 前序章节大纲与记忆（既定事实，新大纲不得推翻）\n{prior}\n\
             {body_sec}\
             ## 本批目标\n生成第 {from}–{to} 章章节卡（编号：{nums_s}）：每章标题 + 大纲要点。\n\n\
             ## 对本批章节的期望（可编辑）\n\
             （请在此填写对下一章或下 N 章的剧情期望、节奏、必须出现的冲突/伏笔/人物等；可留空。）\n"
        )
    } else {
        format!(
            "## Root & novel setup\n{root_ref}\n\n\
             ## Prior chapter outlines & memory (facts — do not overturn)\n{prior}\n\
             {body_sec}\
             ## Batch target\nGenerate chapter cards {from}–{to} (numbers: {nums_s}): title + outline beats each.\n\n\
             ## Expectations for this batch (editable)\n\
             (Describe desired plot, pacing, required conflicts/foreshadowing/characters for the next chapter(s); optional.)\n"
        )
    }
}

pub fn gen_chapter_cards_user(
    loc: PromptLocale,
    from: u32,
    to: u32,
    nums: &[u32],
    consideration: &str,
) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mat = if consideration.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("【须考虑的材料与期望（以用户编辑为准）】\n{consideration}\n\n")
    } else {
        format!("[Materials & expectations (user-edited)]\n{consideration}\n\n")
    };
    if loc.is_zh() {
        format!("{mat}请生成第 {from}–{to} 章中以下编号的章节卡（含 label 标题与 outline 大纲）：{nums_s}。不得推翻上述既定事实。outline 必须严格按固定格式：1.[本章定位] 2.[开篇钩子] 3–N 补充情节。若材料含本章链接人物/剧情卡，必须严格遵守其人设与剧情要点。整段回复只能是 {{\"outlines\":[...]}} JSON 对象。")
    } else {
        format!("{mat}Generate chapter cards (label + outline) for numbers: {nums_s} (range {from}–{to}). Do not overturn established facts above. Each outline MUST follow: 1.[Chapter role] 2.[Opening hook] 3–N more beats. Obey chapter-linked character/plot cards when present. Reply with ONLY the JSON object {{\"outlines\":[...]}}.")
    }
}

/// 从当前章续写后续章节大纲（章节卡「生成下一章」）。
pub fn plan_next_chapters_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    chapter_count: u32,
    from: u32,
    to: u32,
    nums: &[u32],
) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let fmt = chapter_outline_body_format_rule(loc);
    let cards = chapter_linked_cards_hard_rule(loc);
    let example_outline = chapter_outline_body_example(loc);
    let out_contract = outlines_json_output_contract(loc, from, &nums_s, example_outline);
    let body = if loc.is_zh() {
        format!(
            "你是 Novel Work 续章大纲引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章。任务：根据「当前章全文 + 历史记忆 + 根大纲 + 人物/剧情/知识卡设定」，\
             为第 {from}–{to} 章（编号：{nums_s}）撰写剧情大纲（标题+要点），顺着既有情节往前推。\n\
             规则：\n\
             - 承接当前章结尾的局势与悬念，不得推翻记忆中的既定事实；\n\
             - 多章时章与章衔接递进，节奏匹配全书总章数中的位置；\n\
             - 根剧情卡须安排推进；知识卡勿冲突；\n\
             - 只输出本批编号，同号视为覆盖重写标题与大纲。\n\
             {fmt}\n\
             {cards}\n\
             {out_contract}"
        )
    } else {
        format!(
            "You are Novel Work’s next-chapter outline engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total: {chapter_count} chapters. Task: from current chapter body + memory + root outline + character/plot/knowledge cards, \
             write plot outlines (title + beats) for chapters {from}–{to} (numbers: {nums_s}).\n\
             Rules: continue from the current chapter’s ending; do not overturn memory facts; \
             multi-chapter batches must progress coherently within the planned length; advance root plot cards; respect knowledge cards; \
             only listed numbers; same numbers overwrite title/outline.\n\
             {fmt}\n\
             {cards}\n\
             {out_contract}"
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn plan_next_chapters_user(
    loc: PromptLocale,
    from: u32,
    to: u32,
    nums: &[u32],
    consideration: &str,
) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mat = if consideration.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("【须考虑的材料与期望（以用户编辑为准）】\n{consideration}\n\n")
    } else {
        format!("[Materials & expectations (user-edited)]\n{consideration}\n\n")
    };
    if loc.is_zh() {
        format!(
            "{mat}请生成第 {from}–{to} 章大纲（编号 {nums_s}），写入 outlines JSON。不得推翻上述既定事实；承接当前局势与期望。每章 outline 必须：1.[本章定位] 2.[开篇钩子] 3–N 补充情节。若材料含本章链接人物/剧情卡，必须严格遵守。"
        )
    } else {
        format!(
            "{mat}Generate outlines for chapters {from}–{to} (numbers {nums_s}) as outlines JSON. Do not overturn facts; honor current situation and expectations. Each outline MUST be: 1.[Chapter role] 2.[Opening hook] 3–N more beats. Obey chapter-linked character/plot cards when present."
        )
    }
}

pub fn plan_next_done_msg(loc: PromptLocale, from: u32, to: u32, used_mock: bool) -> String {
    let mock = if used_mock {
        if loc.is_zh() {
            "（未配置 API Key，为本地占位大纲。）"
        } else {
            " (No API key — local placeholder outlines.)"
        }
    } else {
        ""
    };
    if loc.is_zh() {
        if from == to {
            format!("已生成第{from}章剧情大纲并写入结构树。{mock}")
        } else {
            format!("已生成第{from}–{to}章剧情大纲并写入结构树。{mock}")
        }
    } else if from == to {
        format!("Outlined chapter {from} and saved to the tree.{mock}")
    } else {
        format!("Outlined chapters {from}–{to} and saved to the tree.{mock}")
    }
}

pub fn gen_chapter_plots_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    chapter_count: u32,
) -> String {
    let body = if loc.is_zh() {
        format!(
            "你是 Novel Work 剧情拆解引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章：拆解剧情时考虑本章在全书中的位置，支线体量要匹配总篇幅。\n\
             任务：根据给定章节的大纲，拆成多张「剧情卡」，并整理涉及人物。\n\
             规则：\n\
             - 每张剧情卡对应大纲中的一条相对独立的情节线/事件/冲突，通常 2–6 张，勿空洞重复；\n\
             - 列出该剧情涉及的人物；树上已有同名人物不要改名，只需在 characters 里用相同 label；\n\
             - 缺失人物必须给出可写入人物卡的字段（role/personality 等可简短）；\n\
             - 不要输出与本章无关的剧情。\n\
             回复中必须包含完整 JSON（可多行或代码块）：{{\"plots\":[{{\"label\":\"剧情标题\",\"outline\":\"剧情要点\",\"characters\":[{{\"label\":\"人名\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}]}}\n\
             JSON 后可跟一句简短说明。"
        )
    } else {
        format!(
            "You are Novel Work’s plot-breakdown engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total: {chapter_count} chapters—size side plots for this chapter’s place in that arc.\n\
             Task: split the chapter outline into multiple plot cards and list involved characters.\n\
             Rules: 2–6 concrete plot cards; reuse existing character labels when names match; fill missing character fields; stay on-chapter.\n\
             Reply MUST include complete JSON (multi-line or fenced OK): {{\"plots\":[{{\"label\":\"title\",\"outline\":\"beats\",\"characters\":[{{\"label\":\"name\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}]}}\n\
             Then a short note is OK."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn gen_chapter_plots_user(
    loc: PromptLocale,
    chapter_num: u32,
    label: &str,
    outline: &str,
    existing_chars: &str,
    user_notes: &str,
) -> String {
    let chars = if existing_chars.trim().is_empty() {
        if loc.is_zh() {
            "（尚无人物卡）"
        } else {
            "(no character cards yet)"
        }
    } else {
        existing_chars
    };
    let notes = if user_notes.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("用户补充的剧情要求（须纳入拆解，不得忽略）：\n{user_notes}\n\n")
    } else {
        format!("User plot notes (must incorporate):\n{user_notes}\n\n")
    };
    if loc.is_zh() {
        format!(
            "请为第{chapter_num}章「{label}」生成剧情卡。\n\
             本章大纲：\n{outline}\n\n\
             {notes}\
             树上已有人物：\n{chars}\n\n\
             按大纲与补充要求拆多张剧情卡，补齐缺失人物并写入 JSON。"
        )
    } else {
        format!(
            "Generate plot cards for chapter {chapter_num} “{label}”.\n\
             Outline:\n{outline}\n\n\
             {notes}\
             Existing characters:\n{chars}\n\n\
             Output the plots JSON."
        )
    }
}

pub fn default_new_character(loc: PromptLocale) -> (&'static str, &'static str, &'static str) {
    if loc.is_zh() {
        ("新角色", "配角", "中立")
    } else {
        ("New character", "supporting", "neutral")
    }
}

/// Outline + extracted memory facts (no full prior chapter body).
pub fn prev_chapter_memory_block(
    loc: PromptLocale,
    label: &str,
    outline: &str,
    facts: &str,
) -> String {
    if loc.is_zh() {
        let mem = if facts.trim().is_empty() {
            "记忆：（尚未抽取，仅大纲）"
        } else {
            facts
        };
        format!("## {label}\n大纲：{outline}\n记忆要点：\n{mem}\n\n")
    } else {
        let mem = if facts.trim().is_empty() {
            "Memory: (not extracted yet — outline only)"
        } else {
            facts
        };
        format!("## {label}\nOutline: {outline}\nMemory facts:\n{mem}\n\n")
    }
}

pub fn prev_chapter_block(loc: PromptLocale, label: &str, outline: &str, body: &str) -> String {
    if loc.is_zh() {
        let body_part = if body.trim().is_empty() {
            "正文：（本章尚未生成，仅有大纲可供参考）".to_string()
        } else {
            format!("正文（完整）：\n{body}")
        };
        format!("## {label}\n大纲：{outline}\n{body_part}\n\n")
    } else {
        let body_part = if body.trim().is_empty() {
            "Body: (not generated yet — outline only)".to_string()
        } else {
            format!("Body (full):\n{body}")
        };
        format!("## {label}\nOutline: {outline}\n{body_part}\n\n")
    }
}

/// Cover T2I prompt: always English output (for image models).
pub fn cover_t2i_prompt_system() -> &'static str {
    "You write ONE English text-to-image prompt for a novel book cover.\n\
     Rules:\n\
     - Output ONLY the prompt text (no quotes, no markdown, no labels, no Chinese).\n\
     - One dense paragraph, ~40–90 words, comma-separated visual phrases OK.\n\
     - Emphasize: mood, main subject(s), setting, lighting, color palette, composition suitable for a vertical book cover.\n\
     - Style: polished book-cover illustration / cinematic key art; avoid readable title text in the image.\n\
     - No NSFW; no artist names; no camera brand spam."
}

pub fn cover_t2i_prompt_user(
    title: &str,
    synopsis: &str,
    root_outline: &str,
    characters: &str,
) -> String {
    format!(
        "Novel title: {title}\n\n\
         Synopsis:\n{synopsis}\n\n\
         Root outline / premise:\n{root_outline}\n\n\
         Key characters (for visual cues):\n{characters}\n\n\
         Write the English cover image prompt now."
    )
}

/// 整理近 N 章 → 根上跨章剧情卡（进行中/已收束/搁置）。
pub fn consolidate_plots_system(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "你是长篇连载的剧情线编辑。根据近几章大纲与章节记忆，整理「跨章节剧情卡」挂在全书根上。\n\
         只输出 JSON（可包在 ```json 中）：\n\
         {\"root_plots\":[{\"id\":\"已有卡id或空\",\"label\":\"短标题\",\"outline\":\"跨章要点/未收束钩子/下一步\",\"status\":\"active|resolved|deferred\"}],\
         \"absorb_plot_ids\":[\"可归档的章内剧情卡id\"]}\n\
         规则：\n\
         1) root_plots：3–8 张为宜；优先更新已有根卡（填 id），不足再新建（id 留空）。\n\
         2) status：active=进行中须继续推进；resolved=已收束；deferred=搁置。生成正文时只看 active。\n\
         3) outline 写跨章导航（因果/承诺/未解），不要复述整章正文。\n\
         4) absorb_plot_ids：仅填输入里列出的「章内剧情卡」id，表示内容已并入根卡、可归档回查；不要编造 id；宁缺毋滥。\n\
         5) 不得推翻记忆中的既定事实；不要删卡，只整理。"
    } else {
        "You are a serial-novel plot editor. From recent chapter outlines + memory, consolidate book-wide plot cards on the root.\n\
         Output JSON only (```json ok):\n\
         {\"root_plots\":[{\"id\":\"existing id or empty\",\"label\":\"short title\",\"outline\":\"cross-chapter beats/open hooks/next\",\"status\":\"active|resolved|deferred\"}],\
         \"absorb_plot_ids\":[\"chapter-local plot ids to archive\"]}\n\
         Rules:\n\
         1) Prefer 3–8 root_plots; update existing root cards by id when possible; else create (empty id).\n\
         2) status: active=must keep advancing; resolved=closed; deferred=parked. Generation only injects active.\n\
         3) outline = navigation (cause/promises/open threads), not full chapter retell.\n\
         4) absorb_plot_ids: only ids from the listed chapter-local plots whose content is now in root cards; never invent ids; sparse is fine.\n\
         5) Do not overturn memory facts; do not delete cards — consolidate only."
    }
}

pub fn consolidate_plots_user(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    chapters_block: &str,
    root_plots_block: &str,
    chapter_plots_block: &str,
    user_notes: &str,
) -> String {
    let notes = if user_notes.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("用户补充：\n{}\n\n", user_notes.trim())
    } else {
        format!("User notes:\n{}\n\n", user_notes.trim())
    };
    if loc.is_zh() {
        format!(
            "书名：{title}\n简介：{synopsis}\n\n\
             ## 近章材料（大纲+记忆）\n{chapters_block}\n\
             ## 现有根剧情卡（可更新）\n{root_plots_block}\n\
             ## 章内剧情卡（可 absorb）\n{chapter_plots_block}\n\
             {notes}\
             输出整理后的 JSON。"
        )
    } else {
        format!(
            "Title: {title}\nSynopsis: {synopsis}\n\n\
             ## Recent chapters (outline + memory)\n{chapters_block}\n\
             ## Existing root plot cards (updatable)\n{root_plots_block}\n\
             ## Chapter-local plot cards (absorbable)\n{chapter_plots_block}\n\
             {notes}\
             Output the consolidation JSON."
        )
    }
}

/// 正文编辑：单段按用户意见改写（chat_model）。
pub fn rewrite_paragraph_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "你是 Novel Work 段落改写助手。按用户修改意见改写给定段落。\n\
         硬性规则：只输出改写后的段落正文；不要解释、不要标题、不要用 markdown 代码围栏；\
         保持人称与时态；除非意见要求，否则不扩写成多段、不删改相邻未给出的情节。"
            .into()
    } else {
        "You are Novel Work's paragraph rewriter. Rewrite the given paragraph per the user's notes.\n\
         Hard rules: output only the rewritten paragraph body; no explanation, no title, no markdown fences; \
         keep person/tense; unless asked, do not expand into many paragraphs or invent adjacent plot."
            .into()
    }
}

pub fn rewrite_paragraph_user(loc: PromptLocale, paragraph: &str, instruction: &str) -> String {
    if loc.is_zh() {
        format!(
            "【原段落】\n{paragraph}\n\n【修改意见】\n{instruction}\n\n请只输出改写后的段落："
        )
    } else {
        format!(
            "[Original paragraph]\n{paragraph}\n\n[Revision notes]\n{instruction}\n\nOutput only the rewritten paragraph:"
        )
    }
}

/// 人物卡整卡 AI 改写：输出完整 JSON，填满所有字段。
pub fn rewrite_character_card_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "你是 Novel Work 人物设定助手。按用户提示词改写或生成整张人物卡。\n\
         硬性规则：只输出一个 JSON 对象；必须包含提示词列出的全部字段且每一项都有具体、可写作的内容；\
         禁止留空、禁止省略键、禁止写「待定」「暂无」「未知」；不要解释、不要 markdown 代码围栏。\n\
         若提供【参考书】（世界观、其他人物/剧情/知识卡），人设须与其中设定一致、可引用世界锚点中的法则名。\n\
         relations 至少一条且每条字段齐全；author_verdict 须为 prove|disprove|unresolved；\
         catchphrases 为至少一个非空字符串的数组。"
            .into()
    } else {
        "You are Novel Work's character-profile assistant. Rewrite or draft the full character card per the prompt.\n\
         Hard rules: output one JSON object only; every listed field must be filled with concrete, writable detail; \
         no empty values, no omitted keys, no placeholders like TBD/N/A; no explanation, no markdown fences.\n\
         If [Reference] materials (worldview, other character/plot/knowledge cards) are provided, stay consistent and \
         you may cite axiom names from world anchors.\n\
         relations: at least one complete entry; author_verdict: prove|disprove|unresolved; \
         catchphrases: array with at least one non-empty string."
            .into()
    }
}

pub fn is_character_whole_rewrite_label(label: &str) -> bool {
    label.trim() == "__character_card_whole__"
}

pub fn parse_story_rules_block_whole_label(label: &str) -> Option<&str> {
    const P: &str = "__story_rules_block__:";
    let s = label.trim();
    if !s.starts_with(P) {
        return None;
    }
    let slot = s[P.len()..].trim();
    if crate::story_rules_fmt::is_story_rules_fan_slot(slot) {
        Some(slot)
    } else {
        None
    }
}

pub fn is_story_rules_block_whole_label(label: &str) -> bool {
    parse_story_rules_block_whole_label(label).is_some()
}

pub fn rewrite_story_rules_block_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "你是 Novel Work 故事规则助手。按提示词改写或生成整张故事规则子卡。\n\
         硬性规则：只输出一个 JSON 对象；必须包含提示词列出的全部字段且每一项都有具体、可写作的内容；\
         禁止留空、禁止省略键、禁止写「待定」；列表字段为非空字符串数组；不要解释、不要 markdown 围栏。\n\
         若提供【参考书】（世界观、人物、剧情、其他知识卡、章节），须与其中设定一致。"
            .into()
    } else {
        "You are Novel Work's story-rules assistant. Rewrite or draft the full story-rules sub-card per the prompt.\n\
         Hard rules: one JSON object only; every listed field filled with concrete detail; \
         no empty values or omitted keys; list fields are non-empty string arrays; no fences or explanation.\n\
         If [Reference] (worldview, characters, plots, knowledge, chapters) is provided, stay consistent."
            .into()
    }
}

/// 故事规则 Chat：生成四张右侧子卡 JSON。
pub fn generate_story_rules_chat_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "你是 Novel Work 故事架构师。根据对话生成或改写故事规则四卡（表层设定、故事引擎、兑现系统、约束红线）。\n\
         硬性规则：只输出一个 JSON；含 reply（2–4 句中文）与 blocks 对象。\n\
         blocks 含 surface_setting、story_engine、fulfillment_system、constraint_redlines，字段名用英文键。\n\
         surface_setting: premise, core_conflict, reader_promise, target_audience, tone_reference, commercial_tags, extended_premise\n\
         story_engine: premise, bright_line, dark_line, suspense_setup, conflict_engine, external_conflict, internal_conflict, relational_conflict, progression_cycle, protagonist_dilemma\n\
         fulfillment_system: premise, growth_path, ending_texture, payoff_syntax[], emotional_rhythm, tension_circles[]\n\
         constraint_redlines: premise, redlines[]\n\
         须服从【功能选项】与【世界观快照】；用户要求参考章节时以【已有章节】为准。\n\
         输出示例：{\"reply\":\"…\",\"blocks\":{\"surface_setting\":{…},\"story_engine\":{…},\"fulfillment_system\":{…},\"constraint_redlines\":{…}}}"
            .into()
    } else {
        "You are Novel Work's story architect. From chat, generate or rewrite the four story-rules cards.\n\
         Output one JSON with reply and blocks (surface_setting, story_engine, fulfillment_system, constraint_redlines). \
         Use English keys as in the schema; list fields as non-empty string arrays. Obey [Story tags] and worldview snapshot."
            .into()
    }
}

pub fn generate_story_rules_chat_user(
    loc: PromptLocale,
    novel_title: &str,
    synopsis: &str,
    features_block: &str,
    worldview_snapshot: &str,
    blocks_snapshot: &str,
    chapter_context: &str,
    novel_reference: &str,
    history: &str,
    latest_user: &str,
) -> String {
    if loc.is_zh() {
        format!(
            "【小说】{novel_title}\n【简介】{synopsis}\n\n【功能选项】\n{features_block}\n\n\
             【已有章节（标题 + 大纲）】\n{chapter_context}\n\n\
             【全书卡面参考书】\n{novel_reference}\n\n\
             【世界观快照 JSON】\n{worldview_snapshot}\n\n【当前故事规则四卡 JSON】\n{blocks_snapshot}\n\n\
             【对话历史】\n{history}\n\n【用户最新消息】\n{latest_user}\n\n请输出 JSON（含 reply 与 blocks）："
        )
    } else {
        format!(
            "[Novel] {novel_title}\n[Synopsis] {synopsis}\n\n[Story tags]\n{features_block}\n\n\
             [Existing chapters (title + outline)]\n{chapter_context}\n\n\
             [Novel cards reference]\n{novel_reference}\n\n\
             [Worldview snapshot JSON]\n{worldview_snapshot}\n\n[Current four cards JSON]\n{blocks_snapshot}\n\n\
             [Chat history]\n{history}\n\n[Latest user message]\n{latest_user}\n\nOutput JSON with reply and blocks:"
        )
    }
}

/// 世界观字段改写 / 生成（一句话立意、公理字段、禁忌、力量体系等）。
pub fn rewrite_text_field_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "你是 Novel Work 世界观设定助手。按用户提示词改写或生成指定字段。\n\
         硬性规则：只输出该字段的最终正文；不要解释、不要标题、不要用 markdown 代码围栏；\
         保持设定自洽、简洁可注入写作；若原文为空则按提示词新写。"
            .into()
    } else {
        "You are Novel Work's worldbuilding assistant. Rewrite or draft the named field per the user's prompt.\n\
         Hard rules: output only the final field text; no explanation, no title, no markdown fences; \
         keep settings coherent and concise; if current text is empty, draft from the prompt."
            .into()
    }
}

pub fn rewrite_text_field_user(
    loc: PromptLocale,
    field_label: &str,
    current: &str,
    instruction: &str,
    context: &str,
) -> String {
    let cur = if current.trim().is_empty() {
        if loc.is_zh() {
            "（空，请新写）"
        } else {
            "(empty — draft new)"
        }
    } else {
        current
    };
    let ctx = context.trim();
    if loc.is_zh() {
        if ctx.is_empty() {
            format!(
                "【字段】{field_label}\n\n【当前内容】\n{cur}\n\n【提示词】\n{instruction}\n\n请只输出该字段的最终正文："
            )
        } else {
            format!(
                "【字段】{field_label}\n\n【相关上下文】\n{ctx}\n\n【当前内容】\n{cur}\n\n【提示词】\n{instruction}\n\n请只输出该字段的最终正文："
            )
        }
    } else if ctx.is_empty() {
        format!(
            "[Field] {field_label}\n\n[Current]\n{cur}\n\n[Prompt]\n{instruction}\n\nOutput only the final field text:"
        )
    } else {
        format!(
            "[Field] {field_label}\n\n[Context]\n{ctx}\n\n[Current]\n{cur}\n\n[Prompt]\n{instruction}\n\nOutput only the final field text:"
        )
    }
}

/// 根节点世界观 Chat：一次输出六卡 + 故事规则 JSON。
pub fn generate_worldview_chat_system(loc: PromptLocale) -> String {
    if loc.is_zh() {
        "你是 Novel Work 世界观架构师。根据用户对话，生成或重写整套世界观设定。\n\
         硬性规则：\n\
         1) 只输出一个 JSON 对象，不要 markdown 围栏、不要其它文字。\n\
         2) 顶层含 reply（给用户看的简短中文说明，2–4 句）与 worldview 对象。\n\
         3) worldview 各块要自洽、可注入写作；子项 2–5 条为宜。\n\
         4) 用户要求「随机/全新/重来」时，忽略现有世界观快照，但仍必须严格服从【功能选项】（题材/核心玩法/风格/关系/受众）；未配置时才可自由发挥。\n\
         5) 用户给出基础/种子时，在其上扩展并保持内部一致，且不得违背【功能选项】。\n\
         6) 用户要求微调时，在现有快照上修改，仍须符合【功能选项】。\n\
         7) 【功能选项】是硬约束：生成的世界、冲突、爽点、人物关系与受众口吻必须贴合这些标签，禁止写成无关题材或相反调性。\n\
         JSON 结构：\n\
         {\"reply\":\"…\",\"worldview\":{\n\
           \"core_laws\":{\"premise\":\"\",\"taboos\":[\"\"],\"power_system\":\"\",\"power_expression\":\"\",\n\
             \"axioms\":[{\"name\":\"\",\"statement\":\"\",\"boundary\":\"\",\"cost\":\"\",\"mechanism\":\"\"}]},\n\
           \"spatiotemporal\":{\"premise\":\"\",\"era\":\"\",\"ecology\":\"\",\"world_pattern\":\"\",\"atmosphere\":\"\",\n\
             \"locations\":[{\"name\":\"\",\"features\":\"\",\"terrain\":\"\",\"faction\":\"\"}]},\n\
           \"social_power\":{\"premise\":\"\",\"class_structure\":\"\",\"political_system\":\"\",\"power_visibility\":\"\",\n\
             \"races\":[{\"name\":\"\",\"features\":\"\",\"population\":\"\",\"social_status\":\"\"}],\n\
             \"factions\":[{\"name\":\"\",\"faction_type\":\"\",\"goal\":\"\",\"means\":\"\",\"power_base\":\"\"}]},\n\
           \"existence\":{\"premise\":\"\",\"death\":\"\",\"calendar\":\"\",\"lifespan\":\"\",\"disease_reproduction\":\"\"},\n\
           \"info_flow\":{\"premise\":\"\",\"info_speed\":\"\",\"info_barrier\":\"\",\"message_truth\":\"\",\"knowledge_carrier\":\"\"},\n\
           \"history_culture\":{\"premise\":\"\",\"customs\":\"\",\"economy\":\"\",\"daily_slices\":\"\",\n\
             \"religions\":[{\"name\":\"\",\"core_belief\":\"\",\"followers_scope\":\"\"}],\n\
             \"major_events\":[{\"title\":\"\",\"event\":\"\",\"long_term_impact\":\"\"}]},\n\
           \"story_rules\":{\"extracted\":\"…\"}\n\
         }}\n\
         必须包含全部六块世界观（core_laws / spatiotemporal / social_power / existence / info_flow / history_culture）以及 story_rules，不可省略 history_culture。"
            .into()
    } else {
        "You are Novel Work's worldbuilding architect. From the chat, generate or rewrite the full worldview.\n\
         Hard rules:\n\
         1) Output one JSON object only — no markdown fences, no extra text.\n\
         2) Top level: reply (2–4 sentences for the user) and worldview.\n\
         3) Keep sections coherent and injectable; prefer 2–5 items per list.\n\
         4) On random/reset: ignore the current worldview snapshot, but you MUST still obey [Story tags] (genre/gameplay/tone/romance/audience). Only invent freely if tags are empty.\n\
         5) When the user gives a seed/basis, expand it consistently without violating [Story tags].\n\
         6) On tweak requests, edit the snapshot while staying within [Story tags].\n\
         7) [Story tags] are hard constraints for genre, core gameplay, tone, relationships, and audience.\n\
         8) worldview MUST include all six blocks: core_laws, spatiotemporal, social_power, existence, info_flow, history_culture, plus story_rules.extracted.\n\
         history_culture shape: premise, customs, economy, daily_slices,\n\
         religions[{name,core_belief,followers_scope}], major_events[{title,event,long_term_impact}]."
            .into()
    }
}

pub fn generate_worldview_chat_user(
    loc: PromptLocale,
    novel_title: &str,
    synopsis: &str,
    root_outline: &str,
    features_block: &str,
    snapshot_json: &str,
    history: &str,
    latest_user: &str,
) -> String {
    if loc.is_zh() {
        format!(
            "【小说】{novel_title}\n【简介】{synopsis}\n【根大纲】{root_outline}\n\n\
             【功能选项】（硬约束，必须体现）\n{features_block}\n\n\
             【当前世界观快照 JSON】\n{snapshot_json}\n\n\
             【对话历史】\n{history}\n\n\
             【用户最新消息】\n{latest_user}\n\n\
             请输出 JSON（含 reply 与 worldview）："
        )
    } else {
        format!(
            "[Novel] {novel_title}\n[Synopsis] {synopsis}\n[Root outline] {root_outline}\n\n\
             [Story tags] (hard constraints)\n{features_block}\n\n\
             [Current worldview snapshot JSON]\n{snapshot_json}\n\n\
             [Chat history]\n{history}\n\n\
             [Latest user message]\n{latest_user}\n\n\
             Output JSON with reply and worldview:"
        )
    }
}

/// 简纲 → 细纲（分条场景要点）
pub fn generate_detailed_outline_system(loc: PromptLocale) -> String {
    let body = if loc.is_zh() {
        "你是 Novel Work 细纲引擎。任务：把本章**简纲**进化为可直接扩写正文的**细纲**列表。\n\
         规则：\n\
         1）细纲每条是一个可落地的场景/节拍（谁、做什么、结果或转折），按时间顺序；\n\
         2）覆盖简纲全部关键点，可合理拆细，但禁止另起无关主线或推翻简纲；\n\
         3）结合已链接人物/剧情/知识卡，使细纲可写、可验收；\n\
         4）条数通常 5–12；过短则拆，过碎则合并；\n\
         5）只输出 JSON：{\"detailed_outline\":[\"…\",\"…\"]}，不要 Markdown 围栏或其它字段。"
    } else {
        "You are Novel Work’s detailed-outline engine. Evolve the chapter **brief outline** into a **detailed outline** list ready for body expansion.\n\
         Rules:\n\
         1) Each item is a landable scene/beat (who, does what, result/turn), in order;\n\
         2) Cover every key beat of the brief outline; split as needed; do not invent a conflicting arc;\n\
         3) Honor linked characters/plots/knowledge so beats are writable and checkable;\n\
         4) Typically 5–12 items;\n\
         5) Output JSON only: {\"detailed_outline\":[\"…\",\"…\"]} — no markdown fences or extra fields."
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn generate_detailed_outline_user(
    loc: PromptLocale,
    root_ref: &str,
    chapter_info: &str,
    cards: &str,
    user_notes: &str,
) -> String {
    let notes = if user_notes.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("——— 补充要求 ——\n{user_notes}\n\n")
    } else {
        format!("——— Extra notes ——\n{user_notes}\n\n")
    };
    if loc.is_zh() {
        format!(
            "{root_ref}\n\n{notes}{chapter_info}\n\n{cards}\n\n\
             请根据本章简纲（及链接卡）输出细纲 JSON：{{\"detailed_outline\":[\"…\"]}}"
        )
    } else {
        format!(
            "{root_ref}\n\n{notes}{chapter_info}\n\n{cards}\n\n\
             From the brief outline (and linked cards), output detailed outline JSON: {{\"detailed_outline\":[\"…\"]}}"
        )
    }
}

/// 重写细纲中的单条节拍（保留前后条与简纲一致）
pub fn regenerate_detailed_outline_item_system(loc: PromptLocale) -> String {
    let body = if loc.is_zh() {
        "你是 Novel Work 细纲引擎。任务：只重写本章细纲中的**一条**场景节拍。\n\
         规则：\n\
         1）输出须与前后相邻细纲条衔接，不推翻简纲主线，不另起无关支线；\n\
         2）本条仍是可落地的场景要点（谁、做什么、结果或转折），长度与相邻条相近；\n\
         3）只改指定下标那一条；不要输出整份细纲列表；\n\
         4）只输出 JSON：{\"item\":\"…\"}，不要 Markdown 围栏或其它字段。"
    } else {
        "You are Novel Work’s detailed-outline engine. Rewrite **one** beat of the chapter detailed outline.\n\
         Rules:\n\
         1) Keep continuity with neighboring beats; do not overturn the brief outline or invent a side arc;\n\
         2) One landable scene beat (who, does what, result/turn), similar length to neighbors;\n\
         3) Rewrite only the indexed item — do not return the full list;\n\
         4) Output JSON only: {\"item\":\"…\"} — no markdown fences or extra fields."
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn regenerate_detailed_outline_item_user(
    loc: PromptLocale,
    root_ref: &str,
    chapter_info: &str,
    cards: &str,
    index: usize,
    current: &str,
    neighbors: &str,
    user_notes: &str,
) -> String {
    let notes = if user_notes.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("——— 补充要求 ——\n{user_notes}\n\n")
    } else {
        format!("——— Extra notes ——\n{user_notes}\n\n")
    };
    let idx1 = index + 1;
    if loc.is_zh() {
        format!(
            "{root_ref}\n\n{notes}{chapter_info}\n\n{cards}\n\n\
             ——— 当前细纲（含序号）———\n{neighbors}\n\n\
             请只重写第 {idx1} 条（0-based index={index}）。\n\
             当前文案：{current}\n\n\
             输出 JSON：{{\"item\":\"…\"}}"
        )
    } else {
        format!(
            "{root_ref}\n\n{notes}{chapter_info}\n\n{cards}\n\n\
             ——— Current detailed outline (numbered) ——\n{neighbors}\n\n\
             Rewrite only item #{idx1} (0-based index={index}).\n\
             Current text: {current}\n\n\
             Output JSON: {{\"item\":\"…\"}}"
        )
    }
}
