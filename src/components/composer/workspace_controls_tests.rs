use super::{ComposerView, workspace_controls::WorkspaceMenu};
use crate::agent::*;
use crate::{
    git_review::{Checkout, workspace::Branches},
    theme::ThemeMode,
};
use gpui::AppContext;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn reply<T>(value: T) -> async_channel::Receiver<T> {
    let (sender, receiver) = async_channel::bounded(1);
    sender.try_send(value).ok().unwrap();
    receiver
}

#[derive(Default)]
struct Backend {
    requests: Mutex<Vec<AgentRequest>>,
}
impl AgentBackend for Backend {
    fn subscribe_connection_events(&self) -> async_channel::Receiver<AgentConnectionEvent> {
        async_channel::unbounded().1
    }
    fn load_model_catalog(&self) -> async_channel::Receiver<Result<AgentModelCatalog, String>> {
        async_channel::unbounded().1
    }
    fn load_permission_profiles(
        &self,
        _: PathBuf,
    ) -> async_channel::Receiver<Result<Vec<AgentPermissionProfile>, String>> {
        reply(Ok(vec![]))
    }
    fn update_thread_permissions(
        &self,
        _: AgentThreadPermissionUpdate,
    ) -> async_channel::Receiver<Result<AgentThreadPermissionResult, String>> {
        async_channel::unbounded().1
    }
    fn read_config(
        &self,
        cwd: PathBuf,
    ) -> async_channel::Receiver<Result<AgentConfigSnapshot, AgentConfigError>> {
        reply(Ok(AgentConfigSnapshot {
            generation: 0,
            cwd,
            effective: serde_json::json!({"model": "test"}),
            origins: Default::default(),
            layers: None,
            requirements: None,
            value_aliases: Default::default(),
            value_defaults: Default::default(),
            profile_parents: Default::default(),
            required_fields: Default::default(),
        }))
    }
    fn run_prompt(&self, request: AgentRequest) -> AgentRun {
        self.requests.lock().unwrap().push(request);
        AgentRun::new(async_channel::unbounded().1, None)
    }
}

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "echora-composer-workspace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.invalid"],
        ] {
            Self::git(&path, &args);
        }
        std::fs::write(path.join("file"), "main").unwrap();
        Self::git(&path, &["add", "."]);
        Self::git(&path, &["commit", "-m", "fixture"]);
        Self::git(&path, &["branch", "topic"]);
        Self(path)
    }
    fn git(path: &std::path::Path, args: &[&str]) {
        assert!(
            std::process::Command::new("git")
                .current_dir(path)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(app: &mut gpui::TestApp, repo: &Repo) -> (gpui::Entity<ComposerView>, Arc<Backend>) {
    let backend = Arc::new(Backend::default());
    let source: Arc<dyn AgentBackend> = backend.clone();
    let composer = app.new_entity(|cx| ComposerView::new_with_backend(ThemeMode::Dark, source, cx));
    app.update_entity(&composer, |c, cx| {
        c.conversation.cwd = repo.0.clone();
        c.conversation.project_id = Some("fixture".into());
        c.conversation.selected_model = "test".into();
        c.codex_home = Some(repo.0.join("codex-home"));
        c.set_checkout(Checkout::Branch("main".into()), cx);
        c.set_project_label(Some("Fixture".into()), cx);
    });
    app.run_until_parked();
    (composer, backend)
}

#[test]
fn first_worktree_send_waits_for_creation_and_uses_the_created_directory() {
    let repo = Repo::new();
    let mut app = gpui::TestApp::new();
    let (composer, backend) = fixture(&mut app, &repo);
    app.update_entity(&composer, |c, cx| {
        c.set_draft_text("inspect workspace", cx);
        c.select_run_location(true, cx);
        c.select_workspace_branch("topic".into(), false, cx);
        c.submit_prompt("inspect workspace".into(), cx);
        c.submit_prompt("inspect workspace".into(), cx);
        assert!(backend.requests.lock().unwrap().is_empty());
    });
    app.run_until_parked();
    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_ne!(requests[0].cwd, repo.0);
    assert!(
        requests[0]
            .cwd
            .starts_with(repo.0.join("codex-home/worktrees"))
    );
    assert_eq!(
        crate::git_review::checkout(&requests[0].cwd),
        Checkout::Detached
    );
    assert_eq!(
        crate::git_review::checkout(&repo.0),
        Checkout::Branch("main".into())
    );
    app.update_entity(&composer, |c, _| {
        assert_eq!(c.conversation.cwd, requests[0].cwd)
    });
}

#[test]
fn local_file_state_choice_copies_changes_into_the_first_sends_worktree() {
    let repo = Repo::new();
    std::fs::write(repo.0.join("file"), "local edit").unwrap();
    let mut app = gpui::TestApp::new();
    let (composer, backend) = fixture(&mut app, &repo);
    app.update_entity(&composer, |c, cx| {
        c.set_draft_text("inspect changes", cx);
        c.select_run_location(true, cx);
        c.choose_workspace_branch("main".into(), true, cx);
        c.submit_prompt("inspect changes".into(), cx);
    });
    app.run_until_parked();
    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        std::fs::read_to_string(requests[0].cwd.join("file")).unwrap(),
        "local edit"
    );
    assert_eq!(
        std::fs::read_to_string(repo.0.join("file")).unwrap(),
        "local edit"
    );
}

#[test]
fn failed_worktree_creation_keeps_prompt_and_source_directory_and_can_retry() {
    let repo = Repo::new();
    let mut app = gpui::TestApp::new();
    let (composer, backend) = fixture(&mut app, &repo);
    app.update_entity(&composer, |c, cx| {
        c.set_draft_text("keep this prompt", cx);
        c.select_run_location(true, cx);
        c.workspace_controls.base = Some("missing".into());
        c.submit_prompt("keep this prompt".into(), cx);
    });
    app.run_until_parked();
    app.update_entity(&composer, |c, cx| {
        assert_eq!(c.prompt_text(cx), "keep this prompt");
        assert_eq!(c.conversation.cwd, repo.0);
        assert!(c.submission_error.is_some());
        assert!(!c.workspace_controls.busy);
        assert!(backend.requests.lock().unwrap().is_empty());
        c.workspace_controls.base = Some("topic".into());
        c.submit_prompt("keep this prompt".into(), cx);
    });
    app.run_until_parked();
    assert_eq!(backend.requests.lock().unwrap().len(), 1);
}

struct Harness {
    composer: gpui::Entity<ComposerView>,
}

fn control_bounds(
    window: &gpui::TestAppWindow<Harness>,
    location: bool,
) -> gpui::Bounds<gpui::Pixels> {
    window.read(|h, cx| {
        let c = &h.composer.read(cx).workspace_controls;
        if location {
            c.location_bounds.get()
        } else {
            c.branch_bounds.get()
        }
        .unwrap()
    })
}

fn menu_open(window: &gpui::TestAppWindow<Harness>) -> bool {
    window.read(|h, cx| h.composer.read(cx).workspace_controls.menu.is_some())
}
impl gpui::Render for Harness {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        let toolbar = self.composer.update(cx, |c, cx| {
            super::render::context_toolbar(c, crate::theme::Theme::for_mode(ThemeMode::Dark), cx)
        });
        let overlay = self
            .composer
            .update(cx, |c, cx| c.render_workspace_menu(window, cx));
        gpui::div()
            .on_action(|_: &crate::app::DismissPermissionUi, _, cx| cx.stop_propagation())
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .pb(gpui::px(30.))
            .child(
                gpui::div()
                    .w(gpui::px(740.))
                    .mx_auto()
                    .child(toolbar)
                    .child(self.composer.clone()),
            )
            .children(overlay)
    }
}

