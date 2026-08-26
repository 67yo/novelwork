//! 人物卡：写作注入时拼完整结构化人设。
use crate::models::CharacterCard;

pub const CHARACTER_INJECT_CAP: usize = 4500;

fn push_section(lines: &mut Vec<String>, title: &str, body: &str) {
    let b = body.trim();
    if b.is_empty() {
        return;
    }
    lines.push(format!("## {title}"));
    lines.push(b.to_string());
    lines.push(String::new());
}

fn push_labeled(lines: &mut Vec<String>, title: &str, items: &[(&str, &str)]) {
    let bits: Vec<String> = items
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| format!("- {k}：{}", v.trim()))
        .collect();
    if bits.is_empty() {
        return;
    }
    lines.push(format!("### {title}"));
    lines.extend(bits);
    lines.push(String::new());
}

fn verdict_label(v: &str) -> &'static str {
    match v.trim() {
        "prove" | "终将证立" | "证立" => "终将证立",
        "disprove" | "终将证伪" | "证伪" => "终将证伪",
        "unresolved" | "悬而未决" | "未决" => "悬而未决",
        _ => "",
    }
}

/// 完整人设正文；优先结构化字段，旧卡仍可读扁平字段。
pub fn format_character_full(label: &str, c: &CharacterCard) -> String {
    let mut lines: Vec<String> = Vec::new();
    let name = label.trim();
    if !name.is_empty() {
        push_section(&mut lines, "真名", name);
    }
    push_section(&mut lines, "别称", &c.aliases);
    push_section(&mut lines, "性别", &c.gender);
    push_section(&mut lines, "年龄", &c.age);

    let wp = &c.world_position;
    let social = if !wp.social_role.trim().is_empty() {
        wp.social_role.as_str()
    } else {
        c.role.as_str()
    };
    let faction = if !wp.faction.trim().is_empty() {
        wp.faction.as_str()
    } else {
        c.alignment.as_str()
    };
    if !wp.birth_class.trim().is_empty()
        || !faction.trim().is_empty()
        || !social.trim().is_empty()
        || !wp.conflict_stance.trim().is_empty()
    {
        lines.push("## 世界位置".into());
        if !wp.birth_class.trim().is_empty() {
            lines.push(format!("- 出生阶层：{}", wp.birth_class.trim()));
        }
        if !faction.trim().is_empty() {
            lines.push(format!("- 阵营归属：{}", faction.trim()));
        }
        if !social.trim().is_empty() {
            lines.push(format!("- 社会角色：{}", social.trim()));
        }
        if !wp.conflict_stance.trim().is_empty() {
            lines.push(format!("- 核心冲突立场：{}", wp.conflict_stance.trim()));
        }
        lines.push(String::new());
    }

    let wa = &c.world_anchors;
    if !wa.embodies_law.trim().is_empty()
        || !wa.embodies_note.trim().is_empty()
        || !wa.shaped_by_law.trim().is_empty()
        || !wa.shaped_by_note.trim().is_empty()
        || !wa.will_challenge.trim().is_empty()
    {
        lines.push("## 世界锚点".into());
        let emb: Vec<&str> = [wa.embodies_law.trim(), wa.embodies_note.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !emb.is_empty() {
            lines.push(format!("- 体现法则：{}", emb.join(" — ")));
        }
        let sh: Vec<&str> = [wa.shaped_by_law.trim(), wa.shaped_by_note.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !sh.is_empty() {
            lines.push(format!("- 受塑于：{}", sh.join(" — ")));
        }
        if !wa.will_challenge.trim().is_empty() {
            lines.push(format!("- 或将挑战：{}", wa.will_challenge.trim()));
        }
        lines.push(String::new());
    }

    let rels: Vec<_> = c
        .relations
        .iter()
        .filter(|r| {
            !r.name.trim().is_empty()
                || !r.relation.trim().is_empty()
                || !r.definition.trim().is_empty()
                || !r.default_attitude.trim().is_empty()
                || !r.hidden_tension.trim().is_empty()
        })
        .collect();
    if !rels.is_empty() {
        lines.push("## 关系网络".into());
        for (i, r) in rels.iter().enumerate() {
            let title = if r.name.trim().is_empty() {
                format!("{}", i + 1)
            } else {
                r.name.trim().to_string()
            };
            lines.push(format!("### {title}"));
            if !r.relation.trim().is_empty() {
                lines.push(format!("- 关系：{}", r.relation.trim()));
            }
            if !r.definition.trim().is_empty() {
                lines.push(format!("- 对角色的定义：{}", r.definition.trim()));
            }
            if !r.default_attitude.trim().is_empty() {
                lines.push(format!("- 默认态度：{}", r.default_attitude.trim()));
            }
            if !r.hidden_tension.trim().is_empty() {
                lines.push(format!("- 隐藏张力：{}", r.hidden_tension.trim()));
            }
            lines.push(String::new());
        }
    }

    let cb = &c.core_belief;
    let belief = if !cb.belief.trim().is_empty() {
        cb.belief.as_str()
    } else {
        c.motto.as_str()
    };
    let verd = verdict_label(&cb.author_verdict);
    if !belief.trim().is_empty() || !verd.is_empty() || !cb.source.trim().is_empty() {
        lines.push("## 核心信念".into());
        if !belief.trim().is_empty() {
            lines.push(format!("- 核心信念：{}", belief.trim()));
        }
        if !verd.is_empty() {
            lines.push(format!("- 作者视角：{verd}"));
        }
        if !cb.source.trim().is_empty() {
            lines.push(format!("- 信念来源：{}", cb.source.trim()));
        }
        lines.push(String::new());
    }

    let dp = &c.deep;
    let deep_start = lines.len();
    lines.push("## 深层维度".into());
    push_labeled(
        &mut lines,
        "核心欲望",
        &[
            ("表层目标", &dp.desire_surface),
            ("深层渴望", &dp.desire_deep),
        ],
    );
    push_labeled(
        &mut lines,
        "深层恐惧",
        &[("恐惧", &dp.fear), ("来源", &dp.fear_source)],
    );
    push_labeled(
        &mut lines,
        "秘密",
        &[
            ("内容", &dp.secret_content),
            ("谁知道", &dp.secret_who_knows),
            ("该知但未知道", &dp.secret_should_know),
            ("暴露后果", &dp.secret_exposure),
        ],
    );
    push_labeled(
        &mut lines,
        "底线",
        &[
            ("触线条件", &dp.line_trigger),
            ("来源", &dp.line_source),
            ("越线反应", &dp.line_reaction),
        ],
    );
    push_labeled(
        &mut lines,
        "关键创伤",
        &[
            ("伤口", &dp.trauma_wound),
            ("触发器", &dp.trauma_trigger),
            ("应激反应", &dp.trauma_stress),
            ("行为烙印", &dp.trauma_imprint),
        ],
    );
    push_labeled(
        &mut lines,
        "内在矛盾",
        &[
            ("冲突两级", &dp.contradiction_poles),
            ("来源", &dp.contradiction_source),
            ("可能走向", &dp.contradiction_trajectory),
        ],
    );
    push_labeled(
        &mut lines,
        "人物弧光",
        &[
            ("成长向", &dp.arc_growth),
            ("堕落向", &dp.arc_fall),
            ("关键抉择", &dp.arc_choice),
        ],
    );
    if lines.len() == deep_start + 1 {
        lines.pop();
        if !c.personality.trim().is_empty() {
            push_section(&mut lines, "性格", &c.personality);
        }
    }

    let vo = &c.voice;
    let positioning = if !vo.positioning.trim().is_empty() {
        vo.positioning.as_str()
    } else {
        c.style.as_str()
    };
    let has_voice = !positioning.trim().is_empty()
        || !vo.cognitive_filter.trim().is_empty()
        || !vo.sentence_length.trim().is_empty()
        || !vo.pause.trim().is_empty()
        || !vo.patterns.trim().is_empty()
        || vo.catchphrases.iter().any(|p| !p.trim().is_empty())
        || !vo.emotion_anger.trim().is_empty()
        || !vo.emotion_tense.trim().is_empty()
        || !vo.emotion_mask.trim().is_empty()
        || !vo.emotion_sad.trim().is_empty()
        || !vo.emotion_happy.trim().is_empty()
        || !vo.banned.trim().is_empty();
    if has_voice {
        lines.push("## 角色声线".into());
        if !positioning.trim().is_empty() {
            lines.push("### 声音定位".into());
            lines.push(positioning.trim().to_string());
            lines.push(String::new());
        }
        if !vo.cognitive_filter.trim().is_empty() {
            lines.push("### 认知滤镜".into());
            lines.push(vo.cognitive_filter.trim().to_string());
            lines.push(String::new());
        }
        let mut syn = Vec::new();
        if !vo.sentence_length.trim().is_empty() {
            syn.push(format!("- 长短句偏好：{}", vo.sentence_length.trim()));
        }
        if !vo.pause.trim().is_empty() {
            syn.push(format!("- 停顿习惯：{}", vo.pause.trim()));
        }
        if !vo.patterns.trim().is_empty() {
            syn.push(format!("- 常用句式：{}", vo.patterns.trim()));
        }
        for p in &vo.catchphrases {
            if !p.trim().is_empty() {
                syn.push(format!("- 口头禅：{}", p.trim()));
            }
        }
        if !syn.is_empty() {
            lines.push("### 句法指纹".into());
            lines.extend(syn);
            lines.push(String::new());
        }
        push_labeled(
            &mut lines,
            "情绪变化",
            &[
                ("愤怒时", &vo.emotion_anger),
                ("紧张时", &vo.emotion_tense),
                ("掩饰时", &vo.emotion_mask),
                ("伤心时", &vo.emotion_sad),
                ("开心时", &vo.emotion_happy),
            ],
        );
        if !vo.banned.trim().is_empty() {
            lines.push("### 禁用表达（不会说的话）".into());
            lines.push(vo.banned.trim().to_string());
            lines.push(String::new());
        }
    }

    let body = lines.join("\n").trim().to_string();
    if !body.is_empty() {
        return body;
    }
    // 极旧卡：constraints 即全文
    c.constraints.trim().to_string()
}

/// 结构化字段 → 旧扁平字段 + constraints 全文（与前端 syncLegacyFields 对齐）
pub fn sync_character_legacy(label: &str, c: &mut CharacterCard) {
    if !c.world_position.social_role.trim().is_empty() {
        c.role = c.world_position.social_role.clone();
    } else if !c.role.trim().is_empty() {
        c.world_position.social_role = c.role.clone();
    }
    if !c.world_position.faction.trim().is_empty() {
        c.alignment = c.world_position.faction.clone();
    } else if !c.alignment.trim().is_empty() {
        c.world_position.faction = c.alignment.clone();
    }
    if !c.core_belief.belief.trim().is_empty() {
        c.motto = c.core_belief.belief.clone();
    }
    if !c.voice.positioning.trim().is_empty() {
        c.style = c.voice.positioning.clone();
    }
    let formatted = format_character_full(label, c);
    if !formatted.is_empty() {
        c.constraints = formatted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    #[test]
    fn formats_rich_card() {
        let c = CharacterCard {
            gender: "男".into(),
            age: "中年".into(),
            aliases: "老李".into(),
            world_position: CharacterWorldPosition {
                birth_class: "贱籍".into(),
                faction: "商会".into(),
                social_role: "账房".into(),
                conflict_stance: "反对".into(),
            },
            core_belief: CharacterCoreBelief {
                belief: "钱能买命".into(),
                author_verdict: "disprove".into(),
                source: "".into(),
            },
            ..Default::default()
        };
        let md = format_character_full("李四", &c);
        assert!(md.contains("真名"));
        assert!(md.contains("李四"));
        assert!(md.contains("账房"));
        assert!(md.contains("终将证伪"));
    }
}
