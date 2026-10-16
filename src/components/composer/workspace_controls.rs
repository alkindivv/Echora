//! Run location and checkout selection for unsent chats.
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AppContext, Bounds, Context, Entity, FocusHandle, Focusable, KeyDownEvent, Pixels,
    ScrollHandle, Window, point, px,
};

use super::ComposerView;
use crate::{
    components::prompt_input::{PromptChanged, PromptInput, PromptSubmitted},
    git_review::{Checkout, workspace::Branches},
    theme::ThemeMode,
};

pub struct DraftWorkspaceChanged(pub PathBuf);
impl gpui::EventEmitter<DraftWorkspaceChanged> for ComposerView {}
pub struct OpenWorkspaceGitSettings;
impl gpui::EventEmitter<OpenWorkspaceGitSettings> for ComposerView {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceMenu {
    Location,
    Branch,
    CreateBranch,
}

pub(super) struct WorkspaceControls {
    pub menu: Option<WorkspaceMenu>,
    pub location_focus: FocusHandle,
    pub branch_focus: FocusHandle,
    pub menu_focus: FocusHandle,
    pub dialog_close_focus: FocusHandle,
    pub input_focus_pending: bool,
    pub branch_prefix: String,
    pub location_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub branch_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub input: Entity<PromptInput>,
    pub branches: Option<Branches>,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub focused: Option<usize>,
    pub scroll: ScrollHandle,
    pub worktree_mode: bool,
    pub base: Option<String>,
    pub include_local_changes: bool,
    pub local_cwd: Option<PathBuf>,
    pub worktree_cwd: Option<PathBuf>,
    pub pending_send: Option<(String, bool)>,
    cycle: u64,
}

impl WorkspaceControls {
    pub fn new(mode: ThemeMode, cx: &mut Context<ComposerView>) -> Self {
        let input = cx.new(|cx| {
            PromptInput::project_name(
                mode,
                crate::i18n::format!("搜索分支" => "Search branches"),
                cx,
            )
        });
        cx.subscribe(&input, |this, _, _: &PromptChanged, cx| {
            this.workspace_controls.focused = None;
            if this.workspace_controls.menu == Some(WorkspaceMenu::CreateBranch) {
                this.workspace_controls.error = None;
            }
            this.workspace_controls
                .scroll
                .set_offset(point(px(0.), px(0.)));
            cx.notify();
        })
        .detach();
        cx.subscribe(&input, |this, _, _: &PromptSubmitted, cx| {
            this.confirm_workspace_choice(cx)
        })
        .detach();
        Self {
            menu: None,
            location_focus: cx.focus_handle().tab_stop(true),
            branch_focus: cx.focus_handle().tab_stop(true),
            menu_focus: cx.focus_handle(),
            dialog_close_focus: cx.focus_handle().tab_stop(true),
            input_focus_pending: false,
            branch_prefix: "codex/".into(),
            location_bounds: Rc::new(Cell::new(None)),
            branch_bounds: Rc::new(Cell::new(None)),
            input,
            branches: None,
            loading: false,
            busy: false,
            error: None,
            focused: None,
            scroll: ScrollHandle::new(),
            worktree_mode: false,
            base: None,
            include_local_changes: false,
            local_cwd: None,
            worktree_cwd: None,
            pending_send: None,
            cycle: 0,
        }
    }

    pub fn owns_cwd(&self, cwd: &Path) -> bool {
        self.local_cwd.as_deref() == Some(cwd) || self.worktree_cwd.as_deref() == Some(cwd)
    }

    pub fn reset(&mut self) {
        self.cycle = self.cycle.wrapping_add(1);
        self.menu = None;
        self.branches = None;
        self.loading = false;
        self.busy = false;
        self.error = None;
        self.worktree_mode = false;
        self.base = None;
        self.include_local_changes = false;
        self.local_cwd = None;
        self.worktree_cwd = None;
        self.pending_send = None;
    }
}

impl ComposerView {
    pub fn set_branch_prefix(&mut self, prefix: String) {
        self.workspace_controls.branch_prefix = prefix;
    }