#[test]
fn toolbar_hit_areas_keyboard_search_outside_dismissal_and_real_switch() {
    use gpui::MouseButton;
    let repo = Repo::new();
    let mut app = gpui::TestApp::new();
    app.update(|cx| {
        cx.bind_keys([gpui::KeyBinding::new(
            "escape",
            crate::app::DismissPermissionUi,
            None,
        )])
    });
    let (composer, _) = fixture(&mut app, &repo);
    let target = composer.clone();
    let mut window = app.open_window(|_, _| Harness { composer: target });
    window.draw();
    let location = control_bounds(&window, true);
    // Both icon and trailing padding belong to the button's hit region.
    window.simulate_click(
        gpui::point(location.left() + gpui::px(2.), location.center().y),
        MouseButton::Left,
    );
    window.draw();
    assert!(menu_open(&window));
    window.simulate_keystrokes("down down enter");
    app.run_until_parked();
    app.update_entity(&composer, |c, _| {
        assert!(c.workspace_controls.worktree_mode)
    });
    window.draw();
    let location = control_bounds(&window, true);
    window.simulate_click(
        gpui::point(location.right() - gpui::px(2.), location.center().y),
        MouseButton::Left,
    );
    window.draw();
    window.simulate_keystrokes("down enter");
    app.update_entity(&composer, |c, _| {
        assert!(!c.workspace_controls.worktree_mode)
    });
    window.draw();
    let branch = control_bounds(&window, false);
    window.simulate_click(branch.center(), MouseButton::Left);
    app.run_until_parked();
    window.draw();
    app.update_entity(&composer, |c, cx| {
        c.workspace_controls
            .input
            .update(cx, |i, cx| i.set_text("topic", cx))
    });
    window.draw();
    window.simulate_keystrokes("down enter");
    app.run_until_parked();
    window.draw();
    assert_eq!(
        crate::git_review::checkout(&repo.0),
        Checkout::Branch("topic".into())
    );
    app.update_entity(&composer, |c, _| {
        assert_eq!(c.workspace.checkout, Some(Checkout::Branch("topic".into())))
    });
    let branch = control_bounds(&window, false);
    window.simulate_click(branch.center(), MouseButton::Left);
    app.run_until_parked();
    window.draw();
    window.simulate_keystroke("escape");
    window.draw();
    assert!(!menu_open(&window));
    window.simulate_click(branch.center(), MouseButton::Left);
    app.run_until_parked();
    window.draw();
    window.simulate_click(gpui::point(gpui::px(5.), gpui::px(5.)), MouseButton::Left);
    window.draw();
    assert!(!menu_open(&window));
}

