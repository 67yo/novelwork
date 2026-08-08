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
         ## 书名\n## 作者\n## 类型标签\n（用顿号分隔 1–5 个短标签；优先从用户提供的可选标签中选；没有合适的可新增 2–6 字标签）\n\
         ## 章节大纲\n（按目录逐章：章名 + 一两句情节要点；目录很长时可合并卷/篇）\n\
         ## 写作手法\n（叙事视角、节奏、文风措辞、人物塑造、章法结构、伏笔与转折习惯等，写具体可模仿的要点）\n\
         不要写客套话。"
    } else {
        "You are a novel knowledge-base analyst. From the TOC and text samples, output Markdown with these H2 sections:\n\
         ## Title\n## Author\n## Genre tags\n(1–5 short tags separated by commas; prefer the provided optional tags; invent a short new tag only if none fit)\n\
         ## Chapter outline\n(one short beat per chapter; merge volumes if the TOC is huge)\n\
         ## Writing craft\n(POV, pacing, diction, characterization, structure, foreshadowing habits—concrete and imitable)\n\
         No pleasantries."
    }
}

/// Analyze a web page into a reusable knowledge pack for writing.
pub fn knowledge_url_extract_system(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "你是知识库分析助手。根据网页正文，输出 Markdown，必须包含这些小节（用二级标题）：\n\
         ## 书名\n（页面/资料名称）\n\
         ## 作者\n（原作者或站点）\n\
         ## 类型标签\n（用顿号分隔 1–5 个短标签；优先从可选标签中选；没有合适的可新增 2–6 字标签）\n\
         ## 章节大纲\n（按主题模块列出要点：设定、规则、人物/势力、事件流程、专有名词等；每项一两句）\n\
         ## 写作手法\n（此处写「知识要点」：可被小说严格遵守的事实、规则边界、专有名词、禁忌与不可违背设定）\n\
         不要写客套话。"
    } else {
        "You are a knowledge-base analyst. From the web page text, output Markdown with these H2 sections:\n\
         ## Title\n(page / material name)\n\
         ## Author\n(creator or site)\n\
         ## Genre tags\n(1–5 short tags; prefer provided tags; invent a short new tag only if none fit)\n\
         ## Chapter outline\n(topic modules: setting, rules, characters/factions, event flow, proper nouns—one short beat each)\n\
         ## Writing craft\n(put knowledge constraints here: hard facts, rule boundaries, proper nouns, taboos writers must follow)\n\
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
    known_genres: &str,
) -> String {
    if loc.is_zh() {
        format!(
            "提取需求：{extract_prompt}\n\n已知书名：{title}\n已知作者：{author}\n可选类型标签（优先选用）：{known_genres}\n\n目录：\n{toc}\n\n正文抽样：\n{samples}"
        )
    } else {
        format!(
            "Extract focus: {extract_prompt}\n\nKnown title: {title}\nKnown author: {author}\nOptional genre tags (prefer these): {known_genres}\n\nTOC:\n{toc}\n\nSamples:\n{samples}"
        )
    }
}

pub fn default_knowledge_book_prompt(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "提取书名、作者、类型标签、各章大纲要点，并总结作者的写作手法与可模仿技巧。"
    } else {
        "Extract title, author, genre tags, chapter outline beats, and the author's writing craft."
    }
}

