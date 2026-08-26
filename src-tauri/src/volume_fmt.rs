//! 分卷结构化设定 → 写作/画布 outline 注入文本。
use crate::models::{VolumeKeyBeat, VolumeLayerBlock, VolumePayload};

fn push_section(lines: &mut Vec<String>, title: &str, body: &str) {
    let b = body.trim();
    if b.is_empty() {
        return;
    }
    lines.push(format!("## {title}"));
    lines.push(b.to_string());
    lines.push(String::new());
}

fn push_layer(lines: &mut Vec<String>, title: &str, layer: &VolumeLayerBlock) {
    let ch = layer.chapters.trim();
    let desc = layer.description.trim();
    if ch.is_empty() && desc.is_empty() {
        return;
    }
    lines.push(format!("## {title}"));
    if !ch.is_empty() {
        lines.push(format!("- 对应章节：{ch}"));
    }
    if !desc.is_empty() {
        lines.push(desc.to_string());
    }
    lines.push(String::new());
}

pub fn format_volume_full(p: &VolumePayload) -> String {
    let mut lines = Vec::new();
    push_section(&mut lines, "本卷定位", &p.positioning);
    let layers = [
        ("前置", &p.layer_setup),
        ("对抗", &p.layer_confrontation),
        ("收束", &p.layer_resolution),
    ];
    if layers
        .iter()
        .any(|(_, l)| !l.chapters.trim().is_empty() || !l.description.trim().is_empty())
    {
        lines.push("# 三层结构".into());
        lines.push(String::new());
        for (title, layer) in layers {
            push_layer(&mut lines, title, layer);
        }
    }
    if !p.conflict_external.trim().is_empty()
        || !p.conflict_internal.trim().is_empty()
        || !p.conflict_deep.trim().is_empty()
    {
        lines.push("# 冲突层级".into());
        lines.push(String::new());
        push_section(&mut lines, "外部对抗", &p.conflict_external);
        push_section(&mut lines, "内心矛盾", &p.conflict_internal);
        push_section(&mut lines, "深层威胁", &p.conflict_deep);
    }
    let beats: Vec<&VolumeKeyBeat> = p
        .key_beats
        .iter()
        .filter(|b| !b.cost.trim().is_empty() || !b.description.trim().is_empty())
        .collect();
    if !beats.is_empty() {
        lines.push("# 关键节点".into());
        lines.push(String::new());
        for b in beats {
            lines.push(format!("## {}", b.order));
            if !b.cost.trim().is_empty() {
                lines.push(format!("- 代价：{}", b.cost.trim()));
            }
            if !b.description.trim().is_empty() {
                lines.push(b.description.trim().to_string());
            }
            lines.push(String::new());
        }
    }
    lines.join("\n").trim().to_string()
}

/// 结构化字段是否有实质内容（不含默认空 key_beat）。
pub fn volume_payload_nonempty(p: &VolumePayload) -> bool {
    !p.positioning.trim().is_empty()
        || !p.layer_setup.chapters.trim().is_empty()
        || !p.layer_setup.description.trim().is_empty()
        || !p.layer_confrontation.chapters.trim().is_empty()
        || !p.layer_confrontation.description.trim().is_empty()
        || !p.layer_resolution.chapters.trim().is_empty()
        || !p.layer_resolution.description.trim().is_empty()
        || !p.conflict_external.trim().is_empty()
        || !p.conflict_internal.trim().is_empty()
        || !p.conflict_deep.trim().is_empty()
        || p.key_beats.iter().any(|b| !b.cost.trim().is_empty() || !b.description.trim().is_empty())
}

fn layer_from_h2(title: &str) -> Option<&'static str> {
    match title.trim() {
        "前置" => Some("setup"),
        "对抗" => Some("confrontation"),
        "收束" => Some("resolution"),
        _ => None,
    }
}

fn conflict_from_h2(title: &str) -> Option<&'static str> {
    match title.trim() {
        "外部对抗" => Some("external"),
        "内心矛盾" => Some("internal"),
        "深层威胁" => Some("deep"),
        _ => None,
    }
}