#[gpui::test]
fn worktree_selection_is_reversible_and_does_not_change_the_source_checkout(
    cx: &mut gpui::TestAppContext,
) {
    let composer = cx.new(|cx| ComposerView::new(ThemeMode::Dark, cx));
    composer.update(cx, |c, cx| {
        c.conversation.cwd = "/tmp/source".into();
        c.set_checkout(Checkout::Branch("main".into()), cx);
        c.select_run_location(true, cx);
        assert!(c.workspace_controls.worktree_mode);
        assert_eq!(c.conversation.cwd, std::path::PathBuf::from("/tmp/source"));
        c.select_workspace_branch("topic".into(), false, cx);
        assert_eq!(c.workspace_controls.base.as_deref(), Some("topic"));
        assert_eq!(c.workspace.checkout, Some(Checkout::Branch("main".into())));
        c.select_run_location(false, cx);
        assert!(!c.workspace_controls.worktree_mode);
        assert!(c.workspace_controls.worktree_cwd.is_none());
    });
}

#[gpui::test]
fn branch_search_preserves_full_names_and_reaches_all_rows(cx: &mut gpui::TestAppContext) {
    let composer = cx.new(|cx| ComposerView::new(ThemeMode::Dark, cx));
    composer.update(cx, |c, cx| {
        c.workspace_controls.menu = Some(WorkspaceMenu::Branch);
        c.workspace_controls.branches = Some(Branches {
            current: Checkout::Branch("main".into()),
            names: (0..150)
                .map(|i| format!("topic/long-branch-name-{i:03}"))
                .collect(),
            dirty_files: 0,
            default_remote: None,
        });
        c.workspace_controls
            .input
            .update(cx, |i, cx| i.set_text("149", cx));
        assert_eq!(
            c.workspace_branch_choices(cx),
            ["topic/long-branch-name-149"]
        );
        c.workspace_controls
            .input
            .update(cx, |i, cx| i.set_text("", cx));
        assert_eq!(c.workspace_branch_choices(cx).len(), 150);
        c.workspace_controls.busy = true;
        assert_eq!(c.send_action(cx), super::send::SendAction::Starting);
    });
}

#[test]
fn branch_list_scroll_reaches_the_last_row() {
    let repo = Repo::new();
    let mut app = gpui::TestApp::new();
    let (composer, _) = fixture(&mut app, &repo);
    let target = composer.clone();
    let mut window = app.open_window(|_, _| Harness { composer: target });
    window.draw();
    let branch = control_bounds(&window, false);
    window.simulate_click(branch.center(), gpui::MouseButton::Left);
    app.run_until_parked();
    app.update_entity(&composer, |c, _| {
        c.workspace_controls.branches.as_mut().unwrap().names = (0..150)
            .map(|i| format!("topic/long-branch-name-{i:03}"))
            .collect();
    });
    window.draw();
    window.simulate_scroll(
        gpui::point(branch.center().x, branch.top() - gpui::px(100.)),
        gpui::point(gpui::px(0.), gpui::px(-10_000.)),
    );
    window.draw();
    let offset = window.read(|h, cx| h.composer.read(cx).workspace_controls.scroll.offset().y);
    assert!(
        offset < gpui::px(-4000.),
        "last row must be reachable: {offset:?}"
    );
}

#[test]
fn create_branch_dialog_keeps_keyboard_focus_and_uses_the_saved_prefix() {
    use gpui::Focusable;
    let repo = Repo::new();
    let mut app = gpui::TestApp::new();
    app.update(|cx| {
        cx.bind_keys([gpui::KeyBinding::new(
            "escape",
            crate::app::DismissPermissionUi,
            None,
        )])
    });
    let (composer, _) = fixture(&mut app, &repo);
    app.update_entity(&composer, |c, cx| {
        c.set_branch_prefix("feature/".into());
        c.open_create_branch(cx);
    });
    let target = composer.clone();
    let mut window = app.open_window(|_, _| Harness { composer: target });
    window.draw();
    window.update(|h, window, cx| {
        let c = h.composer.read(cx);
        assert_eq!(c.workspace_controls.input.read(cx).text(), "feature/");
        assert!(
            c.workspace_controls
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
    });
    for stroke in [
        "tab",
        "tab",
        "tab",
        "tab",
        "shift-tab",
        "shift-tab",
        "shift-tab",
        "shift-tab",
    ] {
        window.simulate_keystroke(stroke);
        window.draw();
        window.update(|h, window, cx| {
            assert!(
                h.composer
                    .read(cx)
                    .workspace_controls
                    .menu_focus
                    .contains_focused(window, cx)
            );
        });
    }
    window.simulate_keystroke("escape");
    window.draw();
    assert!(!menu_open(&window));
    assert_eq!(
        crate::git_review::checkout(&repo.0),
        Checkout::Branch("main".into())
    );
}
