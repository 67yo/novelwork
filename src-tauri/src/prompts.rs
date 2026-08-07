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

pub fn knowledge_extract_system(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "你是小说知识库分析助手。根据目录与正文抽样，输出 Markdown，必须包含这些小节（用二级标题）：\n\
         ## 书名\n## 作者\n## 章节大纲\n（按目录逐章：章名 + 一两句情节要点；目录很长时可合并卷/篇）\n\
         ## 写作手法\n（叙事视角、节奏、文风措辞、人物塑造、章法结构、伏笔与转折习惯等，写具体可模仿的要点）\n\
         不要写客套话。"
    } else {
        "You are a novel knowledge-base analyst. From the TOC and text samples, output Markdown with these H2 sections:\n\
         ## Title\n## Author\n## Chapter outline\n(one short beat per chapter; merge volumes if the TOC is huge)\n\
         ## Writing craft\n(POV, pacing, diction, characterization, structure, foreshadowing habits—concrete and imitable)\n\
         No pleasantries."
    }
}

pub fn knowledge_extract_user(
    loc: PromptLocale,
    extract_prompt: &str,
    title: &str,
    author: &str,
    toc: &str,
    samples: &str,
) -> String {
    if loc.is_zh() {
        format!(
            "提取需求：{extract_prompt}\n\n已知书名：{title}\n已知作者：{author}\n\n目录：\n{toc}\n\n正文抽样：\n{samples}"
        )
    } else {
        format!(
            "Extract focus: {extract_prompt}\n\nKnown title: {title}\nKnown author: {author}\n\nTOC:\n{toc}\n\nSamples:\n{samples}"
        )
    }
}

pub fn parse_title_author(_loc: PromptLocale, line: &str) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut author = None;
    let t = line.trim();
    for (keys, slot) in [
        (
            [
                "## 书名",
                "## Title",
                "书名：",
                "书名:",
                "Title:",
                "title:",
            ]
            .as_slice(),
            &mut title,
        ),
        (
            [
                "## 作者",
                "## Author",
                "作者：",
                "作者:",
                "Author:",
                "author:",
            ]
            .as_slice(),
            &mut author,
        ),
    ] {
        for k in keys {
            if let Some(v) = t.strip_prefix(k) {
                let v = v.trim().trim_start_matches(['：', ':']).trim();
                if !v.is_empty() {
                    *slot = Some(v.to_string());
                }
                break;
            }
        }
    }
    (title, author)
}