pub fn default_knowledge_url_prompt(loc: PromptLocale) -> &'static str {
    if loc.is_zh() {
        "从网页内容提炼结构化知识：主题概要、关键设定与规则、人物或势力、专有名词、流程与禁忌；整理为可被写作严格参考的知识库要点。"
    } else {
        "From the page, extract structured knowledge: topic summary, key settings and rules, characters or factions, proper nouns, flows and taboos—usable as a writing knowledge base."
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

fn is_genre_heading(line: &str) -> bool {
    matches!(
        line,
        "## 类型标签"
            | "## 类型"
            | "## 题材"
            | "## Genre tags"
            | "## Genres"
            | "## Genre"
    ) || line.starts_with("## 类型标签")
        || line.starts_with("## Genre tags")
}

/// Parse genre tags from analysis markdown (`## 类型标签` / `## Genre tags`).
pub fn extract_genres_from_analysis(analysis: &str) -> Vec<String> {
    let mut expect = false;
    for line in analysis.lines() {
        let trimmed = line.trim();
        if is_genre_heading(trimmed) {
            // inline: "## 类型标签：玄幻、系统" or "## 类型标签 玄幻"
            let rest = trimmed
                .trim_start_matches('#')
                .trim()
                .trim_start_matches("类型标签")
                .trim_start_matches("类型")
                .trim_start_matches("题材")
                .trim_start_matches("Genre tags")
                .trim_start_matches("Genres")
                .trim_start_matches("Genre")
                .trim()
                .trim_start_matches(['：', ':'])
                .trim();
            if !rest.is_empty() {
                return split_genre_tokens(rest);
            }
            expect = true;
            continue;
        }
        if expect {
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                break;
            }
            return split_genre_tokens(trimmed.trim_start_matches(['*', '-', ' ']));
        }
    }
    Vec::new()
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
    fn extracts_genre_line_after_heading() {
        let md = "## 书名\n雾港\n## 类型标签\n玄幻、系统、重生\n## 章节大纲\nx";
        assert_eq!(
            extract_genres_from_analysis(md),
            vec!["玄幻", "系统", "重生"]
        );
    }

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
            "你是 Nove Work 开书助手。通过简短对话帮用户确定：书名、简介、主要角色、每章目标字数范围、全书计划总章数。\n\
             必须询问：每章大约多少字（如 2000–3000），以及这本小说大概写多少章（如 20、30）。不要现在生成章节大纲或章节列表。\n\
             信息不足时用一两句追问。信息足够，或用户说「创建/生成/开始」时，先用一两句确认，\
             然后输出完整 JSON（可多行或代码块）：\n\
             {{\"create\":{{\"title\":\"\",\"synopsis\":\"\",\"knowledge_strategy\":\"\",\"word_count_min\":2000,\"word_count_max\":3000,\"chapter_count\":20,\"characters\":[{{\"label\":\"\",\"role\":\"主角\",\"gender\":\"\",\"personality\":\"\",\"motto\":\"\",\"style\":\"\",\"alignment\":\"正派\"}}]}}}}\n\
             characters 至少主角；word_count_min/max 为每章目标字数；chapter_count 为全书计划章数。\n\
             若用户消息附带【网页正文】，以正文为准作答，勿臆造页面未提供的内容。{force_hint}"
        )
    } else {
        format!(
            "You are the Nove Work novel-creation assistant. Through short dialogue, settle: title, synopsis, main characters, target words per chapter, and planned total chapter count.\n\
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
            plots_header: "【链接剧情卡——关键情节点必须在本章正文中实际发生或明确推进，不可只提一句带过】",
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
            plots_header: "[Linked plot cards — key beats MUST occur or clearly advance in this chapter body; do not name-drop only]",
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
    let wmin_lo = wmin.saturating_sub(60);
    let wmax_hi = wmax.saturating_add(60);
    let body = if loc.is_zh() {
        format!(
            "你是强约束小说写作引擎 Nove Work。生成本章时必须同时遵守：\n\
             1）全书故事简介与根节点关联人物卡（总体设定参考，勿偏离）；\n\
             2）前序章节的大纲与章节记忆（若有）：已是既定事实与因果，本章必须衔接，严禁推翻、改写或无视记忆中的人物状态/承诺/事件结果；\n\
             3）本章默认大纲（主线走向，不可丢弃关键情节点）；\n\
             4）本章已链接人物卡（性格、身份、行事风格、关系，言行不得出戏）；\n\
             5）本章已链接剧情卡（含根节点贯穿剧情）：卡内要点/补充正文是本章情节来源之一，须结合大纲写入正文，使事件落地，禁止只点名不推进。\n\
             6）已链接知识卡（提取特征与知识约束必须遵守，勿与之矛盾）。\n\
             7）全书计划共 {chapter_count} 章：本章信息量与悬念投放须符合所处位置，勿按无限连载节奏注水；勿抢写后续章大纲中才应发生的高潮。\n\
             未链接到本章的设定不要硬塞；冲突时：前序记忆与大纲定历史事实 → 本章大纲定主线 → 剧情卡补齐本章事件 → 人物约束行为 → 知识卡定边界 → 简介定调。\n\
             【契约】用户消息中的「必须落地」列表是硬性验收项：每一项都须在正文中实际发生或明确推进；写完在脑中逐项勾选，缺项则补写后再输出。\n\
             小说：《{title}》\n简介：{synopsis}\n知识库策略：{knowledge_strategy}\n\
             【硬性篇幅】以树根节点每章目标为准：正文非空白字符数必须落在 {wmin}–{wmax} 字，允许误差不超过 60 字\
             （有效区间 {wmin_lo}–{wmax_hi}）。上下限相同时按该单点目标 ±60。写完自行点数，偏短续写、偏长删冗，达标再交卷。"
        )
    } else {
        format!(
            "You are Nove Work, a strongly constrained novel-writing engine. When generating this chapter you MUST obey:\n\
             1) Novel synopsis and root-linked character cards (global setting — do not contradict);\n\
             2) Prior chapter outlines + chapter memory (if any): established facts/causality—this chapter must continue them; never overturn, rewrite, or ignore remembered states/promises/outcomes;\n\
             3) The default chapter outline (main arc — do not drop key beats);\n\
             4) Chapter-linked character cards (traits, role, style — stay in character);\n\
             5) Linked plot cards (incl. root book-wide plots): their beats/extra text are chapter plot sources—combine with the outline and make events land in the body; do not name-drop without advancing them.\n\
             6) Linked knowledge cards (extracted features/constraints — do not contradict).\n\
             7) Planned total: {chapter_count} chapters—pace for this place in the arc; do not steal climaxes reserved for later chapter outlines.\n\
             Do not force unlinked lore. On conflicts: prior memory/outlines = history → this chapter outline drives the arc → plot cards supply events → characters constrain behavior → knowledge cards bound craft → synopsis sets tone.\n\
             [Contract] The “Must land” list in the user message is hard acceptance criteria—each item must occur or clearly advance; mentally check every item and fill gaps before outputting.\n\
             Novel: “{title}”\nSynopsis: {synopsis}\nKnowledge strategy: {knowledge_strategy}\n\
             [Hard length] Root per-chapter target: non-whitespace count MUST be in {wmin}–{wmax}, tolerance ≤60 \
             (allowed band {wmin_lo}–{wmax_hi}). If min=max, that single target ±60. Count before submit; expand if short, trim if long."
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
             请在衔接前序记忆/大纲与预生成条件的前提下，严格按「必须落地」列表生成本章正文（Markdown）。\n\
             篇幅硬约束：非空白字数 {wmin}–{wmax}，误差 ≤60（有效 {wmin_lo}–{wmax_hi}）。\n\
             只输出正文，不要输出清单或自我评分。缺项或字数超差禁止交卷。"
        )
    } else {
        format!(
            "{root_ref}\n\n{brief}{hist}{chapter_info}\n\n{cards}\n\n{contract}\n\
             Continue prior memory/outlines and generate conditions, then write this chapter’s body (Markdown) covering every Must-land item.\n\
             Hard length: {wmin}–{wmax} non-whitespace chars, tolerance ≤60 (band {wmin_lo}–{wmax_hi}).\n\
             Output body only — no checklist or self-score. Do not submit with gaps or out-of-band length."
        )
    }
}

pub fn repair_chapter_system(loc: PromptLocale, wmin: u32, wmax: u32) -> String {
    let wmin_lo = wmin.saturating_sub(60);
    let wmax_hi = wmax.saturating_add(60);
    let body = if loc.is_zh() {
        format!(
            "你是 Nove Work 约束补写引擎。任务：在不大改已有情节骨架的前提下，把「未落地项」自然织入当前章正文。\n\
             禁止另起炉灶；禁止删除已有合理段落；可增补场景/对白/过渡使缺项发生。\n\
             字数硬约束：非空白 {wmin}–{wmax}，误差 ≤60（有效 {wmin_lo}–{wmax_hi}）。只输出完整正文 Markdown。"
        )
    } else {
        format!(
            "You are Nove Work’s constraint-repair engine. Weave every Missing item into the current chapter without overhauling the plot skeleton.\n\
             Do not restart the chapter; do not delete sound existing passages; add scenes/dialogue/transitions so gaps land.\n\
             Hard length: {wmin}–{wmax} non-whitespace, tolerance ≤60 (band {wmin_lo}–{wmax_hi}). Output full chapter Markdown only."
        )
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
        format!(" ⚠字数 {words} 仍偏离目标 {wmin}–{wmax}（有效 {lo}–{hi}）。")
    } else {
        format!(" ⚠ Length {words} still off target {wmin}–{wmax} (band {lo}–{hi}).")
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
                "你是 Nove Work 章节精修引擎。小说：《{title}》。\n\
                 【精修目的】不大改剧情：在已生成正文的情节骨架上做小幅梳理与润色；必须结合「参考前 N 章」的完整正文来核对历史因果，再梳理当前章，使衔接合理、人物关系正确、结构正常、语句通顺；禁止另起炉灶或大幅改写主线。\n\
                 【必须同时使用的材料】\n\
                 1）参考前 {linked_n} 章的完整正文（标题+大纲+正文；只作历史依据，禁止改写这些历史章）；\n\
                 2）当前章全部链接卡片（大纲、人物卡、剧情卡、知识卡、人物关系等；剧情卡要点应已在正文中落地，缺漏则小幅补写）；\n\
                 3）当前章已生成的完整正文（精修对象：尽量保留原有情节与段落顺序，只改与历史矛盾、不合理、不通顺之处）。\n\
                 【必须确保】当前章与历史章节衔接合理；链接剧情卡关键情节不缺失；无异常/突兀情节；无剧情错误与时间线矛盾；人物关系与卡面一致；结构清楚；语句通顺。\n\
                 【硬性篇幅】以树根节点每章目标为准：非空白字数必须落在 {wmin}–{wmax}，误差不超过 60 字（有效 {wmin_lo}–{wmax_hi}）。精修后必须仍在有效区间内。"
            )
        } else {
            format!(
                "你是 Nove Work 章节精修引擎。小说：《{title}》。\n\
                 【精修目的】不大改剧情：在已生成正文的情节骨架上做小幅梳理与润色；必须结合「本小说章节记忆库」中与前 {linked_n} 章相关的核心事实，以及各章大纲，来核对历史因果，再梳理当前章，使衔接合理、人物关系正确、结构正常、语句通顺；禁止另起炉灶或大幅改写主线。\n\
                 【必须同时使用的材料】\n\
                 1）前 {linked_n} 章的大纲 + 章节记忆要点（人物出场、行为、目标、承诺、人设；只作历史依据，禁止改写历史章）；\n\
                 2）当前章全部链接卡片（大纲、人物卡、剧情卡、知识卡、人物关系等；剧情卡要点应已在正文中落地，缺漏则小幅补写）；\n\
                 3）当前章已生成的完整正文（精修对象：尽量保留原有情节与段落顺序，只改与历史矛盾、不合理、不通顺之处）。\n\
                 【必须确保】当前章与上述历史记忆/大纲衔接合理；链接剧情卡关键情节不缺失；无异常/突兀情节；无剧情错误与时间线矛盾；人物关系与卡面一致；结构清楚；语句通顺。\n\
                 【硬性篇幅】以树根节点每章目标为准：非空白字数必须落在 {wmin}–{wmax}，误差不超过 60 字（有效 {wmin_lo}–{wmax_hi}）。精修后必须仍在有效区间内。"
            )
        }
    } else if full_body {
        format!(
            "You are the Nove Work chapter refine engine. Novel: “{title}”.\n\
             [Purpose] Do not majorly rewrite the plot. Use the full text of the prior {linked_n} reference chapters to check continuity, then lightly tidy the current chapter. Do not restart or overhaul the main arc.\n\
             [Required inputs]\n\
             1) The previous {linked_n} chapters in FULL (title + outline + body — history only; never rewrite them);\n\
             2) All cards linked to the current chapter (outline, characters, plot cards, knowledge, relations; plot-card beats should already land—lightly add if missing);\n\
             3) The full already-generated current-chapter body (preserve plot beats and order; fix only contradictions with history, errors, and awkward prose).\n\
             [Must ensure] Aligns with that history; linked plot-card beats are present; no absurd/abrupt beats; no plot/timeline errors; relationships match cards; clear structure; fluent prose.\n\
             [Hard length] Root per-chapter target {wmin}–{wmax} non-whitespace chars, tolerance ≤60 (band {wmin_lo}–{wmax_hi}). Refined body MUST stay in band."
        )
    } else {
        format!(
            "You are the Nove Work chapter refine engine. Novel: “{title}”.\n\
             [Purpose] Do not majorly rewrite the plot. Use this novel’s chapter memory (facts from the prior {linked_n} chapters) plus chapter outlines to check continuity, then lightly tidy the current chapter. Do not restart or overhaul the main arc.\n\
             [Required inputs]\n\
             1) Outlines + chapter-memory facts for the previous {linked_n} chapters (characters present, actions, goals, promises, personas — history only; never rewrite those chapters);\n\
             2) All cards linked to the current chapter (outline, characters, plot cards, knowledge, relations; plot-card beats should already land—lightly add if missing);\n\
             3) The full already-generated current-chapter body (preserve plot beats and order; fix only contradictions with history, errors, and awkward prose).\n\
             [Must ensure] Aligns with memory/outlines; linked plot-card beats are present; no absurd/abrupt beats; no plot/timeline errors; relationships match cards; clear structure; fluent prose.\n\
             [Hard length] Root per-chapter target {wmin}–{wmax} non-whitespace chars, tolerance ≤60 (band {wmin_lo}–{wmax_hi}). Refined body MUST stay in band."
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
                "知识策略：{knowledge_strategy}\n\n\
                 {brief}\
                 ——— ① 参考前 {linked_n} 章完整内容（全文；勿改写历史）———\n{hist}\n\
                 ——— ② 当前章链接设定（大纲 + 全部链接卡片）———\n{chapter_info}\n\n{cards}\n\n\
                 ——— ③ 当前章已生成正文（精修对象：对照①梳理合理性，不大改剧情）———\n{current}\n\n\
                 请先通读①中各章完整正文，再对照②中的剧情卡、精修条件与③，输出精修后的完整当前章 Markdown（不要输出历史章）。改动应克制；若剧情卡要点未落地可小幅补写。\n\
                 【硬性篇幅】非空白字数 {wmin}–{wmax}，误差 ≤60（有效 {wmin_lo}–{wmax_hi}）；偏短续写、偏长删冗，达标再交卷。\n\
                 文末用列表列出本次修正点，按类归并：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。"
            )
        } else {
            format!(
                "知识策略：{knowledge_strategy}\n\n\
                 {brief}\
                 ——— ① 前 {linked_n} 章大纲 + 章节记忆（核对历史；勿改写历史）———\n{hist}\n\
                 ——— ② 当前章链接设定（大纲 + 全部链接卡片）———\n{chapter_info}\n\n{cards}\n\n\
                 ——— ③ 当前章已生成正文（精修对象：对照①梳理合理性，不大改剧情）———\n{current}\n\n\
                 请先依据①中的大纲与记忆要点核对因果与人设，再对照②中的剧情卡、精修条件与③，输出精修后的完整当前章 Markdown（不要输出历史章）。改动应克制；若剧情卡要点未落地可小幅补写。\n\
                 【硬性篇幅】非空白字数 {wmin}–{wmax}，误差 ≤60（有效 {wmin_lo}–{wmax_hi}）；偏短续写、偏长删冗，达标再交卷。\n\
                 文末用列表列出本次修正点，按类归并：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。"
            )
        }
    } else if full_body {
        format!(
            "Knowledge strategy: {knowledge_strategy}\n\n\
             {brief}\
             ——— ① Prior {linked_n} chapters in FULL (history only; do not rewrite) ———\n{hist}\n\
             ——— ② Current chapter linked setup (outline + all linked cards) ———\n{chapter_info}\n\n{cards}\n\n\
             ——— ③ Current chapter body (refine against ①; do not overhaul plot) ———\n{current}\n\n\
             Read every prior chapter body in ① first, then revise ③ against ①, ② (incl. plot cards), and refine conditions. Output only the refined current chapter in Markdown. Lightly add missing plot-card beats if needed.\n\
             [Hard length] {wmin}–{wmax} non-whitespace chars, tolerance ≤60 (band {wmin_lo}–{wmax_hi}); expand if short, trim if long.\n\
             End with a bullet list of fixes by category: continuity / plot-card landing / plot errors / relationships / structure / prose / length."
        )
    } else {
        format!(
            "Knowledge strategy: {knowledge_strategy}\n\n\
             {brief}\
             ——— ① Prior {linked_n} chapters: outlines + chapter memory (history only; do not rewrite) ———\n{hist}\n\
             ——— ② Current chapter linked setup (outline + all linked cards) ———\n{chapter_info}\n\n{cards}\n\n\
             ——— ③ Current chapter body (refine against ①; do not overhaul plot) ———\n{current}\n\n\
             Use outlines and memory in ①, then revise ③ against ①, ② (incl. plot cards), and refine conditions. Output only the refined current chapter in Markdown. Lightly add missing plot-card beats if needed.\n\
             [Hard length] {wmin}–{wmax} non-whitespace chars, tolerance ≤60 (band {wmin_lo}–{wmax_hi}); expand if short, trim if long.\n\
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

pub fn workspace_chat_system(
    loc: PromptLocale,
    title: &str,
    synopsis: &str,
    root_outline: &str,
    has_chapters: bool,
    wmin: u32,
    wmax: u32,
    chapter_count: u32,
    chapter_list: &str,
) -> String {
    let root_note = if root_outline.trim().is_empty() {
        if loc.is_zh() {
            "（空）"
        } else {
            "(empty)"
        }
    } else {
        root_outline
    };
    let body = if loc.is_zh() {
        let status = if has_chapters { "已有" } else { "尚无" };
        format!(
            "你是 Nove Work 创作助手。当前小说《{title}》。简介：{synopsis}\n\
             根节点全书大纲/说明（可改）：{root_note}\n\
             树图现状：{status}章节节点；每章目标字数约 {wmin}–{wmax}；全书计划共 {chapter_count} 章（后续剧情与大纲须按此总篇幅分配节奏，勿写成无限连载）。\n\
             【根节点斜杠指令由系统直接执行，用户以 / 开头发送；你无需伪造这些操作的 JSON】\n\
             · /章节卡 1-10（冲突时：/章节卡 覆盖|跳过|强制追加 1-10）\n\
             · /添加剧情 3 剧情内容\n\
             · /剧情卡 3\n\
             · /清空章节\n\
             若用户要求修改根节点大纲/全书说明，回复中输出 JSON：{{\"root_outline\":\"完整大纲正文\"}}（覆盖写入根节点，勿只改简介字段）。\n\
             若用户要求修改每章目标字数，回复中输出 JSON：{{\"word_count\":{{\"min\":4500,\"max\":4500}}}}（单点目标时 min=max）。\n\
             若用户要求修改全书计划章数，回复中输出 JSON：{{\"chapter_count\":30}}。\n\
             若用户描述新角色，回复中输出 JSON：{{\"character\":{{\"label\":\"\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\",\"link_chapter_id\":\"可选章节节点id\"}}}}\n\
             若用户要求生成/重写若干章的标题与大纲（非斜杠指令），回复中输出 JSON：{{\"outlines\":[{{\"n\":1,\"label\":\"第一章 · 具体标题\",\"outline\":\"本章剧情要点\"}}],\"mode\":\"overwrite\"}}\n\
             mode 用 overwrite（同号覆盖标题+大纲，推荐用于「重新生成第X–Y章」）或 append（强制追加新节点）；缺省按同号覆盖、缺号追加。\n\
             以上 JSON 均可多行或放在代码块中。\n\
             然后再用自然语言正常对话。可用章节节点：{chapter_list}\n\
             若用户消息附带【网页正文】，以正文为准作答，勿臆造页面未提供的内容。"
        )
    } else {
        let status = if has_chapters { "has" } else { "has no" };
        format!(
            "You are the Nove Work writing assistant. Novel “{title}”. Synopsis: {synopsis}\n\
             Root overall outline/notes (editable): {root_note}\n\
             Tree status: {status} chapter nodes; target ≈ {wmin}–{wmax} words per chapter; planned total {chapter_count} chapters (pace all plots/outlines to this length—not endless serialization).\n\
             [Root slash commands are handled by the app — do not invent JSON for them]\n\
             · /chapters 1-10 (conflict: /chapters overwrite|skip|force 1-10)\n\
             · /add-plot 3 plot text\n\
             · /plots 3\n\
             · /clear-chapters\n\
             If the user wants to change the root overall outline/notes, include JSON: {{\"root_outline\":\"full outline text\"}} (overwrites the root node; do not use synopsis for this).\n\
             If the user changes the per-chapter word target, include JSON: {{\"word_count\":{{\"min\":4500,\"max\":4500}}}} (use min=max for a single target).\n\
             If the user changes the planned total chapters, include JSON: {{\"chapter_count\":30}}.\n\
             If the user describes a new character, include JSON: {{\"character\":{{\"label\":\"\",\"role\":\"\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\",\"link_chapter_id\":\"optional chapter node id\"}}}}\n\
             If free-form chat should create/rewrite chapter titles+outlines (not slash commands), include JSON: {{\"outlines\":[{{\"n\":1,\"label\":\"Chapter 1 · title\",\"outline\":\"beats\"}}],\"mode\":\"overwrite\"}}\n\
             mode=overwrite replaces same-number title+outline (use for “regenerate chapters X–Y”); mode=append force-adds nodes; default upserts by chapter number.\n\
             JSON may be multi-line or fenced.\n\
             Then continue in natural language. Available chapter nodes: {chapter_list}\n\
             If the user message includes fetched page text, rely on it; do not invent page content."
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
    let body = if loc.is_zh() {
        format!(
            "你是 Nove Work 大纲引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章：请按这一总篇幅分配本批章节在整体故事中的位置与信息量（开篇/发展/高潮/收束），勿把本批写成与总章数无关的独立短篇。\n\
             {replace_note}\n\
             任务：为第 {from}–{to} 章范围内需要生成的章节撰写大纲（编号列表：{nums_s}）。\n\
             要求：每章标题具体、大纲含关键情节点；章与章衔接合理；不要输出范围外的章。\n\
             回复中必须包含完整 JSON（可多行或代码块）：{{\"outlines\":[{{\"n\":1,\"label\":\"第一章 · 标题\",\"outline\":\"要点\"}}]}}\n\
             JSON 后可跟一句简短说明。"
        )
    } else {
        format!(
            "You are Nove Work’s outline engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total length: {chapter_count} chapters—pace this batch within that arc (setup/rising/climax/resolution); do not treat the batch as a standalone short story.\n\
             {replace_note}\n\
             Task: write outlines for chapters in {from}–{to} that need generation (numbers: {nums_s}).\n\
             Each chapter needs a concrete title and key beats; keep continuity; no chapters outside the list.\n\
             Reply MUST include complete JSON (multi-line or fenced OK): {{\"outlines\":[{{\"n\":1,\"label\":\"Chapter 1 · title\",\"outline\":\"beats\"}}]}}\n\
             Then a short note is OK."
        )
    };
    format!("{body}\n{}", loc.language_rule())
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
        format!("{mat}请生成第 {from}–{to} 章中以下编号的章节卡（含 label 标题与 outline 大纲）：{nums_s}。不得推翻上述既定事实。")
    } else {
        format!("{mat}Generate chapter cards (label + outline) for numbers: {nums_s} (range {from}–{to}). Do not overturn established facts above.")
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
    let body = if loc.is_zh() {
        format!(
            "你是 Nove Work 续章大纲引擎。小说《{title}》。简介：{synopsis}\n\
             全书计划共 {chapter_count} 章。任务：根据「当前章全文 + 历史记忆 + 根大纲 + 人物设定」，\
             为第 {from}–{to} 章（编号：{nums_s}）撰写剧情大纲（标题+要点），顺着既有情节往前推。\n\
             规则：\n\
             - 承接当前章结尾的局势与悬念，不得推翻记忆中的既定事实；\n\
             - 多章时章与章衔接递进，节奏匹配全书总章数中的位置；\n\
             - 人物言行符合设定；可呼应知识库/写作策略中的风格约束；\n\
             - 只输出本批编号，同号视为覆盖重写标题与大纲。\n\
             回复中必须包含完整 JSON（可多行或代码块）：\
             {{\"outlines\":[{{\"n\":{from},\"label\":\"第{from}章 · 标题\",\"outline\":\"情节点\"}}]}}\n\
             JSON 后可跟一句简短说明。"
        )
    } else {
        format!(
            "You are Nove Work’s next-chapter outline engine. Novel “{title}”. Synopsis: {synopsis}\n\
             Planned total: {chapter_count} chapters. Task: from current chapter body + memory + root outline + characters, \
             write plot outlines (title + beats) for chapters {from}–{to} (numbers: {nums_s}).\n\
             Rules: continue from the current chapter’s ending; do not overturn memory facts; \
             multi-chapter batches must progress coherently within the planned length; respect character cards and knowledge strategy; \
             only listed numbers; same numbers overwrite title/outline.\n\
             Reply MUST include complete JSON (multi-line/fenced OK): \
             {{\"outlines\":[{{\"n\":{from},\"label\":\"Chapter {from} · title\",\"outline\":\"beats\"}}]}}\n\
             Then a short note is OK."
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
    knowledge_strategy: &str,
) -> String {
    let nums_s = nums
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let ks = if knowledge_strategy.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("写作策略 / 知识库约束：\n{knowledge_strategy}\n\n")
    } else {
        format!("Writing / knowledge strategy:\n{knowledge_strategy}\n\n")
    };
    let mat = if consideration.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("【须考虑的材料与期望（以用户编辑为准）】\n{consideration}\n\n")
    } else {
        format!("[Materials & expectations (user-edited)]\n{consideration}\n\n")
    };
    if loc.is_zh() {
        format!(
            "{ks}{mat}请生成第 {from}–{to} 章大纲（编号 {nums_s}），写入 outlines JSON。不得推翻上述既定事实；承接当前局势与期望。"
        )
    } else {
        format!(
            "{ks}{mat}Generate outlines for chapters {from}–{to} (numbers {nums_s}) as outlines JSON. Do not overturn facts; honor current situation and expectations."
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
            "你是 Nove Work 剧情拆解引擎。小说《{title}》。简介：{synopsis}\n\
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
            "You are Nove Work’s plot-breakdown engine. Novel “{title}”. Synopsis: {synopsis}\n\
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
                 用户可要求修改章节名称（标题）和/或本章大纲。回复中输出完整 JSON（可多行或代码块）：{{\"label\":\"章节标题\",\"outline\":\"本章剧情要点\"}}\n\
                 只改名称时仍输出完整 JSON（outline 可保持原文）；只改大纲时 label 可保持原标题；均为覆盖写入。\n\
                 然后用自然语言简短说明你改了什么。"
            ),
            "character" => format!(
                "你是 Nove Work 人物卡助手。小说《{title}》。当前人物「{label}」。\n\
                 根据用户描述补全角色卡。回复中输出完整 JSON（可多行或代码块）：\
                 {{\"character\":{{\"label\":\"姓名\",\"role\":\"主角/配角…\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}}}\n\
                 然后用自然语言简短说明。"
            ),
            _ => format!(
                "你是 Nove Work 剧情卡助手。小说《{title}》。当前剧情卡「{label}」，概要：{outline}\n\
                 根据用户描述补全支线/剧情卡。回复中输出完整 JSON（可多行或代码块）：{{\"label\":\"标题\",\"outline\":\"剧情要点\"}}\n\
                 然后用自然语言简短说明。"
            ),
        }
    } else {
        match kind {
            "chapter" => format!(
                "You are the Nove Work chapter-card assistant. Novel “{title}”. Card title “{label}”, outline: {outline}\n\
                 If the user sends /enrich-plots, the app handles it — you will not receive that command.\n\
                 The user may ask to rename the chapter and/or change the outline. Include complete JSON (multi-line or fenced OK): {{\"label\":\"chapter title\",\"outline\":\"chapter beats\"}}\n\
                 If only renaming, still emit full JSON (keep the existing outline); if only outline changes, keep the existing label; both fields overwrite.\n\
                 Then briefly explain what you changed."
            ),
            "character" => format!(
                "You are the Nove Work character-card assistant. Novel “{title}”. Character “{label}”.\n\
                 Fill the card from the user. Include complete JSON (multi-line or fenced OK): \
                 {{\"character\":{{\"label\":\"name\",\"role\":\"protagonist/…\",\"personality\":\"\",\"motto\":\"\",\"gender\":\"\",\"style\":\"\",\"alignment\":\"\"}}}}\n\
                 Then briefly explain."
            ),
            _ => format!(
                "You are the Nove Work plot-card assistant. Novel “{title}”. Plot card “{label}”, summary: {outline}\n\
                 Fill the card from the user. Include complete JSON (multi-line or fenced OK): {{\"label\":\"title\",\"outline\":\"plot beats\"}}\n\
                 Then briefly explain."
            ),
        }
    };
    let web = if loc.is_zh() {
        "\n若用户消息附带【网页正文】，以正文为准作答，勿臆造页面未提供的内容。"
    } else {
        "\nIf the user message includes fetched page text, rely on it; do not invent page content."
    };
    format!("{body}{web}\n{}", loc.language_rule())
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
