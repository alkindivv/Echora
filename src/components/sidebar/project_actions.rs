//! Project row actions, shared with the project information card.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectAction {
    Edit(ProjectId),
    Archive(ProjectId),
    Remove(ProjectId),
}
impl gpui::EventEmitter<ProjectAction> for SidebarView {}

impl SidebarView {
    pub fn project_return_focus(&self, id: &str) -> Option<gpui::FocusHandle> {
        self.project_action_focus.get(id).cloned()
    }

    pub(super) fn project_actions_button(
        &self,
        project: &Project,
        visible: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let id = project.project_id.clone();
        let anchor_id = id.clone();
        let bounds = self.project_action_bounds.clone();
        let click_id = id.clone();
        let key_id = id.clone();
        Self::quiet_icon_button(format!("project-menu-{id}"), "more-horizontal", theme)
            .size(px(20.))
            .role(gpui::Role::Button)
            .aria_label(crate::i18n::format!("{name}的项目操作" => "Project actions for {name}", name = project.name))
            .aria_expanded(self.project_menu_id.as_deref() == Some(&id))
            .track_focus(self.project_action_focus.get(&id).unwrap())
            .tab_stop(true)
            .when(!visible, |e| e.opacity(0.0))
            .focus_visible(|s| s.opacity(1.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_project_actions(click_id.clone(), false, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                if this.project_menu_id.is_none() && matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                    this.open_project_actions(key_id.clone(), true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(canvas(move |r, _, _| {bounds.borrow_mut().insert(anchor_id.clone(), r);}, |_, _, _, _| {}).absolute().inset_0())
    }

    pub(super) fn open_project_actions(
        &mut self,
        id: ProjectId,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.project_menu_id.as_ref() != Some(&id);
        self.close_transient_menus(cx);
        self.project_hover_card = None;
        self.hovered_project_id = None;
        if open {
            self.project_menu_id = Some(id.clone());
            self.project_menu_selected = keyboard.then_some(0);
            self.project_action_focus
                .get(&id)
                .unwrap()
                .focus(window, cx);
        }
        cx.notify();
    }

    pub fn dismiss_project_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.project_menu_id.take() else {
            return false;
        };
        if let Some(focus) = self.project_action_focus.get(&id) {
            focus.focus(window, cx);
        }
        self.project_menu_selected = None;
        cx.notify();
        true
    }

    pub(super) fn activate_project_action(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project_menu_id.clone() else {
            return;
        };
        let Some(project) = self
            .snapshot
            .projects
            .iter()
            .find(|p| p.project_id == id)
            .cloned()
        else {
            return;
        };
        if !self.project_action_enabled(index, &project) {
            return;
        }
        self.dismiss_project_actions(window, cx);
        match index {
            0 => self.store.set_project_pinned(
                id.clone(),
                !self.snapshot.preferences.pinned_project_ids.contains(&id),
            ),
            1 => cx.emit(ProjectAction::Edit(id)),
            2 => {
                if let Some(path) = project.roots.first() {
                    self.reveal_in_finder(path.clone(), cx);
                }
            }
            3 => cx.emit(ProjectAction::Archive(id)),
            4 => cx.emit(ProjectAction::Remove(id)),
            _ => {}
        }
    }

    fn project_action_enabled(&self, index: usize, project: &Project) -> bool {
        if self.snapshot.is_pending_project(&project.project_id) {
            return false;
        }
        match index {
            1 => self
                .snapshot
                .capabilities
                .supports(AgentCapability::ProjectUpdate),
            2 => !project.roots.is_empty(),
            3 => {
                self.snapshot
                    .capabilities
                    .supports(AgentCapability::ThreadArchive)
                    && !self
                        .snapshot
                        .project_thread_ids(&project.project_id)
                        .is_empty()
            }
            4 => self
                .snapshot
                .capabilities
                .supports(AgentCapability::ProjectDelete),
            _ => true,
        }
    }

    pub(super) fn project_actions_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project_menu_id.clone() else {
            return;
        };
        let Some(project) = self
            .snapshot
            .projects
            .iter()
            .find(|p| p.project_id == id)
            .cloned()
        else {
            return;
        };
        match event.keystroke.key.as_str() {
            "escape" | "tab" => {
                self.dismiss_project_actions(window, cx);
            }
            "down" | "up" | "home" | "end" => {
                let key = event.keystroke.key.as_str();
                let mut n = match key {
                    "home" => 0,
                    "end" => 4,
                    "up" => self.project_menu_selected.map_or(4, |n| (n + 4) % 5),
                    _ => self.project_menu_selected.map_or(0, |n| (n + 1) % 5),
                };
                for _ in 0..5 {
                    if self.project_action_enabled(n, &project) {
                        break;
                    }
                    n = if key == "up" || key == "end" {
                        (n + 4) % 5
                    } else {
                        (n + 1) % 5
                    };
                }
                self.project_menu_selected = Some(n);
            }
            "enter" | "space" => {
                self.activate_project_action(self.project_menu_selected.unwrap_or(0), window, cx)
            }
            key if key.chars().count() == 1 => {
                let labels = [
                    crate::i18n::text(
                        if self.snapshot.preferences.pinned_project_ids.contains(&id) {
                            "取消固定"
                        } else {
                            "固定"
                        },
                    ),
                    crate::i18n::text("编辑"),
                    crate::i18n::text("在 Finder 中显示"),
                    crate::i18n::text("归档项目聊天"),
                    crate::i18n::text("移除项目"),
                ];
                self.project_menu_selected = labels.iter().enumerate().position(|(n, label)| {
                    self.project_action_enabled(n, &project)
                        && label.to_lowercase().starts_with(&key.to_lowercase())
                });
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn project_actions_overlay(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let id = self.project_menu_id.as_ref()?;
        let project = self
            .snapshot
            .projects
            .iter()
            .find(|p| &p.project_id == id)?;
        let anchor = self.project_action_bounds.borrow().get(id).copied()?;
        let viewport = window.viewport_size();
        let left =
            (f32::from(anchor.left()) + 1.).clamp(8., (f32::from(viewport.width) - 168.).max(8.));
        let top = (f32::from(anchor.bottom()) + 2.)
            .min(f32::from(viewport.height) - 186.)
            .max(8.);
        let pinned = self.snapshot.preferences.pinned_project_ids.contains(id);
        let labels = [
            crate::i18n::text(if pinned { "取消固定" } else { "固定" }),
            crate::i18n::text("编辑"),
            crate::i18n::text("在 Finder 中显示"),
            crate::i18n::text("归档项目聊天"),
            crate::i18n::text("移除项目"),
        ];
        let glyphs = [
            "project-pin",
            "project-action-1",
            "project-reveal",
            "archive",
            "close-dialog",
        ];
        let mut menu = self.recents_surface("project-actions-menu",160.,theme,window.scale_factor()).aria_label(crate::i18n::format!("{name}的项目操作" => "Project actions for {name}",name=project.name));
        let mut rhythm = AccountMenuRhythm::new(window.scale_factor());
        for (index, label) in labels.into_iter().enumerate() {
            if index >= 2 {
                menu = menu.child(account_menu_separator(rhythm.span(9.), theme));
            }
            let enabled = self.project_action_enabled(index, project);
            menu = menu.child(
                account_menu_row(
                    [
                        "project-action-pin",
                        "project-action-edit",
                        "project-action-reveal",
                        "project-action-archive",
                        "project-action-remove",
                    ][index],
                    label.into(),
                    Some(glyphs[index]),
                    None,
                    rhythm.span(28.5625),
                    theme,
                    enabled,
                )
                .role(gpui::Role::MenuItem)
                .aria_label(label)
                .when(!enabled, |r| r.opacity(0.4))
                .when(self.project_menu_selected == Some(index), |r| {
                    r.bg(theme.sidebar_hover)
                })
                .when(enabled, |r| {
                    r.on_hover(cx.listener(move |this, hover: &bool, _, cx| {
                        if *hover {
                            this.project_menu_selected = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_project_action(index, window, cx)
                    }))
                }),
            );
        }
        Some(
            deferred(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(viewport.width)
                    .h(viewport.height)
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.dismiss_project_actions(window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(div().absolute().left(px(left)).top(px(top)).child(menu)),
            )
            .into_any_element(),
        )
    }
}
