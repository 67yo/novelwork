//! Pre-generate land constraints: build must-land beats, keyword-check, list gaps.
//! ponytail: keyword overlap ≠ true comprehension; upgrade to LLM judge if false negatives hurt.

use crate::chapter_memory::{query_terms, score_memory};

#[derive(Debug, Clone)]
pub struct LandBeat {
    /// `outline` | `plot` | `character`
    pub kind: &'static str,
    pub title: String,
    pub text: String,
}

/// Split chapter outline into ordered beats (more than a single blob when possible).
pub fn outline_beats(outline: &str) -> Vec<String> {
    let t = outline.trim();
    if t.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = t
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(|c: char| {
                    c.is_ascii_digit()
                        || c == '.'
                        || c == ')'
                        || c == '、'
                        || c == '．'
                        || c == '）'
                        || c == '-'
                        || c == '•'
                        || c == '*'
                })
                .trim()
                .to_string()
        })
        .filter(|l| l.chars().count() >= 4)
        .collect();
    if lines.len() >= 2 {
        lines.truncate(10);
        return lines;
    }
    // Single paragraph: split on strong punctuation
    let parts: Vec<String> = t
        .split(|c| "。；;！？!?\n".contains(c))
        .map(|s| s.trim().to_string())
        .filter(|s| s.chars().count() >= 6)
        .collect();
    if parts.len() >= 2 {
        return parts.into_iter().take(8).collect();
    }
    vec![t.chars().take(200).collect()]
}

pub fn format_contract(zh: bool, beats: &[LandBeat]) -> String {
    if beats.is_empty() {
        return if zh {
            "【必须落地】（本章无额外剧情卡/大纲节拍，仍须合理推进。）\n".into()
        } else {
            "[Must land] (no extra plot/outline beats — still advance the chapter.)\n".into()
        };
    }
    let mut out = if zh {
        String::from(
            "【必须落地——正文中须实际发生或明确推进，禁止只点名】\n",
        )
    } else {
        String::from("[Must land — must occur or clearly advance in the body; no name-dropping]\n")
    };
    for (i, b) in beats.iter().enumerate() {
        let tag = match b.kind {
            "plot" => {
                if zh {
                    "剧情卡"
                } else {
                    "plot"
                }
            }
            "character" => {
                if zh {
                    "人物"
                } else {
                    "cast"
                }
            }
            _ => {
                if zh {
                    "大纲"
                } else {
                    "outline"
                }
            }
        };
        out.push_str(&format!("{}. [{}] {} — {}\n", i + 1, tag, b.title, b.text));
    }
    out
}

/// True if body likely covers this beat (keyword overlap).
pub fn beat_landed(body: &str, beat: &LandBeat) -> bool {
    let hay = body.to_lowercase();
    // Character: name must appear
    if beat.kind == "character" {
        let name = beat.title.trim();
        if name.is_empty() {
            return true;
        }
        let name_l = name.to_lowercase();
        if hay.contains(&name_l) {
            return true;
        }
        // short given-name fallback (2 CJK chars)
        let prefix: String = name.chars().take(2).collect();
        return prefix.chars().count() >= 2 && hay.contains(&prefix.to_lowercase());
    }
    // Plot: title hit, or a 3-gram from the latter part of the text (skip leading subject noise)
    if beat.kind == "plot" {
        let title = beat.title.trim();
        if title.chars().count() >= 2 && hay.contains(&title.to_lowercase()) {
            return true;
        }
        let cjk: Vec<char> = beat
            .text
            .chars()
            .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
            .collect();
        if cjk.len() < 3 {
            let terms = query_terms(&beat.text);
            return !terms.is_empty() && score_memory(body, &terms) >= 1;
        }
        let start = cjk.len() / 4;
        return cjk[start..]
            .windows(3)
            .any(|w| hay.contains(&w.iter().collect::<String>()));
    }
    // Outline beats
    let terms = query_terms(&beat.text);
    if terms.is_empty() {
        return true;
    }
    let score = score_memory(body, &terms);
    let need = if terms.len() >= 6 { 2 } else { 1 };
    score >= need
}

pub fn missing_beats<'a>(body: &str, beats: &'a [LandBeat]) -> Vec<&'a LandBeat> {
    beats.iter().filter(|b| !beat_landed(body, b)).collect()
}

pub fn format_missing(zh: bool, missing: &[&LandBeat]) -> String {
    let mut out = if zh {
        String::from("【未落地项——须补写入正文】\n")
    } else {
        String::from("[Missing — weave into the body]\n")
    };
    for (i, b) in missing.iter().enumerate() {
        out.push_str(&format!("{}. {} — {}\n", i + 1, b.title, b.text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_splits_lines() {
        let b = outline_beats("1. 发现密信\n2. 与乙对峙\n3. 逃离雾港");
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn detects_missing_plot() {
        let beats = vec![LandBeat {
            kind: "plot",
            title: "旧地图".into(),
            text: "主角在雾港发现旧地图并约定出海".into(),
        }];
        let miss = missing_beats("主角在茶馆喝了一杯茶，什么也没发生。", &beats);
        assert_eq!(miss.len(), 1);
        let ok = missing_beats("主角在雾港发现旧地图，与人约定出海寻人。", &beats);
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn character_name_check() {
        let beats = vec![LandBeat {
            kind: "character",
            title: "林潮".into(),
            text: "克制隐忍".into(),
        }];
        assert!(missing_beats("林潮推开木门。", &beats).is_empty());
        assert_eq!(missing_beats("有人推开木门。", &beats).len(), 1);
    }
}