/// Fill title/author from analysis markdown (heading may put value on the next line).
pub fn apply_analysis_meta(analysis: &str, title: &mut String, author: &mut String) {
    let mut expect_title = false;
    let mut expect_author = false;
    for line in analysis.lines() {
        let trimmed = line.trim();
        let (t, a) = parse_title_author(PromptLocale::ZhCn, trimmed);
        if expect_title && !trimmed.is_empty() && !trimmed.starts_with('#') {
            *title = trimmed.trim_start_matches(['*', '-', ' ']).to_string();
            expect_title = false;
        }
        if expect_author && !trimmed.is_empty() && !trimmed.starts_with('#') {
            *author = trimmed.trim_start_matches(['*', '-', ' ']).to_string();
            expect_author = false;
        }
        if let Some(v) = t {
            *title = v;
            expect_title = false;
        } else if trimmed == "## 书名" || trimmed == "## Title" {
            expect_title = true;
        }
        if let Some(v) = a {
            *author = v;
            expect_author = false;
        } else if trimmed == "## 作者" || trimmed == "## Author" {
            expect_author = true;
        }
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
            "你是 Nove Work 开书助手。通过简短对话帮用户确定：书名、简介、主要角色、每章目标字数范围、全书计划总章数。\n\
             必须询问：每章大约多少字（如 2000–3000），以及这本小说大概写多少章（如 20、30）。不要现在生成章节大纲或章节列表。\n\
             信息不足时用一两句追问。信息足够，或用户说「创建/生成/开始」时，先用一两句确认，\
             然后单独一行输出 JSON（不要包在代码块里）：\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"chapter_count\":20,\"characters\":[{{\"label\":\"\",\"role\":\"主角\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"正派\"}}]}}}}\n\
             characters 至少主角；word_count_min/max 为每章目标字数；chapter_count 为全书计划章数。{force_hint}"
        )
    } else {
        format!(
            "You are the Nove Work novel-creation assistant. Through short dialogue, settle: title, synopsis, main characters, target words per chapter, and planned total chapter count.\n\
             Always ask for approximate words per chapter (e.g. 2000–3000) and how many chapters the novel will have (e.g. 20, 30). Do NOT generate chapter outlines or chapter lists yet.\n\
             If info is missing, ask one or two brief follow-ups. When enough—or the user says create/generate/start—confirm in one or two sentences, \
             then output a single JSON line (no code fence):\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"chapter_count\":20,\"characters\":[{{\"label\":\"\",\"role\":\"protagonist\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}}}\n\
             Include at least the protagonist; word_count_min/max are per-chapter targets; chapter_count is the planned total chapters.{force_hint}"
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
    pub alignment: &'static str,
    pub personality: &'static str,
    pub style: &'static str,
    pub motto: &'static str,
    pub card_incomplete: &'static str,
    pub no_characters: &'static str,
    pub empty_plot: &'static str,
    pub no_plots: &'static str,
    pub empty_outline: &'static str,
    pub chapter_title: &'static str,
    pub chapter_outline: &'static str,
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
            alignment: "阵营",
            personality: "性格",
            style: "行事风格",
            motto: "座右铭",
            card_incomplete: "（卡面未补全）",
            no_characters: "（本章未链接人物卡）\n",
            empty_plot: "（剧情卡大纲为空）",
            no_plots: "（本章未链接剧情卡）\n",
            empty_outline:
                "（本章大纲为空，请仅依据已链接人物/剧情与小说简介合理推进，勿偏离已有设定）",
            chapter_title: "章节标题",
            chapter_outline: "本章默认大纲（必须遵循的主线走向）",
            chars_header: "【链接人物卡（含根节点贯穿全书人物）——言行必须符合；人物关系见下】",
            relations_header: "【人物关系——互动与称呼须符合；含根节点人物之间及与本章人物的关系】",
            relation_unset: "（关系未标注）",
            plots_header: "【链接剧情卡——须融入本章或与之呼应】",
            knowledge_header: "【知识卡——写作必须参考的特征与约束】",
            no_knowledge: "（未链接知识卡）\n",
            empty_knowledge: "（尚未提取特征，请仅按提取需求约束）",
        }
    } else {
        ChapterContextLabels {
            name: "Name",
            role: "Role",
            gender: "Gender",
            alignment: "Alignment",
            personality: "Personality",
            style: "Style",
            motto: "Motto",
            card_incomplete: "(card incomplete)",
            no_characters: "(no character cards linked)\n",
            empty_plot: "(plot outline empty)",
            no_plots: "(no plot cards linked)\n",
            empty_outline:
                "(chapter outline empty — advance only from linked characters/plots and the synopsis; do not invent conflicting lore)",
            chapter_title: "Chapter title",
            chapter_outline: "Default chapter outline (main arc — must follow)",
            chars_header: "[Linked character cards (incl. root book-wide cast) — speech/actions must match; see relations below]",
            relations_header: "[Character relations — incl. among root cast and with chapter characters]",
            relation_unset: "(relation not set)",
            plots_header: "[Linked plot cards — weave in or echo in this chapter]",
            knowledge_header: "[Knowledge cards — features/constraints writing MUST follow]",
            no_knowledge: "(no knowledge cards linked)\n",
            empty_knowledge: "(features not extracted yet — honor the extract request only)",
        }
    }
}