/// 从 `format_volume_full` 产出的 Markdown 反向解析；不匹配时整段落入 `positioning`。
pub fn parse_volume_from_formatted(text: &str) -> VolumePayload {
    let text = text.trim();
    let mut p = VolumePayload::default();
    if text.is_empty() {
        return p;
    }

    enum Section {
        None,
        Positioning,
        LayerSetup,
        LayerConfrontation,
        LayerResolution,
        ConflictExternal,
        ConflictInternal,
        ConflictDeep,
        KeyBeat(u32),
    }

    let mut section = Section::None;
    let mut buf: Vec<String> = Vec::new();
    let mut parsed_any = false;

    fn flush(section: &Section, buf: &mut Vec<String>, p: &mut VolumePayload, parsed_any: &mut bool) {
        let body = buf.join("\n").trim().to_string();
        buf.clear();
        if body.is_empty() {
            return;
        }
        *parsed_any = true;
        match section {
            Section::Positioning => p.positioning = body,
            Section::LayerSetup => {
                if let Some(ch) = body.strip_prefix("- 对应章节：") {
                    p.layer_setup.chapters = ch.trim().to_string();
                } else {
                    p.layer_setup.description = body;
                }
            }
            Section::LayerConfrontation => {
                if let Some(ch) = body.strip_prefix("- 对应章节：") {
                    p.layer_confrontation.chapters = ch.trim().to_string();
                } else {
                    p.layer_confrontation.description = body;
                }
            }
            Section::LayerResolution => {
                if let Some(ch) = body.strip_prefix("- 对应章节：") {
                    p.layer_resolution.chapters = ch.trim().to_string();
                } else {
                    p.layer_resolution.description = body;
                }
            }
            Section::ConflictExternal => p.conflict_external = body,
            Section::ConflictInternal => p.conflict_internal = body,
            Section::ConflictDeep => p.conflict_deep = body,
            Section::KeyBeat(order) => {
                let mut cost = String::new();
                let mut desc = body.clone();
                if let Some(rest) = body.strip_prefix("- 代价：") {
                    let (c, d) = rest.split_once('\n').unwrap_or((rest, ""));
                    cost = c.trim().to_string();
                    desc = d.trim().to_string();
                }
                p.key_beats.push(crate::models::VolumeKeyBeat {
                    order: *order,
                    cost,
                    description: desc,
                });
            }
            Section::None => {}
        }
    }

    for line in text.lines() {
        if line.starts_with("## ") {
            flush(&section, &mut buf, &mut p, &mut parsed_any);
            let title = line[3..].trim();
            section = if title == "本卷定位" {
                Section::Positioning
            } else if let Some(l) = layer_from_h2(title) {
                parsed_any = true;
                match l {
                    "setup" => Section::LayerSetup,
                    "confrontation" => Section::LayerConfrontation,
                    _ => Section::LayerResolution,
                }
            } else if let Some(c) = conflict_from_h2(title) {
                parsed_any = true;
                match c {
                    "external" => Section::ConflictExternal,
                    "internal" => Section::ConflictInternal,
                    _ => Section::ConflictDeep,
                }
            } else if let Ok(n) = title.parse::<u32>() {
                Section::KeyBeat(n)
            } else {
                Section::None
            };
            continue;
        }
        if line.starts_with("# ") {
            flush(&section, &mut buf, &mut p, &mut parsed_any);
            section = Section::None;
            continue;
        }
        if matches!(section, Section::None) {
            continue;
        }
        // 三层结构层块：章节行与正文可分行
        if matches!(
            section,
            Section::LayerSetup | Section::LayerConfrontation | Section::LayerResolution
        ) && line.starts_with("- 对应章节：")
        {
            flush(&section, &mut buf, &mut p, &mut parsed_any);
            let ch = line["- 对应章节：".len()..].trim();
            match section {
                Section::LayerSetup => p.layer_setup.chapters = ch.to_string(),
                Section::LayerConfrontation => p.layer_confrontation.chapters = ch.to_string(),
                Section::LayerResolution => p.layer_resolution.chapters = ch.to_string(),
                _ => {}
            }
            parsed_any = true;
            continue;
        }
        buf.push(line.to_string());
    }
    flush(&section, &mut buf, &mut p, &mut parsed_any);

    if !parsed_any {
        p.positioning = text.to_string();
    }
    if p.key_beats.is_empty() {
        p.key_beats.push(crate::models::VolumeKeyBeat::default());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_has_positioning() {
        let p = VolumePayload {
            positioning: "逃亡".into(),
            layer_setup: VolumeLayerBlock {
                chapters: "1-5".into(),
                description: "上路".into(),
            },
            ..Default::default()
        };
        let t = format_volume_full(&p);
        assert!(t.contains("本卷定位"));
        assert!(t.contains("1-5"));
    }

    #[test]
    fn parse_roundtrip_positioning_and_layer() {
        let p = VolumePayload {
            positioning: "主角离城".into(),
            layer_setup: VolumeLayerBlock {
                chapters: "1-10 章".into(),
                description: "启程".into(),
            },
            conflict_external: "追兵".into(),
            key_beats: vec![crate::models::VolumeKeyBeat {
                order: 1,
                cost: "受伤".into(),
                description: "救人".into(),
            }],
            ..Default::default()
        };
        let formatted = format_volume_full(&p);
        let back = parse_volume_from_formatted(&formatted);
        assert_eq!(back.positioning, "主角离城");
        assert_eq!(back.layer_setup.chapters, "1-10 章");
        assert_eq!(back.layer_setup.description, "启程");
        assert_eq!(back.conflict_external, "追兵");
        assert_eq!(back.key_beats.len(), 1);
        assert_eq!(back.key_beats[0].cost, "受伤");
        assert_eq!(back.key_beats[0].description, "救人");
    }

    #[test]
    fn parse_freeform_falls_back_to_positioning() {
        let back = parse_volume_from_formatted("根据世界观写的第一卷自由文本");
        assert_eq!(back.positioning, "根据世界观写的第一卷自由文本");
    }
}
