//! A separate draggable companion window; it never owns a conversation or turn.
use super::ComposerView;
use gpui::{
    AppContext, Bounds, Context, FocusHandle, MouseButton, Render, Role, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, img, point,
    prelude::*, px, size,
};
use std::time::Duration;

pub(crate) struct DesktopPet {
    owner: gpui::WeakEntity<ComposerView>,
    frame: usize,
    hover: bool,
    menu: bool,
    focus: FocusHandle,
}
impl ComposerView {
    pub(super) fn toggle_pet(&mut self, cx: &mut Context<Self>) {
        cx.emit(super::ToggleDesktopPet);
    }
}
impl DesktopPet {
    pub(crate) fn open(
        owner: gpui::WeakEntity<ComposerView>,
        cx: &mut gpui::App,
    ) -> anyhow::Result<gpui::WindowHandle<Self>> {
        let options = WindowOptions {
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(60.), px(600.)),
                size(px(113.), px(147.)),
            ))),
            kind: WindowKind::Floating,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: false,
            is_resizable: false,
            is_minimizable: false,
            app_owns_titlebar_drag: true,
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            keep_pet_visible(window);
            cx.new(|cx| {
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(140))
                            .await;
                        if this
                            .update(cx, |this: &mut DesktopPet, cx| {
                                this.frame = this.frame.wrapping_add(1);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
                DesktopPet {
                    owner,
                    frame: 0,
                    hover: false,
                    menu: false,
                    focus: cx.focus_handle(),
                }
            })
        })
    }
}
impl Render for DesktopPet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).is_running());
        let (row, column) = if self.hover {
            (4, self.frame % 5)
        } else if running {
            (7, self.frame % 6)
        } else {
            (0, (self.frame / 6) % 6)
        };
        div()
            .id("desktop-pet")
            .role(Role::Image)
            .aria_label(crate::i18n::format!("虚拟宠物"=>"Pet"))
            .track_focus(&self.focus)
            .on_action(|_: &crate::app::CaptureFrame, _window, _cx| {
                #[cfg(feature = "screenshot")]
                if let Ok(path) = std::env::var("GPUI_CAPTURE_OUTPUT") {
                    crate::capture_frame(_window, path, 3);
                }
            })
            .size_full()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.focus.focus(window, cx);
                    window.start_window_move();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, window, cx| {
                    this.focus.focus(window, cx);
                    this.menu = !this.menu;
                    cx.notify();
                }),
            )
            .on_hover(cx.listener(|this, hovered, _, cx| {
                this.hover = *hovered;
                cx.notify();
            }))
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .w(px(113.))
                    .h(px(123.))
                    .overflow_hidden()
                    .child(
                        img("pets/codex.webp")
                            .absolute()
                            .left(px(-(column as f32) * 113.))
                            .top(px(-(row as f32) * 123.))
                            .w(px(904.))
                            .h(px(1353.))
                            .flex_none(),
                    ),
            )
            .when(self.menu, |d| {
                d.child(
                    div()
                        .id("pet-close")
                        .role(Role::Button)
                        .focusable()
                        .tab_stop(true)
                        .aria_label(crate::i18n::format!("关闭虚拟宠物"=>"Close pet"))
                        .absolute()
                        .top_0()
                        .right_0()
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(gpui::rgb(0x2d2d2d))
                        .text_color(gpui::rgb(0xffffff))
                        .cursor_pointer()
                        .on_click(|_, window, _| window.remove_window())
                        .on_key_down(|event, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                window.remove_window();
                                cx.stop_propagation();
                            }
                        })
                        .child(crate::i18n::text("关闭")),
                )
            })
    }
}

#[cfg(target_os = "macos")]
fn keep_pet_visible(window: &Window) {
    use objc::{
        Message,
        runtime::{NO, Object, Sel},
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // NSPanel otherwise hides on application deactivation, unlike the
    // desktop companion, which stays available while another app is active.
    unsafe {
        let view = &*handle.ns_view.as_ptr().cast::<Object>();
        let Ok(panel): Result<*mut Object, _> = view.send_message(Sel::register("window"), ())
        else {
            return;
        };
        if let Some(panel) = panel.as_ref() {
            let _: Result<(), _> =
                panel.send_message(Sel::register("setHidesOnDeactivate:"), (NO,));
            let _: Result<(), _> = panel.send_message(Sel::register("orderFrontRegardless"), ());
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn keep_pet_visible(_: &Window) {}
