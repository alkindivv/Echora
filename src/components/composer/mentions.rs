//! The reference starts mentions at a line boundary, whitespace or opening
//! bracket. A dismissed token stays closed while typing; moving the caret can
//! discover it again. References and IME preedit are never mention queries.
use super::ComposerView;
use gpui::Context;
use std::ops::Range;

pub struct InspectInputReference(pub crate::agent::AgentInputReference);
impl gpui::EventEmitter<InspectInputReference> for ComposerView {}

fn token(text: &str, caret: usize) -> Option<Range<usize>> {
    let prefix = text.get(..caret)?;
    let line = prefix.rsplit('\n').next()?;
    let at = line.rfind('@')?;
    if line[..at]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{'))
    {
        return None;
    }
    Some(prefix.len() - line.len() + at..caret)
}

impl ComposerView {
    pub(super) fn update_mentions(&mut self, selection_changed: bool, cx: &mut Context<Self>) {
        if self.context_menu_open && self.context_catalog.mention_start.is_none() {
            self.update_context_query(cx);
            return;
        }
        let editor = self.prompt_editor.read(cx);
        let candidate = (!editor.composing() && editor.selection_is_empty())
            .then(|| token(editor.text(), editor.cursor()))
            .flatten()
            .filter(|range| {
                !editor
                    .references()
                    .iter()
                    .any(|r| r.range.start < range.end && range.start < r.range.end)
            });
        let Some(range) = candidate else {
            if self.context_menu_open {
                self.dismiss_context(cx);
            }
            self.context_catalog.mention_start = None;
            self.context_catalog.mention_dismissed = None;
            return;
        };
        if selection_changed {
            self.context_catalog.mention_dismissed = None;
        }
        if !self.context_menu_open && self.context_catalog.mention_dismissed == Some(range.start) {
            return;
        }
        if !self.context_menu_open || self.context_catalog.mention_start != Some(range.start) {
            self.context_menu_open = true;
            self.menu_open = false;
            self.permission_menu_open = false;
            self.slash_menu = None;
            self.load_context_catalog(cx);
            self.context_catalog.mention_start = Some(range.start);
            self.context_catalog.start = range.start + 1;
        }
        self.update_context_query(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::token;
    #[test]
    fn mention_boundaries_match_reference_without_email_or_nested_at() {
        for text in [
            "@", "hello @", "中文 @", "line\n@", "(@", "[@", "{@", "\t@", "@PDF ", "@PDF xxx",
        ] {
            let r = token(text, text.len()).unwrap();
            assert!(text[r].starts_with('@'), "{text}");
        }
        for text in [
            "hello@",
            "email@example.com",
            "中文@",
            ":@",
            "/@",
            "-@",
            "@@",
            "@foo@",
        ] {
            assert!(token(text, text.len()).is_none(), "{text}");
        }
        assert_eq!(token("before @PDF after", 11), Some(7..11));
        assert!(token("中@", 1).is_none());
    }
}
