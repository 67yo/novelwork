//! 小说功能选项：题材 / 核心玩法 / 风格 / 关系 / 受众（写作与世界观生成用中文标签）。
use crate::models::NovelFeatures;

fn label(id: &str) -> &str {
    match id {
        "urban" => "都市",
        "xuanhuan" => "玄幻",
        "xianxia" => "仙侠",
        "scifi" => "科幻",
        "western_fantasy" => "西幻",
        "history" => "历史",
        "military" => "军事",
        "mystery" => "悬疑",
        "supernatural" => "灵异",
        "game" => "游戏",
        "apocalypse" => "末世",
        "system" => "系统",
        "rebirth" => "重生",
        "transmigration" => "穿越",
        "infinite_flow" => "无限流",
        "farming" => "种田经营",
        "lord_building" => "领主建设",
        "tycoon" => "神豪",
        "cautious" => "苟道",
        "academy" => "学院",
        "rule_horror" => "规则怪谈",
        "dungeon" => "副本挑战",
        "many_children" => "多子多福",
        "cool" => "爽文",
        "funny" => "搞笑",
        "hotblood" => "热血",
        "dark" => "黑暗",
        "light" => "轻松",
        "healing" => "治愈",
        "horror" => "恐怖",
        "realistic" => "现实",
        "ensemble" => "群像",
        "epic" => "史诗",
        "single_heroine" => "单女主",
        "multi_heroine" => "多女主",
        "no_cp" => "无CP",
        "dual_heroine" => "双女主",
        "harem" => "后宫",
        "pure_love" => "纯爱",
        "male" => "男频",
        "female" => "女频",
        other => other,
    }
}

fn join_labels(ids: &[String]) -> String {
    ids.iter()
        .map(|s| label(s.trim()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("、")
}

pub fn features_is_empty(f: &NovelFeatures) -> bool {
    f.genres.is_empty()
        && f.core_play.is_empty()
        && f.styles.is_empty()
        && f.relationships.is_empty()
        && f.audiences.is_empty()
}

/// 世界观 / 写作 Prompt 用的功能选项块（中文）。
pub fn format_novel_features_block(f: &NovelFeatures) -> String {
    if features_is_empty(f) {
        return "（未配置功能选项）".into();
    }
    let mut lines = Vec::new();
    if !f.genres.is_empty() {
        lines.push(format!("题材：{}", join_labels(&f.genres)));
    }
    if !f.core_play.is_empty() {
        lines.push(format!("核心玩法：{}", join_labels(&f.core_play)));
    }
    if !f.styles.is_empty() {
        lines.push(format!("风格：{}", join_labels(&f.styles)));
    }
    if !f.relationships.is_empty() {
        lines.push(format!("关系：{}", join_labels(&f.relationships)));
    }
    if !f.audiences.is_empty() {
        lines.push(format!("受众：{}", join_labels(&f.audiences)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_preset_and_custom() {
        let f = NovelFeatures {
            genres: vec!["urban".into(), "赛博修真".into()],
            core_play: vec!["system".into()],
            styles: vec![],
            relationships: vec!["no_cp".into()],
            audiences: vec!["male".into()],
        };
        let s = format_novel_features_block(&f);
        assert!(s.contains("题材：都市、赛博修真"));
        assert!(s.contains("核心玩法：系统"));
        assert!(s.contains("关系：无CP"));
        assert!(s.contains("受众：男频"));
        assert!(!s.contains("风格"));
    }
}