    pub(super) fn open_workspace_menu(
        &mut self,
        menu: WorkspaceMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.thread_id().is_some() || self.workspace_controls.busy {
            return;
        }
        if self.workspace_controls.menu == Some(menu) {
            self.close_workspace_menu(window, cx);
            return;
        }
        self.workspace_controls.menu = Some(menu);
        self.workspace_controls.error = None;
        self.workspace_controls.focused = None;
        self.workspace_controls
            .scroll
            .set_offset(point(px(0.), px(0.)));
        self.project_picker.open = false;
        self.menu_open = false;
        self.permission_menu_open = false;
        self.context_menu_open = false;
        self.workspace_controls.input.update(cx, |input, cx| {
            input.set_mode(self.mode, cx);
            let project = self.workspace.project_label.as_deref().unwrap_or("");
            let placeholder = if self.workspace_controls.worktree_mode {
                crate::i18n::format!("搜索分支…" => "Search branches…")
            } else {
                crate::i18n::format!("搜索 {project} 分支" => "Search {project} branches")
            };
            input.set_placeholder(placeholder, cx);
            input.set_accessible_name(crate::i18n::format!("搜索分支" => "Search branches"));
            input.set_text_silently("", cx);
        });
        if menu == WorkspaceMenu::Branch {
            self.workspace_controls
                .input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
            self.load_workspace_branches(cx);
        } else {
            self.workspace_controls.menu_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_workspace_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let menu = self.workspace_controls.menu.take();
        if menu == Some(WorkspaceMenu::Location) {
            self.workspace_controls.location_focus.focus(window, cx);
        } else {
            self.workspace_controls.branch_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn load_workspace_branches(&mut self, cx: &mut Context<Self>) {
        self.workspace_controls.cycle = self.workspace_controls.cycle.wrapping_add(1);
        let cycle = self.workspace_controls.cycle;
        let cwd = self.conversation.cwd.clone();
        self.workspace_controls.loading = true;
        self.workspace_controls.error = None;
        cx.spawn(async move |this, cx| {
            let lookup = cwd.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::git_review::workspace::branches(&lookup) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.workspace_controls.cycle != cycle || this.conversation.cwd != cwd {
                    return;
                }
                this.workspace_controls.loading = false;
                match result {
                    Ok(branches) => {
                        this.set_checkout(branches.current.clone(), cx);
                        this.workspace_controls.branches = Some(branches);
                    }
                    Err(error) => this.workspace_controls.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn workspace_branch_choices(&self, cx: &gpui::App) -> Vec<String> {
        let query = self
            .workspace_controls
            .input
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        self.workspace_controls
            .branches
            .as_ref()
            .map(|list| {
                list.names
                    .iter()
                    .filter(|name| name.to_lowercase().contains(&query))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn workspace_choices(&self, cx: &gpui::App) -> Vec<(String, bool)> {
        let mut choices = Vec::new();
        if self.workspace_controls.worktree_mode
            && let Some(list) = &self.workspace_controls.branches
        {
            let query = self
                .workspace_controls
                .input
                .read(cx)
                .text()
                .trim()
                .to_lowercase();
            if let Some(current) = list
                .current
                .branch()
                .filter(|name| name.to_lowercase().contains(&query))
            {
                choices.push((current.to_owned(), true));
            }
            if let Some(remote) = list
                .default_remote
                .as_ref()
                .filter(|name| name.to_lowercase().contains(&query))
            {
                choices.push((remote.clone(), false));
            }
        }
        choices.extend(
            self.workspace_branch_choices(cx)
                .into_iter()
                .map(|name| (name, false)),
        );
        choices
    }

    pub(super) fn choose_workspace_branch(
        &mut self,
        branch: String,
        include_changes: bool,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_controls.busy {
            return;
        }
        self.select_workspace_branch(branch, false, cx);
        if self.workspace_controls.worktree_mode {
            self.workspace_controls.include_local_changes = include_changes;
        }
    }

    pub(super) fn select_run_location(&mut self, worktree: bool, cx: &mut Context<Self>) {
        if self.workspace_controls.busy || self.thread_id().is_some() {
            return;
        }
        if worktree
            && !self
                .workspace
                .checkout
                .as_ref()
                .is_some_and(Checkout::is_repository)
        {
            return;
        }
        self.workspace_controls.menu = None;
        self.workspace_controls.pending_send = None;
        self.workspace_controls.error = None;
        if worktree {
            self.workspace_controls
                .local_cwd
                .get_or_insert_with(|| self.conversation.cwd.clone());
            self.workspace_controls.base.get_or_insert_with(|| {
                self.workspace
                    .checkout
                    .as_ref()
                    .and_then(Checkout::branch)
                    .unwrap_or("HEAD")
                    .to_owned()
            });
        }
        self.workspace_controls.worktree_mode = worktree;
        let cwd = if worktree {
            self.workspace_controls.worktree_cwd.clone()
        } else {
            self.workspace_controls.local_cwd.clone()
        };
        if let Some(cwd) = cwd {
            self.apply_draft_cwd(cwd, cx);
        }
        self.focus_prompt_pending = true;
        cx.notify();
    }

    fn apply_draft_cwd(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.set_workspace_context(cwd.clone(), self.conversation.project_id.clone(), None, cx);
        cx.emit(DraftWorkspaceChanged(cwd));
    }

    pub(super) fn select_workspace_branch(
        &mut self,
        branch: String,
        create: bool,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_controls.busy || self.thread_id().is_some() {
            return;
        }
        if self.workspace_controls.worktree_mode
            && self.workspace_controls.worktree_cwd.is_none()
            && !create
        {
            self.workspace_controls.base = Some(branch);
            self.workspace_controls.include_local_changes = false;
            self.workspace_controls.menu = None;
            self.focus_prompt_pending = true;
            cx.notify();
            return;
        }
        self.workspace_controls.busy = true;
        self.workspace_controls.error = None;
        let cycle = self.workspace_controls.cycle;
        let cwd = self.conversation.cwd.clone();
        cx.spawn(async move |this, cx| {
            let lookup = cwd.clone();
            let result = cx
                .background_executor()
                .spawn(
                    async move { crate::git_review::workspace::switch(&lookup, &branch, create) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.workspace_controls.cycle != cycle || this.conversation.cwd != cwd {
                    return;
                }
                this.workspace_controls.busy = false;
                match result {
                    Ok(checkout) => {
                        this.checkout_cycle = this.checkout_cycle.wrapping_add(1);
                        this.set_checkout(checkout, cx);
                        this.workspace_controls.menu = None;
                        this.focus_prompt_pending = true;
                        cx.emit(DraftWorkspaceChanged(cwd));
                    }
                    Err(error) => {
                        this.workspace_controls.error = Some(error.to_string());
                        this.refresh_checkout(false, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn confirm_workspace_choice(&mut self, cx: &mut Context<Self>) {
        if self.workspace_controls.busy
            || (self.workspace_controls.menu == Some(WorkspaceMenu::Branch)
                && self.workspace_controls.loading)
        {
            return;
        }
        match self.workspace_controls.menu {
            Some(WorkspaceMenu::Location) => {
                if let Some(index) = self.workspace_controls.focused {
                    self.select_run_location(index == 1, cx);
                }
            }
            Some(WorkspaceMenu::CreateBranch) => {
                let name = self
                    .workspace_controls
                    .input
                    .read(cx)
                    .text()
                    .trim()
                    .to_owned();
                let duplicate = self
                    .workspace_controls
                    .branches
                    .as_ref()
                    .is_some_and(|b| b.names.contains(&name));
                if !name.is_empty() && !name.ends_with('/') && !duplicate {
                    self.select_workspace_branch(name, true, cx);
                }
            }
            Some(WorkspaceMenu::Branch) => {
                let choices = self.workspace_choices(cx);
                let retry = choices.len() + usize::from(!self.workspace_controls.worktree_mode);
                if self.workspace_controls.focused == Some(retry)
                    && self.workspace_controls.error.is_some()
                {
                    self.load_workspace_branches(cx);
                    return;
                }
                if let Some(index) = self.workspace_controls.focused {
                    if let Some((branch, include_changes)) = choices.get(index) {
                        self.choose_workspace_branch(branch.clone(), *include_changes, cx);
                    } else if !self.workspace_controls.worktree_mode {
                        self.open_create_branch(cx);
                    }
                }
            }
            None => {}
        }
    }

    pub(super) fn open_create_branch(&mut self, cx: &mut Context<Self>) {
        self.workspace_controls.menu = Some(WorkspaceMenu::CreateBranch);
        self.workspace_controls.error = None;
        self.workspace_controls.input_focus_pending = true;
        self.workspace_controls.input.update(cx, |input, cx| {
            input.set_text_silently(&self.workspace_controls.branch_prefix, cx);
            input.set_placeholder("new-branch", cx);
            input.set_accessible_name(crate::i18n::format!("分支名称" => "Branch name"));
        });
        cx.notify();
    }

    pub(super) fn workspace_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = self.workspace_controls.menu;
        let choices = self.workspace_choices(cx);
        let count = if menu == Some(WorkspaceMenu::Location) {
            if self
                .workspace
                .checkout
                .as_ref()
                .is_some_and(Checkout::is_repository)
            {
                2
            } else {
                1
            }
        } else if self.workspace_controls.loading {
            0
        } else {
            choices.len()
                + usize::from(!self.workspace_controls.worktree_mode)
                + usize::from(self.workspace_controls.error.is_some())
        };
        match event.keystroke.key.as_str() {
            "escape" if !self.workspace_controls.busy => self.close_workspace_menu(window, cx),
            "enter" => self.confirm_workspace_choice(cx),
            "down" | "up" | "tab" if count > 0 && menu != Some(WorkspaceMenu::CreateBranch) => {
                let backwards = event.keystroke.key == "up" || event.keystroke.modifiers.shift;
                let next = match self.workspace_controls.focused {
                    None => {
                        if backwards {
                            count - 1
                        } else {
                            0
                        }
                    }
                    Some(i) => {
                        let i = i.min(count - 1);
                        if backwards {
                            (i + count - 1) % count
                        } else {
                            (i + 1) % count
                        }
                    }
                };
                self.workspace_controls.focused = Some(next);
                let row = next.min(choices.len().saturating_sub(1));
                let special_count = choices
                    .iter()
                    .take_while(|(name, changes)| {
                        *changes
                            || self
                                .workspace_controls
                                .branches
                                .as_ref()
                                .and_then(|b| b.default_remote.as_ref())
                                == Some(name)
                    })
                    .count();
                self.workspace_controls.scroll.scroll_to_item(
                    row + 2
                        + usize::from(
                            self.workspace_controls.worktree_mode && row >= special_count,
                        ),
                );
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Selecting the mode is reversible. Allocate only for the first send.
    pub(super) fn prepare_worktree_send(
        &mut self,
        text: &str,
        inverted: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.workspace_controls.busy {
            return true;
        }
        if !self.workspace_controls.worktree_mode
            || self.workspace_controls.worktree_cwd.is_some()
            || self.thread_id().is_some()
        {
            return false;
        }
        let cwd = self.conversation.cwd.clone();
        let base = self
            .workspace_controls
            .base
            .clone()
            .unwrap_or_else(|| "HEAD".into());
        let include_changes = self.workspace_controls.include_local_changes;
        let root = self
            .codex_home
            .clone()
            .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".codex")
            })
            .join("worktrees");
        let cycle = self.workspace_controls.cycle;
        self.workspace_controls.busy = true;
        self.workspace_controls.pending_send = Some((text.to_owned(), inverted));
        self.submission_error = None;
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::git_review::workspace::create_worktree(&cwd, &base, include_changes, &root) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.workspace_controls.cycle != cycle { return; }
                this.workspace_controls.busy = false;
                match result {
                    Ok(worktree) => {
                        this.workspace_controls.local_cwd = Some(worktree.local_cwd);
                        this.workspace_controls.worktree_cwd = Some(worktree.cwd.clone());
                        this.apply_draft_cwd(worktree.cwd, cx);
                    }
                    Err(error) => {
                        this.workspace_controls.pending_send = None;
                        this.submission_error = Some(crate::i18n::format!("无法创建工作树：{error}" => "Unable to create worktree: {error}"));
                    }
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
        true
    }

    pub(super) fn resume_worktree_send(&mut self, cx: &mut Context<Self>) {
        if self.permission_catalog_loading {
            return;
        }
        if let Some((text, inverted)) = self.workspace_controls.pending_send.take() {
            if self.permission_catalog_error.is_none() && self.prompt_text(cx) == text {
                self.submit_prompt_as(text, inverted, cx);
            } else if let Some(error) = &self.permission_catalog_error {
                self.submission_error = Some(error.clone());
            }
        }
    }
}