pub fn generate_chapter_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    knowledge_strategy: &str,
    wmin: u32,
    wmax: u32,
    chapter_count: u32,
) -> String {
    let body = if loc.is_zh() {
        format!(
            "你是强约束小说写作引擎 Nove Work。生成本章时必须同时遵守：\n\
             1）全书故事简介与根节点关联人物卡（总体设定参考，勿偏离）；\n\
             2）本章默认大纲（主线走向，不可丢弃关键情节点）；\n\
             3）本章已链接人物卡（性格、身份、行事风格、关系，言行不得出戏）；\n\
             4）本章已链接剧情卡（支线要点须自然融入或与本章呼应）。\n\
             5）已链接知识卡（提取特征与知识约束必须遵守，勿与之矛盾）。\n\
             6）全书计划共 {chapter_count} 章：本章信息量与悬念投放须符合所处位置，勿按无限连载节奏注水。\n\
             未链接到本章的设定不要硬塞；冲突时以本章大纲为主线，人物为行为约束，剧情为支线补强，知识卡定技法/设定边界，简介定调。\n\
             小说：《{title}》\n简介：{synopsis}\n知识库策略：{knowledge_strategy}\n\
             【硬性篇幅】本章正文非空白字符数必须落在 {wmin}–{wmax} 字；若上下限相同，尽量贴近该目标（可略超，禁止明显偏短）。写不够请继续写到接近目标再结束。"
        )
    } else {
        format!(
            "You are Nove Work, a strongly constrained novel-writing engine. When generating this chapter you MUST obey:\n\
             1) Novel synopsis and root-linked character cards (global setting — do not contradict);\n\
             2) The default chapter outline (main arc — do not drop key beats);\n\
             3) Chapter-linked character cards (traits, role, style — stay in character);\n\
             4) Chapter-linked plot cards (side-plot beats must be woven in or echoed).\n\
             5) Linked knowledge cards (extracted features/constraints — do not contradict).\n\
             6) Planned total: {chapter_count} chapters—pace info/suspense for this chapter’s place in that arc; do not pad as endless serial.\n\
             Do not force unlinked lore. On conflicts: chapter outline drives the arc, characters constrain behavior, plots reinforce side threads, knowledge cards bound craft/setting, synopsis sets tone.\n\
             Novel: “{title}”\nSynopsis: {synopsis}\nKnowledge strategy: {knowledge_strategy}\n\
             [Hard length] Non-whitespace character count MUST land in {wmin}–{wmax}. If min equals max, stay near that target (slightly over OK; clearly short is not)."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn generate_chapter_user(
    loc: PromptLocale,
    root_ref: &str,
    chapter_info: &str,
    cards: &str,
    wmin: u32,
    wmax: u32,
) -> String {
    if loc.is_zh() {
        format!(
             "{root_ref}\n\n{chapter_info}\n\n{cards}\n\n请生成本章正文（Markdown），篇幅目标 {wmin}–{wmax} 字（非空白）。写完后自检：简介与根节点人物、大纲要点、本章链接人物/剧情/知识卡，以及字数是否达标。"
        )
    } else {
        format!(
            "{root_ref}\n\n{chapter_info}\n\n{cards}\n\nWrite this chapter’s body (Markdown), length target {wmin}–{wmax} non-whitespace characters. Then self-check synopsis/root characters, outline, linked character/plot/knowledge cards, and length."
        )
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

pub fn generate_done_msg(loc: PromptLocale, model: &str, words: u32, mock: bool) -> String {
    if loc.is_zh() {
        if mock {
            format!("未配置 DeepSeek API Key，已使用 Mock。字数 {words}。")
        } else {
            format!("预生成完成（{model}），字数 {words}。")
        }
    } else if mock {
        format!("No DeepSeek API key — used mock. Word count {words}.")
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
) -> String {
    let body = if loc.is_zh() {
        format!(
            "你是 Nove Work 章节精修引擎。小说：《{title}》。\n\
             【精修目的】不大改剧情：在已生成正文的情节骨架上做小幅梳理与润色；必须结合「参考前 N 章」的完整正文来核对历史因果，再梳理当前章，使衔接合理、人物关系正确、结构正常、语句通顺；禁止另起炉灶或大幅改写主线。\n\
             【必须同时使用的材料】\n\
             1）参考前 {linked_n} 章的完整正文（全文已导入：标题+大纲+正文；只作历史依据来梳理当前章，禁止改写这些历史章）；\n\
             2）当前章全部链接卡片（大纲、人物卡、剧情卡、知识卡、人物关系等）；\n\
             3）当前章已生成的完整正文（精修对象：尽量保留原有情节与段落顺序，只改与历史矛盾、不合理、不通顺之处）。\n\
             【必须确保】当前章与上述历史章节内容衔接合理；无异常/突兀情节；无剧情错误与时间线矛盾；人物关系与卡面一致；结构清楚；语句通顺。\n\
             本章目标字数约 {wmin}–{wmax} 字；精修后篇幅仍须贴近该目标，篇幅波动应小。"
        )
    } else {
        format!(
            "You are the Nove Work chapter refine engine. Novel: “{title}”.\n\
             [Purpose] Do not majorly rewrite the plot. Use the full text of the prior N reference chapters to check historical continuity, then lightly tidy the current chapter for continuity, relationships, structure, and prose. Do not restart or overhaul the main arc.\n\
             [Required inputs]\n\
             1) The previous {linked_n} chapters in FULL (title + outline + complete body already provided — history only; never rewrite them);\n\
             2) All cards linked to the current chapter;\n\
             3) The full already-generated current-chapter body (preserve plot beats and order; fix only contradictions with history, errors, and awkward prose).\n\
             [Must ensure] Current chapter aligns with that history; no absurd/abrupt beats; no plot/timeline errors; relationships match cards; clear structure; fluent prose.\n\
             Target ≈ {wmin}–{wmax} words; keep length close with only small drift."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn refine_chapter_user(
    loc: PromptLocale,
    knowledge_strategy: &str,
    linked_n: u32,
    prev_text: &str,
    chapter_info: &str,
    cards: &str,
    current: &str,
) -> String {
    let hist = if prev_text.trim().is_empty() {
        if loc.is_zh() {
            "（未导入历史章节：prev_n=0 或无前序章）\n"
        } else {
            "(no prior chapters imported: prev_n=0 or none exist)\n"
        }
    } else {
        prev_text
    };
    if loc.is_zh() {
        format!(
            "知识策略：{knowledge_strategy}\n\n\
             ——— ① 参考前 {linked_n} 章完整内容（全文，结合历史梳理当前章；勿改写历史）———\n{hist}\n\
             ——— ② 当前章链接设定（大纲 + 全部链接卡片）———\n{chapter_info}\n\n{cards}\n\n\
             ——— ③ 当前章已生成正文（精修对象：对照①的历史全文梳理合理性，不大改剧情）———\n{current}\n\n\
             请先通读①中各章完整正文，再对照②③，输出精修后的完整当前章 Markdown（不要输出历史章）。改动应克制。\n\
             文末用列表列出本次修正点，按类归并：历史衔接 / 情节错误 / 人物关系 / 结构 / 语句。"
        )
    } else {
        format!(
            "Knowledge strategy: {knowledge_strategy}\n\n\
             ——— ① Prior {linked_n} chapters in FULL (use this history to tidy the current chapter; do not rewrite history) ———\n{hist}\n\
             ——— ② Current chapter linked setup (outline + all linked cards) ———\n{chapter_info}\n\n{cards}\n\n\
             ——— ③ Current chapter body (refine against ①’s full history; do not overhaul plot) ———\n{current}\n\n\
             Read every prior chapter body in ① first, then revise ③ against ① and ②. Output only the refined current chapter in Markdown.\n\
             End with a bullet list of fixes by category: continuity / plot errors / relationships / structure / prose."
        )
    }
}

pub fn refine_done_msg(
    loc: PromptLocale,
    model: &str,
    linked_n: u32,
    words: u32,
    mock: bool,
) -> String {
    if loc.is_zh() {
        if mock {
            format!("未配置 DeepSeek API Key，已使用 Mock。字数 {words}。")
        } else {
            format!("精修完成（{model}，关联前 {linked_n} 章），字数 {words}。")
        }
    } else if mock {
        format!("No DeepSeek API key — used mock. Word count {words}.")
    } else {
        format!("Refine done ({model}, prior {linked_n} chapters), word count {words}.")
    }
}

pub fn workspace_chat_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    has_chapters: bool,
    wmin: u32,
    wmax: u32,
    chapter_count: u32,
    chapter_list: &str,
) -> String {
    let body = if loc.is_zh() {
        let status = if has_chapters { "已有" } else { "尚无" };
        format!(
            "你是 Nove Work 创作助手。当前小说《{title}》。简介：{synopsis}\n\
             树图现状：{status}章节节点；每章目标字数约 {wmin}–{wmax}；全书计划共 {chapter_count} 章（后续剧情与大纲须按此总篇幅分配节奏，勿写成无限连载）。\n\
             【根节点斜杠指令由系统直接执行，用户以 / 开头发送；你无需伪造这些操作的 JSON】\n\
             · /章节卡 1-10（冲突时：/章节卡 覆盖|跳过|强制追加 1-10）\n\
             · /添加剧情 3 剧情内容\n\
             · /剧情卡 3\n\
             · /清空章节\n\
             若用户要求修改每章目标字数，回复第一行用 JSON：{{\"word_count\":{{\"min\":4500,\"max\":4500}}}}（单点目标时 min=max）。\n\
             若用户要求修改全书计划章数，回复第一行用 JSON：{{\"chapter_count\":30}}。\n\
             若用户描述新角色，回复第一行用 JSON：{{\"character\":{{\"label\":\"\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\",\"link_chapter_id\":\"可选章节节点id\"}}}}\n\
             若用户在自由对话中要求追加若干章大纲（非上述指令格式），回复第一行用 JSON：{{\"outlines\":[{{\"n\":1,\"label\":\"第一章 · 具体标题\",\"outline\":\"本章剧情要点\"}}]}}（会追加到树，不会清空旧章）\n\
             然后再用自然语言正常对话。可用章节节点：{chapter_list}"
        )
    } else {
        let status = if has_chapters { "has" } else { "has no" };
        format!(
            "You are the Nove Work writing assistant. Novel “{title}”. Synopsis: {synopsis}\n\
             Tree status: {status} chapter nodes; target ≈ {wmin}–{wmax} words per chapter; planned total {chapter_count} chapters (pace all plots/outlines to this length—not endless serialization).\n\
             [Root slash commands are handled by the app — do not invent JSON for them]\n\
             · /chapters 1-10 (conflict: /chapters overwrite|skip|force 1-10)\n\
             · /add-plot 3 plot text\n\
             · /plots 3\n\
             · /clear-chapters\n\
             If the user changes the per-chapter word target, first line MUST be JSON: {{\"word_count\":{{\"min\":4500,\"max\":4500}}}} (use min=max for a single target).\n\
             If the user changes the planned total chapters, first line MUST be JSON: {{\"chapter_count\":30}}.\n\
             If the user describes a new character, first line MUST be JSON: {{\"character\":{{\"label\":\"\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\",\"link_chapter_id\":\"optional chapter node id\"}}}}\n\
             If free-form chat asks to append outlines (not the commands above), first line JSON: {{\"outlines\":[{{\"n\":1,\"label\":\"Chapter 1 · title\",\"outline\":\"beats\"}}]}} (appends; never wipes existing chapters)\n\
             Then continue in natural language. Available chapter nodes: {chapter_list}"
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn gen_chapter_cards_system(
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
    let body = if loc.is_zh() {
        format!(
            "你是 Nove Work 大纲引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章：请按这一总篇幅分配本批章节在整体故事中的位置与信息量（开篇/发展/高潮/收束），勿把本批写成与总章数无关的独立短篇。\n\
             任务：为第 {from}–{to} 章范围内需要生成的章节撰写大纲（编号列表：{nums_s}）。\n\
             要求：每章标题具体、大纲含关键情节点；章与章衔接合理；不要输出范围外的章。\n\
             回复第一行必须是且仅包含 JSON：{{\"outlines\":[{{\"n\":1,\"label\":\"第一章 · 标题\",\"outline\":\"要点\"}}]}}\n\
             JSON 后可跟一句简短说明。"
        )
    } else {
        format!(
            "You are Nove Work’s outline engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total length: {chapter_count} chapters—pace this batch within that arc (setup/rising/climax/resolution); do not treat the batch as a standalone short story.\n\
             Task: write outlines for chapters in {from}–{to} that need generation (numbers: {nums_s}).\n\
             Each chapter needs a concrete title and key beats; keep continuity; no chapters outside the list.\n\
             First line MUST be JSON only: {{\"outlines\":[{{\"n\":1,\"label\":\"Chapter 1 · title\",\"outline\":\"beats\"}}]}}\n\
             Then a short note is OK."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn gen_chapter_cards_user(loc: PromptLocale, from: u32, to: u32, nums: &[u32]) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if loc.is_zh() {
        format!("请生成第 {from}–{to} 章中以下编号的章节卡大纲：{nums_s}。")
    } else {
        format!("Generate chapter-card outlines for numbers: {nums_s} (range {from}–{to}).")
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
            "你是 Nove Work 剧情拆解引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章：拆解剧情时考虑本章在全书中的位置，支线体量要匹配总篇幅。\n\
             任务：根据给定章节的大纲，拆成多张「剧情卡」，并整理涉及人物。\n\
             规则：\n\
             - 每张剧情卡对应大纲中的一条相对独立的情节线/事件/冲突，通常 2–6 张，勿空洞重复；\n\
             - 列出该剧情涉及的人物；树上已有同名人物不要改名，只需在 characters 里用相同 label；\n\
             - 缺失人物必须给出可写入人物卡的字段（role/personality 等可简短）；\n\
             - 不要输出与本章无关的剧情。\n\
             回复第一行必须是 JSON：{{\"plots\":[{{\"label\":\"剧情标题\",\"outline\":\"剧情要点\",\"characters\":[{{\"label\":\"人名\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}]}}\n\
             JSON 后可跟一句简短说明。"
        )
    } else {
        format!(
            "You are Nove Work’s plot-breakdown engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total: {chapter_count} chapters—size side plots for this chapter’s place in that arc.\n\
             Task: split the chapter outline into multiple plot cards and list involved characters.\n\
             Rules: 2–6 concrete plot cards; reuse existing character labels when names match; fill missing character fields; stay on-chapter.\n\
             First line MUST be JSON: {{\"plots\":[{{\"label\":\"title\",\"outline\":\"beats\",\"characters\":[{{\"label\":\"name\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}]}}\n\
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
    if loc.is_zh() {
        format!(
            "请为第{chapter_num}章「{label}」生成剧情卡。\n\
             本章大纲：\n{outline}\n\n\
             树上已有人物：\n{chars}\n\n\
             按大纲拆多张剧情卡，补齐缺失人物并写入 JSON。"
        )
    } else {
        format!(
            "Generate plot cards for chapter {chapter_num} “{label}”.\n\
             Outline:\n{outline}\n\n\
             Existing characters:\n{chars}\n\n\
             Output the plots JSON."
        )
    }
}

pub fn knowledge_card_extract_system(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "你是小说写作知识提炼助手。根据用户的提取需求与知识库抽样，提炼出「写作时必须遵守」的主要特征。\n\
         输出简洁 Markdown 要点（可含：文风技法、叙事节奏、人物塑造习惯、世界观/设定边界、禁忌与可模仿句式等）。\n\
         不要复述大段原文，不要客套。"
    } else {
        "You distill novel-writing knowledge. From the user’s extract request and knowledge-base samples, list features writing MUST obey.\n\
         Concise Markdown bullets (craft, pacing, characterization habits, setting bounds, taboos, imitable patterns).\n\
         Do not paste long source text. No pleasantries."
    }
}

pub fn knowledge_card_extract_user(
    loc: PromptLocale,
    extract_prompt: &str,
    corpus: &str,
) -> String {
    if loc.is_zh() {
        format!("提取需求：\n{extract_prompt}\n\n知识库抽样：\n{corpus}")
    } else {
        format!("Extract request:\n{extract_prompt}\n\nKnowledge samples:\n{corpus}")
    }
}

pub fn card_chat_system(
    loc: PromptLocale,
    kind: &str, // "chapter" | "character" | "side_plot"
    title: &str,
    label: &str,
    outline: &str,
) -> String {
    let body = if loc.is_zh() {
        match kind {
            "chapter" => format!(
                "你是 Nove Work 章节卡助手。小说《{title}》。当前章节卡标题「{label}」，大纲：{outline}\n\
                 若用户消息是指令 /完善剧情，由应用直接处理，你不会收到该指令。\n\
                 根据用户描述补全本章信息。回复第一行必须是 JSON：{{\"label\":\"章节标题\",\"outline\":\"本章剧情要点\"}}\n\
                 然后用自然语言简短说明你改了什么。"
            ),
            "character" => format!(
                "你是 Nove Work 人物卡助手。小说《{title}》。当前人物「{label}」。\n\
                 根据用户描述补全角色卡。回复第一行必须是 JSON：\
                 {{\"character\":{{\"label\":\"姓名\",\"role\":\"主角/配角…\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}}}\n\
                 然后用自然语言简短说明。"
            ),
            _ => format!(
                "你是 Nove Work 剧情卡助手。小说《{title}》。当前剧情卡「{label}」，概要：{outline}\n\
                 根据用户描述补全支线/剧情卡。回复第一行必须是 JSON：{{\"label\":\"标题\",\"outline\":\"剧情要点\"}}\n\
                 然后用自然语言简短说明。"
            ),
        }
    } else {
        match kind {
            "chapter" => format!(
                "You are the Nove Work chapter-card assistant. Novel “{title}”. Card title “{label}”, outline: {outline}\n\
                 If the user sends /enrich-plots, the app handles it — you will not receive that command.\n\
                 Fill the card from the user. First line MUST be JSON: {{\"label\":\"chapter title\",\"outline\":\"chapter beats\"}}\n\
                 Then briefly explain what you changed."
            ),
            "character" => format!(
                "You are the Nove Work character-card assistant. Novel “{title}”. Character “{label}”.\n\
                 Fill the card from the user. First line MUST be JSON: \
                 {{\"character\":{{\"label\":\"name\",\"role\":\"protagonist/…\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}}}\n\
                 Then briefly explain."
            ),
            _ => format!(
                "You are the Nove Work plot-card assistant. Novel “{title}”. Plot card “{label}”, summary: {outline}\n\
                 Fill the card from the user. First line MUST be JSON: {{\"label\":\"title\",\"outline\":\"plot beats\"}}\n\
                 Then briefly explain."
            ),
        }
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn default_new_character(loc: PromptLocale) -> (&'static str, &'static str, &'static str) {
    if loc.is_zh() {
        ("新角色", "配角", "中立")
    } else {
        ("New character", "supporting", "neutral")
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
