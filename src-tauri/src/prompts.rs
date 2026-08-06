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
        "你是小说知识库分析助手。根据用户提取需求，用两行输出：书名：xxx\\n作者：xxx"
    } else {
        "You are a novel knowledge-base analyst. From the user's extract request, output exactly two lines:\nTitle: xxx\nAuthor: xxx"
    }
}

pub fn knowledge_extract_user(loc: PromptLocale, extract_prompt: &str, excerpt: &str) -> String {
    if loc.is_zh() {
        format!("提取需求：{extract_prompt}\n\n正文开头：\n{excerpt}")
    } else {
        format!("Extract focus: {extract_prompt}\n\nText opening:\n{excerpt}")
    }
}

pub fn parse_title_author(loc: PromptLocale, line: &str) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut author = None;
    if loc.is_zh() {
        if let Some(v) = line
            .strip_prefix("书名：")
            .or_else(|| line.strip_prefix("书名:"))
        {
            title = Some(v.trim().to_string());
        }
        if let Some(v) = line
            .strip_prefix("作者：")
            .or_else(|| line.strip_prefix("作者:"))
        {
            author = Some(v.trim().to_string());
        }
    }
    if let Some(v) = line
        .strip_prefix("Title:")
        .or_else(|| line.strip_prefix("title:"))
    {
        title = Some(v.trim().to_string());
    }
    if let Some(v) = line
        .strip_prefix("Author:")
        .or_else(|| line.strip_prefix("author:"))
    {
        author = Some(v.trim().to_string());
    }
    // Always accept Chinese labels too (model may mix)
    if title.is_none() {
        if let Some(v) = line
            .strip_prefix("书名：")
            .or_else(|| line.strip_prefix("书名:"))
        {
            title = Some(v.trim().to_string());
        }
    }
    if author.is_none() {
        if let Some(v) = line
            .strip_prefix("作者：")
            .or_else(|| line.strip_prefix("作者:"))
        {
            author = Some(v.trim().to_string());
        }
    }
    (title, author)
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
            "你是 Nove Work 开书助手。通过简短对话帮用户确定：书名、简介、主要角色、每章目标字数范围。\n\
             必须询问用户每章大约多少字（如 2000–3000）。不要现在生成章节大纲或章节列表。\n\
             信息不足时用一两句追问。信息足够，或用户说「创建/生成/开始」时，先用一两句确认，\
             然后单独一行输出 JSON（不要包在代码块里）：\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"characters\":[{{\"label\":\"\",\"role\":\"主角\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"正派\"}}]}}}}\n\
             characters 至少主角；word_count_min/max 为每章目标字数。{force_hint}"
        )
    } else {
        format!(
            "You are the Nove Work novel-creation assistant. Through short dialogue, settle: title, synopsis, main characters, and target words per chapter.\n\
             Always ask for approximate words per chapter (e.g. 2000–3000). Do NOT generate chapter outlines or chapter lists yet.\n\
             If info is missing, ask one or two brief follow-ups. When enough—or the user says create/generate/start—confirm in one or two sentences, \
             then output a single JSON line (no code fence):\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"characters\":[{{\"label\":\"\",\"role\":\"protagonist\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"\"}}]}}}}\n\
             Include at least the protagonist; word_count_min/max are per-chapter targets.{force_hint}"
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
            empty_outline: "（本章大纲为空，请仅依据已链接人物/剧情与小说简介合理推进，勿偏离已有设定）",
            chapter_title: "章节标题",
            chapter_outline: "本章默认大纲（必须遵循的主线走向）",
            chars_header: "【链接人物卡——人物言行必须符合】",
            relations_header: "【人物关系——互动与称呼须符合】",
            relation_unset: "（关系未标注）",
            plots_header: "【链接剧情卡——须融入本章或与之呼应】",
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
            chars_header: "[Linked character cards — speech and actions must match]",
            relations_header: "[Character relations — interactions must match]",
            relation_unset: "(relation not set)",
            plots_header: "[Linked plot cards — weave in or echo in this chapter]",
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
) -> String {
    let body = if loc.is_zh() {
        format!(
            "你是强约束小说写作引擎 Nove Work。生成本章时必须同时遵守：\n\
             1）本章默认大纲（主线走向，不可丢弃关键情节点）；\n\
             2）已链接人物卡（性格、身份、行事风格、关系，言行不得出戏）；\n\
             3）已链接剧情卡（支线要点须自然融入或与本章呼应）。\n\
             未链接的设定不要硬塞；大纲/人物/剧情三者冲突时以大纲为主线，人物为行为约束，剧情为支线补强。\n\
             小说：《{title}》\n简介：{synopsis}\n知识库策略：{knowledge_strategy}\n\
             【硬性篇幅】本章正文非空白字符数必须落在 {wmin}–{wmax} 字；若上下限相同，尽量贴近该目标（可略超，禁止明显偏短）。写不够请继续写到接近目标再结束。"
        )
    } else {
        format!(
            "You are Nove Work, a strongly constrained novel-writing engine. When generating this chapter you MUST obey:\n\
             1) The default chapter outline (main arc — do not drop key beats);\n\
             2) Linked character cards (traits, role, style — stay in character);\n\
             3) Linked plot cards (side-plot beats must be woven in or echoed).\n\
             Do not force unlinked lore. On conflicts: outline drives the arc, characters constrain behavior, plots reinforce side threads.\n\
             Novel: “{title}”\nSynopsis: {synopsis}\nKnowledge strategy: {knowledge_strategy}\n\
             [Hard length] Non-whitespace character count MUST land in {wmin}–{wmax}. If min equals max, stay near that target (slightly over OK; clearly short is not)."
        )
    };
    format!("{body}\n{}", loc.language_rule())
}

pub fn generate_chapter_user(loc: PromptLocale, chapter_info: &str, cards: &str, wmin: u32, wmax: u32) -> String {
    if loc.is_zh() {
        format!(
            "{chapter_info}\n\n{cards}\n\n请生成本章正文（Markdown），篇幅目标 {wmin}–{wmax} 字（非空白）。写完后自检：大纲要点、链接人物、链接剧情，以及字数是否达标。"
        )
    } else {
        format!(
            "{chapter_info}\n\n{cards}\n\nWrite this chapter’s body (Markdown), length target {wmin}–{wmax} non-whitespace characters. Then self-check outline, linked characters, linked plots, and length."
        )
    }
}

pub fn generate_footer(loc: PromptLocale, node_id: &str, model: &str) -> String {
    if loc.is_zh() {
        format!(
            "\n\n---\n> 节点约束校验摘要：已按大纲、人物卡与剧情卡生成。章节节点 `{node_id}`。模型：{model}。\n"
        )
    } else {
        format!(
            "\n\n---\n> Constraint check: generated from outline, character cards, and plot cards. Node `{node_id}`. Model: {model}.\n"
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
            "你是 Nove Work 精修引擎。小说：《{title}》。\n\
             任务：对照【本章默认大纲】【链接人物卡】【链接剧情卡】，以及前面已生成的 {linked_n} 章正文，改写当前章。\n\
             重点：逻辑矛盾、人物言行与卡面不符、已链接剧情未呼应、前后情节不通、时间线错乱、阅读不流畅。\n\
             本章目标字数约 {wmin}–{wmax} 字。保留原有风格与关键剧情推进，不要无故另起炉灶。精修后篇幅仍须贴近该目标。"
        )
    } else {
        format!(
            "You are the Nove Work refine engine. Novel: “{title}”.\n\
             Task: rewrite the current chapter against [default outline], [linked character cards], [linked plot cards], and the previous {linked_n} generated chapters.\n\
             Focus: logic flaws, character OOC, missing linked-plot echoes, plot holes, timeline errors, poor flow.\n\
             Target ≈ {wmin}–{wmax} words. Keep style and key progression; do not restart from scratch without cause. Keep length near the target after refining."
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
    if loc.is_zh() {
        format!(
            "知识策略：{knowledge_strategy}\n\n\
             ——— 关联前文章节（共 {linked_n} 章）———\n{prev_text}\n\
             ——— 当前章设定（大纲 + 链接人物 + 链接剧情）———\n{chapter_info}\n\n{cards}\n\n\
             ——— 当前章正文（待精修）———\n{current}\n\n\
             请输出精修后的完整章节 Markdown；文末用列表列出修正点（大纲/人物/剧情/连贯/流畅）。"
        )
    } else {
        format!(
            "Knowledge strategy: {knowledge_strategy}\n\n\
             ——— Prior chapters ({linked_n}) ———\n{prev_text}\n\
             ——— Current chapter setup (outline + linked characters + linked plots) ———\n{chapter_info}\n\n{cards}\n\n\
             ——— Current body (to refine) ———\n{current}\n\n\
             Output the full refined chapter in Markdown; end with a bullet list of fixes (outline / characters / plots / coherence / flow)."
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
    chapter_list: &str,
) -> String {
    let body = if loc.is_zh() {
        let status = if has_chapters { "已有" } else { "尚无" };
        format!(
            "你是 Nove Work 创作助手。当前小说《{title}》。简介：{synopsis}\n\
             树图现状：{status}章节节点；每章目标字数约 {wmin}–{wmax}。\n\
             若用户要求修改每章目标字数，回复第一行用 JSON：{{\"word_count\":{{\"min\":4500,\"max\":4500}}}}（单点目标时 min=max）。\n\
             若用户描述新角色，回复第一行用 JSON：{{\"character\":{{\"label\":\"\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\",\"link_chapter_id\":\"可选章节节点id\"}}}}\n\
             若用户决定章节内容并要求生成大纲（或信息已够写大纲），回复第一行用 JSON：{{\"outlines\":[{{\"label\":\"第一章 · 具体标题\",\"outline\":\"本章剧情要点\"}}]}}\n\
             然后再用自然语言正常对话。可用章节节点：{chapter_list}"
        )
    } else {
        let status = if has_chapters { "has" } else { "has no" };
        format!(
            "You are the Nove Work writing assistant. Novel “{title}”. Synopsis: {synopsis}\n\
             Tree status: {status} chapter nodes; target ≈ {wmin}–{wmax} words per chapter.\n\
             If the user changes the per-chapter word target, first line MUST be JSON: {{\"word_count\":{{\"min\":4500,\"max\":4500}}}} (use min=max for a single target).\n\
             If the user describes a new character, first line MUST be JSON: {{\"character\":{{\"label\":\"\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\",\"link_chapter_id\":\"optional chapter node id\"}}}}\n\
             If the user settles chapter content and wants outlines (or enough info exists), first line MUST be JSON: {{\"outlines\":[{{\"label\":\"Chapter 1 · concrete title\",\"outline\":\"beats for this chapter\"}}]}}\n\
             Then continue in natural language. Available chapter nodes: {chapter_list}"
        )
    };
    format!("{body}\n{}", loc.language_rule())
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
        format!("## {label}\n大纲：{outline}\n正文：\n{body}\n\n")
    } else {
        format!("## {label}\nOutline: {outline}\nBody:\n{body}\n\n")
    }
}
