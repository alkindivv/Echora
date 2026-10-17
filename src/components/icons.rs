use gpui::{Hsla, Svg, prelude::*, px, svg};

pub fn icon(name: &'static str, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(16.0))
        .text_color(color)
}

/// Package artwork retains its colors. SVGs explicitly authored with
/// `currentColor` use the surrounding foreground, as they do in the reference.
pub fn package_icon(path: String, color: Hsla) -> gpui::AnyElement {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
    };
    static MASKS: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let mut masks = MASKS.get_or_init(Default::default).lock().unwrap();
    let monochrome = *masks.entry(path.clone()).or_insert_with(|| {
        if !path.ends_with(".svg") || !std::fs::metadata(&path).is_ok_and(|m| m.len() <= 262144) {
            return false;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            return false;
        };
        if !source.contains("currentColor")
            || source.contains("style=")
            || source.contains("<style")
        {
            return false;
        }
        ["fill=", "stroke="].iter().all(|attribute| {
            source.split(attribute).skip(1).all(|tail| {
                let Some(quote) = tail.chars().next().filter(|c| matches!(c, '\'' | '"')) else {
                    return false;
                };
                matches!(tail[1..].split(quote).next(), Some("currentColor" | "none"))
            })
        })
    });
    drop(masks);
    if monochrome {
        svg()
            .path(path)
            .size(px(16.))
            .text_color(color)
            .into_any_element()
    } else {
        use gpui::StyledImage;
        gpui::img(path)
            .size(px(16.))
            .with_fallback(move || icon("command-init", color).into_any_element())
            .into_any_element()
    }
}
