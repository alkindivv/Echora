//! Native file context and planning mode for side conversations.

use super::ComposerView;
use crate::{agent::AgentInputFile, components::icons::icon, theme::Theme};
use gpui::{
    Context, KeyDownEvent, PathPromptOptions, Role, Window, deferred, div, img, prelude::*, px,
};

pub struct OpenAttachmentPreview(pub std::path::PathBuf);
impl gpui::EventEmitter<OpenAttachmentPreview> for ComposerView {}

impl ComposerView {
    pub(super) fn toggle_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu_open = !self.context_menu_open;
        self.context_focus_pending = self.context_menu_open;
        self.menu_open = false;
        self.permission_menu_open = false;
        if self.context_menu_open {
            self.context_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn attach_files(&mut self, cx: &mut Context<Self>) {
        self.context_menu_open = false;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some(crate::i18n::text("添加文件或文件夹").into()),
        });
        cx.spawn(async move |this, cx| {
            let paths = selection
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.attach_paths(paths, cx);
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn attach_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        if !paths.is_empty() {
            self.draft_revision = self.draft_revision.wrapping_add(1);
        }
        for path in paths.into_iter().take(16) {
            if self.prompt_context.files.len() >= 16 {
                break;
            }
            if !self
                .prompt_context
                .files
                .iter()
                .any(|file| file.path == path)
            {
                let image = crate::media::read_image_dimensions(&path)
                    .ok()
                    .flatten()
                    .is_some();
                self.prompt_context
                    .files
                    .push(AgentInputFile { path, image });
            }
        }
        self.focus_prompt_pending = true;
        cx.notify();
    }

    pub(super) fn render_attachments(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("side-chat-attachments")
            .mb(px(6.0))
            .py(px(4.0))
            .px(px(4.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.0))
            .overflow_x_scroll()
            .children(
                self.prompt_context
                    .files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| {
                        let label = file
                            .path
                            .file_name()
                            .unwrap_or(file.path.as_os_str())
                            .to_string_lossy()
                            .into_owned();
                        let path = file.path.clone();
                        if file.image {
                            let key_path = path.clone();
                            let remove_path = path.clone();
                            let keyboard_remove_path = path.clone();
                            let group = gpui::SharedString::from(format!("composer-image-{index}"));
                            return div()
                                .id(("composer-image", index))
                                .debug_selector(move || format!("COMPOSER_IMAGE_{index}"))
                                .role(Role::Button)
                                .aria_label(crate::i18n::format!("预览图片 {label}" => "Preview image {label}"))
                                .focusable().tab_stop(true)
                                .group(group.clone())
                                .relative().flex_none().size(px(122.0))
                                .rounded(px(10.0)).cursor_pointer()
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(OpenAttachmentPreview(path.clone()));
                                    cx.stop_propagation();
                                }))
                                .on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        cx.emit(OpenAttachmentPreview(key_path.clone()));
                                        cx.stop_propagation();
                                    }
                                }))
                                .child(div().size_full().rounded(px(10.0)).overflow_hidden()
                                    .child(img(file.path.clone()).flex_none().size(px(122.0)).aspect_ratio(1.0).rounded(px(10.0)).object_fit(gpui::ObjectFit::Cover)))
                                .child(div().absolute().inset_0().rounded(px(10.0))
                                    .border_1().border_color(theme.text.alpha(0.1))
                                    .group_hover(group.clone(), move |s| s.bg(theme.text.alpha(0.05)).border_color(theme.text.alpha(0.2))))
                                .child(div().id(("side-chat-remove-attachment", index))
                                    .role(Role::Button)
                                    .aria_label(crate::i18n::format!("移除附件 {label}" => "Remove attachment {label}"))
                                    .focusable().tab_stop(true)
                                    .absolute().top(px(-2.0)).right(px(-2.0)).size(px(16.0))
                                    .rounded_full().bg(theme.text).text_color(theme.control_soft)
                                    .shadow(vec![gpui::BoxShadow::new(px(0.0), px(0.0), theme.surface.into()).spread_radius(px(2.0))])
                                    .flex().items_center().justify_center().cursor_pointer()
                                    .opacity(0.0)
                                    .group_hover(group, |s| s.opacity(1.0))
                                    .focus_visible(|s| s.opacity(1.0))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_attachment(&remove_path, cx);
                                        cx.stop_propagation();
                                    }))
                                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                            this.remove_attachment(&keyboard_remove_path, cx);
                                            cx.stop_propagation();
                                        }
                                    }))
                                    .child(icon("close-dialog", theme.surface.into()).size(px(12.0))))
                                .into_any_element();
                        }
                        div()
                            .id(("side-chat-attachment", index))
                            .flex_none()
                            .max_w(px(200.0))
                            .h(px(26.0))
                            .px(px(8.0))
                            .rounded(px(8.0))
                            .bg(theme.text.alpha(0.06))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                icon(
                                    "utility-folder",
                                    theme.text_secondary.into(),
                                )
                                .size(px(14.0)),
                            )
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(theme.text)
                                    .child(label.clone()),
                            )
                            .child(
                                div()
                                    .id(("side-chat-remove-attachment", index))
                                    .role(Role::Button)
                                    .aria_label(crate::i18n::format!("移除附件 {label}" => "Remove attachment {label}"))
                                    .focusable()
                                    .tab_stop(true)
                                    .size(px(20.0))
                                    .rounded(px(4.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(theme.sidebar_hover))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.draft_revision = this.draft_revision.wrapping_add(1);
                                        this.prompt_context.files.retain(|file| file.path != path);
                                        cx.notify();
                                        cx.stop_propagation();
                                    }))
                                    .on_key_down(cx.listener(
                                        move |this, e: &KeyDownEvent, _, cx| {
                                            if matches!(e.keystroke.key.as_str(), "enter" | "space")
                                            {
                                                if index < this.prompt_context.files.len() {
                                                    this.draft_revision =
                                                        this.draft_revision.wrapping_add(1);
                                                    this.prompt_context.files.remove(index);
                                                }
                                                cx.notify();
                                                cx.stop_propagation();
                                            }
                                        },
                                    ))
                                    .child(
                                        icon("close-dialog", theme.text_secondary.into())
                                            .size(px(12.0)),
                                    ),
                            )
                            .into_any_element()
                    }),
            )
    }

    fn remove_attachment(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.draft_revision = self.draft_revision.wrapping_add(1);
        self.prompt_context.files.retain(|file| file.path != path);
        self.focus_prompt_pending = true;
        cx.notify();
    }

    pub(super) fn render_context_menu(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let item = |id, label: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .role(Role::MenuItem)
                .aria_label(label)
                .focusable()
                .tab_stop(true)
                .w_full()
                .h(px(28.5703))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .cursor_pointer()
                .text_size(px(13.0))
                .text_color(theme.text)
                .hover(move |s| s.bg(theme.sidebar_hover))
                .focus_visible(move |s| s.bg(theme.sidebar_hover))
                .child(icon(glyph, theme.text_secondary.into()).size(px(16.0)))
                .child(label)
        };
        deferred(
            div()
                .id("side-chat-context-menu")
                .role(Role::Menu)
                .key_context("ComposerContextMenu")
                .on_action(
                    cx.listener(|this, _: &super::DismissContextMenu, window, cx| {
                        this.context_menu_open = false;
                        this.context_focus_pending = false;
                        this.prompt_focus_handle(cx).focus(window, cx);
                        cx.notify();
                        cx.stop_propagation();
                    }),
                )
                .track_focus(&self.context_focus)
                .absolute()
                .bottom(px(50.0))
                .left(px(8.0))
                .w(px(self
                    .available_width
                    .map_or(280.0, |width| (width - 16.0).clamp(200.0, 280.0))))
                .rounded(px(12.0))
                .p(px(4.0))
                .bg(theme.model_picker_surface)
                .border(px(0.5))
                .border_color(theme.border)
                .shadow_md()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.context_menu_open = false;
                    cx.notify();
                }))
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    match e.keystroke.key.as_str() {
                        "escape" => {
                            this.context_menu_open = false;
                            this.prompt_focus_handle(cx).focus(window, cx);
                        }
                        "down" | "tab" => window.focus_next(cx),
                        "up" => window.focus_prev(cx),
                        "enter" | "space" => this.attach_files(cx),
                        _ => return,
                    }
                    cx.notify();
                    cx.stop_propagation();
                }))
                .child(
                    div()
                        .px(px(8.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .text_size(px(12.0))
                        .text_color(theme.text_tertiary)
                        .child(crate::i18n::text("添加")),
                )
                .child(
                    item(
                        "side-chat-add-files",
                        crate::i18n::text("文件和文件夹"),
                        "utility-folder",
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.attach_files(cx);
                        cx.stop_propagation();
                    }))
                    .on_key_down(cx.listener(
                        |this, e: &KeyDownEvent, _, cx| {
                            if matches!(e.keystroke.key.as_str(), "enter" | "space") {
                                this.attach_files(cx);
                                cx.stop_propagation();
                            }
                        },
                    )),
                )
                .when(self.plan_mode_available(), |menu| {
                    menu.child(
                        item(
                            "side-chat-plan-mode",
                            crate::i18n::text("计划模式"),
                            "panel-review",
                        )
                        .child(div().flex_1())
                        .when(self.prompt_context.plan_mode == Some(true), |d| {
                            d.child(icon("check", theme.text.into()).size(px(14.0)))
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.prompt_context.plan_mode =
                                Some(this.prompt_context.plan_mode != Some(true));
                            this.context_menu_open = false;
                            cx.notify();
                            cx.stop_propagation();
                        }))
                        .on_key_down(cx.listener(
                            |this, e: &KeyDownEvent, _, cx| {
                                if matches!(e.keystroke.key.as_str(), "enter" | "space") {
                                    this.prompt_context.plan_mode =
                                        Some(this.prompt_context.plan_mode != Some(true));
                                    this.context_menu_open = false;
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            },
                        )),
                    )
                }),
        )
        .with_priority(20)
    }
}
