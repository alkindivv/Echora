//! Native project symbols and the Unicode Emoji 17.0 search catalog.
use crate::{
    theme::{Theme, ThemeMode},
    workspace::ProjectMarker,
};
use gpui::{Div, Hsla, div, prelude::*, px, rgba, svg};

pub(crate) const ICONS: [(&str, &str, &str); 30] = [
    ("folder", "文件夹", "Folder"),
    ("dollar", "美元", "Dollar"),
    ("book", "书", "Book"),
    ("graduation-cap", "毕业帽", "Graduation cap"),
    ("pencil", "铅笔", "Pencil"),
    ("writing", "写作", "Writing"),
    ("code", "代码括号", "Code brackets"),
    ("terminal", "终端", "Terminal"),
    ("music", "音乐", "Music"),
    ("popcorn", "爆米花", "Popcorn"),
    ("custom", "自定义", "Custom"),
    ("palette", "调色板", "Palette"),
    ("stethoscope", "听诊器", "Stethoscope"),
    ("health", "健康", "Health"),
    ("lotus", "莲花", "Lotus"),
    ("suitcase", "行李箱", "Suitcase"),
    ("bar-chart", "条形图", "Bar chart"),
    ("kettlebell", "壶铃", "Kettlebell"),
    ("dumbbell", "哑铃", "Dumbbell"),
    ("journal", "日志", "Journal"),
    ("scales", "天平", "Scales"),
    ("earth-spin", "地球旋转", "Earth spin"),
    ("plane", "飞机", "Plane"),
    ("globe", "地球仪", "Globe"),
    ("wrench", "扳手", "Wrench"),
    ("paw", "爪印", "Paw"),
    ("flask", "烧瓶", "Flask"),
    ("brain", "大脑", "Brain"),
    ("heart", "心形", "Heart"),
    ("plant", "植物", "Plant"),
];
pub(crate) const EMOJI_CATEGORIES: [(&str, &str); 10] = [
    ("推荐", "Recommended"),
    ("笑脸与情绪", "Smileys & Emotion"),
    ("人物与身体", "People & Body"),
    ("动物与自然", "Animals & Nature"),
    ("食物和饮料", "Food & Drink"),
    ("旅行与地点", "Travel & Places"),
    ("活动", "Activities"),
    ("物品", "Objects"),
    ("符号", "Symbols"),
    ("旗帜", "Flags"),
];
pub(crate) const RECOMMENDED_EMOJIS: [&str; 8] = ["😄", "👍", "❤️", "🎉", "🚀", "👀", "🙏", "🔥"];
pub(crate) const COLOR_NAMES: [(&str, &str); 8] = [
    ("默认", "Default"),
    ("红色", "Red"),
    ("橙色", "Orange"),
    ("黄色", "Yellow"),
    ("绿色", "Green"),
    ("蓝色", "Blue"),
    ("紫色", "Purple"),
    ("粉色", "Pink"),
];
pub(crate) fn color(index: usize, mode: ThemeMode) -> Hsla {
    let colors = match mode {
        ThemeMode::Dark => [
            0xffffff99, 0xff6764ff, 0xff8549ff, 0xffd240ff, 0x40c977ff, 0x339cffff, 0xad7bf9ff,
            0xff8cc1ff,
        ],
        ThemeMode::Light => [
            0x1a1c1f99, 0xfa423eff, 0xfb6a22ff, 0xffc300ff, 0x04b84cff, 0x0285ffff, 0x924ff7ff,
            0xff66adff,
        ],
    };
    rgba(colors[index.min(7)]).into()
}
pub(crate) fn glyph(marker: Option<&ProjectMarker>, theme: Theme, size: f32) -> Div {
    let d = div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size));
    if let Some(emoji) = marker.and_then(|m| m.emoji.as_ref()) {
        return d.child(emoji.clone());
    }
    let name = marker
        .map(|m| m.icon.as_str())
        .filter(|n| ICONS.iter().any(|(id, _, _)| id == n))
        .unwrap_or("folder");
    let color = marker
        .map(|m| {
            color(
                m.color,
                if theme.text.r > 0.5 {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
            )
        })
        .unwrap_or(theme.text_tertiary.into());
    d.child(
        svg()
            .path(format!("icons/project-marker-{name}.svg"))
            .size(px(size))
            .text_color(color),
    )
}
#[derive(Clone, serde::Deserialize)]
pub(crate) struct Emoji {
    pub(crate) emoji: String,
    pub(crate) name: String,
    pub(crate) group: String,
}
pub(crate) fn emojis() -> &'static [Emoji] {
    static CATALOG: std::sync::OnceLock<Vec<Emoji>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/project-emoji.json"))
            .expect("bundled Unicode emoji catalog")
    })
}
