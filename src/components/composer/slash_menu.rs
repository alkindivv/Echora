//! Slash command discovery, availability, filtering and keyboard navigation.
//! Commands delegate to the catalog, shell, review and native editor workflows.
//! A submenu retains the slash token until a terminal action is selected.

use std::ops::Range;

use gpui::Context;

use super::{ComposerView, ConversationChanged, toast::ToastKind};
use crate::conversation::AutoApprovalReviewPresentation;

const DENIAL_LIMIT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    Approve,
    CodeReview,
    Compact,
    Goal,
    Memories,
    PlanMode,
    Model,
    Reasoning,
    Local,
    Init,
    Mcp,
    Project,
    Worktree,
    Chat,
    Side,
    Fork,
    ForkWorktree,
    Archive,
    New,
    Pin,
    Rename,
    Status,
    Feedback,
    Sketch,
    Pet,
    Skill { index: usize, cycle: u64 },
}

impl SlashCommand {
    const ALL: [Self; 24] = [
        Self::Approve,
        Self::CodeReview,
        Self::Compact,
        Self::Goal,
        Self::Memories,
        Self::PlanMode,
        Self::Model,
        Self::Reasoning,
        Self::Local,
        Self::Init,
        Self::Mcp,
        Self::Project,
        Self::Worktree,
        Self::Chat,
        Self::Side,
        Self::Fork,
        Self::Archive,
        Self::New,
        Self::Pin,
        Self::Rename,
        Self::Status,
        Self::Feedback,
        Self::Sketch,
        Self::Pet,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Approve => "autoreview",
            Self::CodeReview => "review",
            Self::Compact => "compact",
            Self::Goal => "goal",
            Self::Memories => "memories",
            Self::PlanMode => "plan-mode",
            Self::Model => "model",
            Self::Reasoning => "reasoning",
            Self::Local => "local",
            Self::Init => "init",
            Self::Mcp => "mcp",
            Self::Project => "project",
            Self::Worktree => "worktree",
            Self::Chat => "chat",
            Self::Side => "side",
            Self::Fork | Self::ForkWorktree => "fork",
            Self::Archive => "archive",
            Self::New => "new",
            Self::Pin => "pin",
            Self::Rename => "rename",
            Self::Status => "status",
            Self::Feedback => "feedback",
            Self::Sketch => "sketch",
            Self::Pet => "pet",

            Self::Skill { .. } => "",
        }
    }

    pub(crate) fn title(self) -> String {
        match self {
            Self::Approve => crate::i18n::format!("批准" => "Approve"),
            Self::CodeReview => crate::i18n::format!("代码审查" => "Code review"),
            Self::Compact => crate::i18n::format!("压缩" => "Compact"),
            Self::Goal => crate::i18n::format!("目标" => "Goal"),
            Self::Memories => crate::i18n::format!("记忆" => "Memories"),
            Self::PlanMode => crate::i18n::format!("计划模式" => "Plan mode"),
            Self::Model => crate::i18n::format!("模型"=>"Model"),
            Self::Reasoning => crate::i18n::format!("推理"=>"Reasoning"),
            Self::Local => crate::i18n::format!("在本地运行"=>"Run locally"),
            Self::Init => crate::i18n::format!("初始化" => "Init"),
            Self::Mcp => "MCP".into(),
            Self::Project => crate::i18n::format!("在项目中使用 Work" => "Work in a project"),
            Self::Worktree => crate::i18n::format!("新的工作树" => "New worktree"),
            Self::Chat => crate::i18n::format!("聊天" => "Chat"),
            Self::Side => crate::i18n::format!("侧边" => "Side"),
            Self::Fork | Self::ForkWorktree => crate::i18n::format!("创建聊天分支" => "Fork chat"),
            Self::Archive => crate::i18n::format!("归档" => "Archive"),
            Self::New => crate::i18n::format!("新聊天" => "New chat"),
            Self::Pin => crate::i18n::format!("置顶聊天" => "Pin chat"),
            Self::Rename => crate::i18n::format!("重命名" => "Rename"),
            Self::Status => crate::i18n::format!("状态" => "Status"),
            Self::Feedback => crate::i18n::format!("反馈" => "Feedback"),
            Self::Sketch => crate::i18n::format!("绘图" => "Sketch"),
            Self::Pet => crate::i18n::format!("虚拟宠物" => "Pet"),
            Self::Skill { .. } => crate::i18n::format!("技能" => "Skills"),
        }
    }

    fn requires_empty_composer(self) -> bool {
        matches!(
            self,
            Self::Compact
                | Self::CodeReview
                | Self::Init
                | Self::Fork
                | Self::Archive
                | Self::New
                | Self::Pin
                | Self::Rename
        )
    }

    /// The row shows a chevron: choosing it opens a submenu.
    pub(crate) fn opens_submenu(self) -> bool {
        matches!(
            self,
            Self::CodeReview
                | Self::Mcp
                | Self::Project
                | Self::Fork
                | Self::Model
                | Self::Reasoning
        )
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Approve => "auto-review-shield",
            Self::CodeReview => "slash-review",
            Self::Compact => "context-compaction",
            Self::Goal => "goal-chip",
            Self::Memories => "slash-memories",
            Self::PlanMode => "slash-plan-mode",
            Self::Model => "model-reset",
            Self::Reasoning => "slash-plan-mode",
            Self::Local => "local",
            Self::Init => "command-init",
            Self::Mcp => "command-mcp",
            Self::Project => "utility-folder",
            Self::Worktree => "project-worktree",
            Self::Chat => "command-chat",
            Self::Side => "side-chat",
            Self::Fork | Self::ForkWorktree => "project-worktree",
            Self::Archive => "archive",
            Self::New => "new-chat",
            Self::Pin => "pin",
            Self::Rename => "project-marker-pencil",
            Self::Status => "command-status",
            Self::Feedback => "command-feedback",
            Self::Sketch => "command-sketch",
            Self::Pet => "command-pet",

            Self::Skill { .. } => "skill-cube",
        }
    }
}

/// One row of the top-level menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlashItem {
    pub(crate) command: SlashCommand,
    pub(crate) title: String,
    pub(crate) description: String,
    /// Title byte ranges the query matched; the rest dims while typing.
    pub(crate) matched: Vec<Range<usize>>,
}

/// One row of the Approve submenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DenialItem {
    pub(crate) review: AutoApprovalReviewPresentation,
    pub(crate) title: String,
    pub(crate) detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlashMenu {
    pub(crate) highlighted: usize,
    pub(crate) denials: bool,
    /// The Code review submenu is showing.
    pub(crate) review: bool,
    pub(crate) mcp: bool,
    pub(crate) project: bool,
    pub(crate) fork: bool,
    pub(crate) models: bool,
    pub(crate) reasoning: bool,
    /// Escape closes the menu until the query changes.
    dismissed: Option<String>,
}

/// Matches `query` as a subsequence of `text` (case-insensitive) and scores
/// it: a prefix beats a contiguous match, which beats a scattered one.
pub(super) fn fuzzy(text: &str, query: &str) -> Option<(u32, Vec<Range<usize>>)> {
    if query.is_empty() {
        return Some((1, Vec::new()));
    }
    let lower = text.to_lowercase();
    // Byte offsets of the lowercase copy only map back when lowercasing kept
    // every length.
    if lower.len() == text.len()
        && let Some(start) = lower.find(query)
    {
        let score = if start == 0 { 30 } else { 20 };
        return Some((score, std::iter::once(start..start + query.len()).collect()));
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = query.chars().peekable();
    for (index, ch) in text.char_indices() {
        let Some(next) = wanted.peek() else {
            break;
        };
        if ch.to_lowercase().eq(next.to_lowercase()) {
            wanted.next();
            let end = index + ch.len_utf8();
            match ranges.last_mut() {
                Some(last) if last.end == index => last.end = end,
                _ => ranges.push(index..end),
            }
        }
    }
    wanted.peek().is_none().then_some((10, ranges))
}

/// `/gooo` still finds Goal: repeated letters collapse, as the reference
/// normalises the goal query.
fn collapse_repeats(query: &str) -> String {
    let mut out = String::new();
    for ch in query.chars() {
        if !out.ends_with(ch) {
            out.push(ch);
        }
    }
    out
}

/// The `/query` token that ends at `caret`: the nearest `/` on the caret's
/// line that starts the line or follows whitespace. Returns the token's byte
/// range (from the `/` to the caret).
pub(super) fn token_range(text: &str, caret: usize) -> Option<Range<usize>> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let line_start = text[..caret].rfind('\n').map_or(0, |index| index + 1);
    let line = &text[line_start..caret];
    line.char_indices()
        .rev()
        .filter(|(_, ch)| *ch == '/')
        .find(|(index, _)| {
            line[..*index]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace)
        })
        .map(|(index, _)| line_start + index..caret)
}

/// The reference's "empty composer" for Compact: nothing but one `/…` line.
fn composer_is_only_token(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('/') && !text.contains('\n')
}

impl ComposerView {
    /// The `/query` token at the caret, with its lowercased query.
    pub(super) fn slash_token(&self, cx: &gpui::App) -> Option<(Range<usize>, String)> {
        let editor = self.prompt_editor.read(cx);
        let range = token_range(editor.text(), editor.cursor())?;
        let query = editor.text()[range.start + 1..range.end].to_lowercase();
        Some((range, query))
    }

    pub(super) fn slash_query(&self, cx: &gpui::App) -> Option<String> {
        self.slash_token(cx).map(|(_, query)| query)
    }

    pub(super) fn command_available(&self, command: SlashCommand) -> bool {
        match command {
            SlashCommand::Approve => !self
                .conversation
                .approvable_denials(DENIAL_LIMIT)
                .is_empty(),
            SlashCommand::CodeReview => self.review_available(),
            SlashCommand::Compact => self.conversation.thread_id.is_some() && !self.side_chat,
            SlashCommand::Goal => !self.side_chat,
            SlashCommand::Memories => !self.side_chat && self.memories_feature_enabled(),
            SlashCommand::PlanMode => self.plan_mode_available(),
            SlashCommand::Model | SlashCommand::Reasoning => self.command_model_available(),
            SlashCommand::Local => {
                !self.side_chat
                    && self.thread_id().is_none()
                    && self.workspace_controls.worktree_mode
            }
            SlashCommand::Init => {
                !self.side_chat
                    && !self.is_running()
                    && !self.goal_draft
                    && self.conversation.cwd != std::path::Path::new("/")
            }
            SlashCommand::Fork | SlashCommand::ForkWorktree => {
                !self.side_chat && self.thread_id().is_some() && !self.command_catalog.fork_loading
            }
            SlashCommand::Side
            | SlashCommand::Archive
            | SlashCommand::New
            | SlashCommand::Pin
            | SlashCommand::Rename => !self.side_chat && self.thread_id().is_some(),
            SlashCommand::Status
            | SlashCommand::Feedback
            | SlashCommand::Sketch
            | SlashCommand::Pet => true,
            SlashCommand::Mcp => true,
            SlashCommand::Project => !self.side_chat && self.thread_id().is_none(),
            SlashCommand::Worktree => {
                !self.side_chat
                    && self.thread_id().is_none()
                    && self
                        .workspace
                        .checkout
                        .as_ref()
                        .is_some_and(crate::git_review::Checkout::is_repository)
            }
            SlashCommand::Chat => {
                !self.side_chat
                    && self.thread_id().is_none()
                    && self.conversation.project_id.is_some()
            }
            SlashCommand::Skill { .. } => true,
        }
    }

    pub(super) fn command_description(&self, command: SlashCommand) -> String {
        match command {
            SlashCommand::Approve => {
                crate::i18n::format!("批准最近一次自动审查驳回" => "Approve a recent auto-review denial")
            }
            SlashCommand::CodeReview => crate::i18n::format!(
                "审查未提交的更改，或与某个分支进行比较" => "Review uncommitted changes or compare against a branch"
            ),
            SlashCommand::Compact => match self.context_usage_percent() {
                Some(usage) => crate::i18n::format!(
                    "压缩此聊天的上下文（已使用 {usage}%）" => "Compact this chat's context ({usage}% full)"
                ),
                None => crate::i18n::format!("压缩此聊天的上下文" => "Compact this chat's context"),
            },
            SlashCommand::Goal => {
                crate::i18n::format!("设置要持续追求的目标" => "Set a goal to keep pursuing")
            }
            // The reference's row has a title only.
            SlashCommand::Memories | SlashCommand::Model => String::new(),
            SlashCommand::Reasoning => self.selected_effort_label(),
            SlashCommand::Local => {
                crate::i18n::format!("在本地运行此聊天"=>"Run this chat locally")
            }
            SlashCommand::Init => {
                crate::i18n::format!("创建包含 Codex 指令的 AGENTS.md 文件" => "Create an AGENTS.md file with instructions for Codex")
            }
            SlashCommand::Mcp => {
                crate::i18n::format!("显示 MCP 服务器状态" => "Show MCP server status")
            }
            SlashCommand::Project => {
                crate::i18n::format!("为新聊天选择项目" => "Choose a project for a new chat")
            }
            SlashCommand::Worktree => {
                if self.workspace_controls.worktree_mode {
                    crate::i18n::format!("关闭工作树模式" => "Turn worktree mode off")
                } else {
                    crate::i18n::format!("在新的工作树中运行此聊天" => "Run this chat in a new worktree")
                }
            }
            SlashCommand::Chat => {
                crate::i18n::format!("不要在项目中工作" => "Don't work in a project")
            }
            SlashCommand::Side => {
                crate::i18n::format!("发起临时侧边聊天" => "Start a temporary side chat")
            }
            SlashCommand::Fork | SlashCommand::ForkWorktree => {
                crate::i18n::format!("在当前工作空间或新工作树中创建此聊天的分支" => "Fork this chat in the current workspace or a new worktree")
            }
            SlashCommand::Archive => {
                crate::i18n::format!("归档当前聊天" => "Archive the current chat")
            }
            SlashCommand::New => {
                crate::i18n::format!("在同一工作空间中开启空白聊天" => "Start a new chat")
            }
            SlashCommand::Pin => {
                crate::i18n::format!("将此聊天保留在侧边栏" => "Keep this chat in the sidebar")
            }
            SlashCommand::Rename => {
                crate::i18n::format!("重命名当前聊天" => "Rename the current chat")
            }
            SlashCommand::Status => {
                crate::i18n::format!("显示聊天 ID、上下文用量和速率限制" => "Show current chat status and usage")
            }
            SlashCommand::Feedback => {
                crate::i18n::format!("发送有关此聊天的反馈" => "Send feedback")
            }
            SlashCommand::Sketch => {
                crate::i18n::format!("绘制草图" => "Draw and attach a sketch")
            }
            SlashCommand::Pet => {
                crate::i18n::format!("唤醒或收起桌面虚拟宠物" => "Toggle the desktop pet")
            }
            SlashCommand::Skill { .. } => String::new(),
            SlashCommand::PlanMode => {
                if self.prompt_context.plan_mode == Some(true) {
                    crate::i18n::format!("关闭计划模式" => "Turn plan mode off")
                } else {
                    crate::i18n::format!("开启计划模式" => "Turn plan mode on")
                }
            }
        }
    }

    /// The share of the model context the last turn used, as the reference
    /// computes it: `min(last.totalTokens, window) / window`, rounded.
    pub(super) fn context_usage_percent(&self) -> Option<i64> {
        let usage = self
            .conversation
            .thread_token_usages
            .get(self.conversation.thread_id.as_deref()?)?;
        let window = usage.model_context_window.filter(|window| *window > 0)?;
        let used = usage.last.total_tokens.clamp(0, window) as f64;
        Some((used * 100.0 / window as f64).round() as i64)
    }

    /// The visible top-level rows for the current query.
    pub(crate) fn slash_items(&self, cx: &gpui::App) -> Vec<SlashItem> {
        let Some(query) = self.slash_query(cx) else {
            return Vec::new();
        };
        let only_token = composer_is_only_token(self.prompt_editor.read(cx).text());
        let mut items = SlashCommand::ALL
            .into_iter()
            .filter(|command| self.command_available(*command))
            .filter(|command| only_token || !command.requires_empty_composer())
            .filter_map(|command| {
                let title = if command == SlashCommand::Worktree {
                    let project = self
                        .workspace
                        .project_label
                        .as_ref()
                        .map(|s| s.as_ref())
                        .unwrap_or("");
                    crate::i18n::format!("新的 {project} 工作树" => "New {project} worktree")
                } else {
                    command.title()
                };
                let query = if command == SlashCommand::Goal {
                    collapse_repeats(&query)
                } else {
                    query.clone()
                };
                let by_title = fuzzy(&title, &query);
                let by_id = fuzzy(command.id(), &query).map(|(score, _)| (score, Vec::new()));
                let (score, matched) = match (by_title, by_id) {
                    // The id may rank the command, but the title still shows
                    // what matched (the reference's "Code **review**").
                    (Some(title), Some(id)) if id.0 > title.0 => (id.0, title.1),
                    (Some(title), _) => title,
                    (None, Some(id)) => id,
                    (None, None) => return None,
                };
                Some((
                    score,
                    SlashItem {
                        command,
                        description: self.command_description(command),
                        title,
                        matched: if query.is_empty() {
                            Vec::new()
                        } else {
                            matched
                        },
                    },
                ))
            })
            .collect::<Vec<_>>();
        items.sort_by(|(a_score, a), (b_score, b)| {
            b_score.cmp(a_score).then_with(|| a.title.cmp(&b.title))
        });
        let mut items: Vec<_> = items.into_iter().map(|(_, item)| item).collect();
        items.extend(self.command_skill_items(&query));
        items
    }

    pub(crate) fn denial_items(&self) -> Vec<DenialItem> {
        let approving = self.conversation.approving_review.clone();
        self.conversation
            .approvable_denials(DENIAL_LIMIT)
            .into_iter()
            .map(|review| {
                let detail = if approving.as_deref() == Some(review.review.key.review_id.as_str()) {
                    crate::i18n::format!("正在记录批准操作…" => "Recording approval…")
                } else {
                    review
                        .review
                        .rationale
                        .clone()
                        .filter(|text| !text.trim().is_empty())
                        .unwrap_or_else(|| {
                            crate::i18n::format!("自动审查未提供理由" => "Auto-review did not include a rationale")
                        })
                };
                DenialItem {
                    title: crate::components::auto_approval::action_label(&review.review.action),
                    detail,
                    review,
                }
            })
            .collect()
    }

    /// Whether the menu is showing. A query with a space that matches
    /// nothing closes it, so the text can be sent as typed.
    pub(crate) fn slash_menu_open(&self, cx: &gpui::App) -> bool {
        let Some(menu) = &self.slash_menu else {
            return false;
        };
        let Some(query) = self.slash_query(cx) else {
            return false;
        };
        if menu.dismissed.as_ref() == Some(&query) {
            return false;
        }
        menu.denials
            || menu.review
            || menu.mcp
            || menu.project
            || menu.fork
            || menu.models
            || menu.reasoning
            || !query.contains(char::is_whitespace)
            || !self.slash_items(cx).is_empty()
    }

    pub(super) fn dismiss_slash_menu(&mut self, cx: &mut Context<Self>) {
        let query = self.slash_query(cx);
        if let Some(menu) = &mut self.slash_menu {
            menu.review = false;
            menu.denials = false;
            menu.mcp = false;
            menu.project = false;
            menu.fork = false;
            menu.models = false;
            menu.reasoning = false;
            menu.dismissed = query;
        }
        cx.notify();
    }

    /// Removes the `/query` token, leaving the rest of the composer text.
    pub(super) fn remove_slash_token(&mut self, cx: &mut Context<Self>) {
        if let Some((range, _)) = self.slash_token(cx) {
            self.prompt_editor
                .update(cx, |editor, cx| editor.replace_range(range, "", cx));
        }
    }

    /// Follows the composer text: a slash token opens the menu, anything else
    /// closes it.
    pub(super) fn update_slash_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu_open {
            self.slash_menu = None;
            return;
        }
        let Some(query) = self.slash_query(cx) else {
            self.slash_menu = None;
            return;
        };
        if self.slash_menu.is_none() {
            self.ensure_memories_feature(cx);
            self.ensure_command_skills(cx);
        }
        let menu = self.slash_menu.get_or_insert_with(SlashMenu::default);
        if menu
            .dismissed
            .as_ref()
            .is_some_and(|dismissed| dismissed != &query)
        {
            menu.dismissed = None;
        }
        // The review submenu filters by what is typed after its `/`, so an
        // edit keeps it; only the denials submenu resets.
        menu.highlighted = 0;
        menu.denials = false;
        cx.notify();
    }

    fn slash_row_count(&self, cx: &gpui::App) -> usize {
        match &self.slash_menu {
            Some(menu) if menu.denials => self.denial_items().len(),
            Some(menu) if menu.review => self.review_rows(cx).len(),
            Some(menu) if menu.mcp => 0,
            Some(menu) if menu.models || menu.reasoning => self.command_model_rows(cx).len(),
            Some(menu) if menu.fork => self.command_fork_rows(cx).len(),
            Some(menu) if menu.project => self.command_project_rows(cx).len(),
            _ => self.slash_items(cx).len(),
        }
    }

    /// Up/Down/Enter/Escape while the menu shows. Returns whether the key
    /// was the menu's.
    pub(super) fn slash_menu_key(&mut self, key: &str, ctrl: bool, cx: &mut Context<Self>) -> bool {
        if self.prompt_editor.read(cx).composing() || !self.slash_menu_open(cx) {
            return false;
        }
        let count = self.slash_row_count(cx);
        let query = self.slash_query(cx);
        let Some(menu) = self.slash_menu.as_mut() else {
            return false;
        };
        match (key, ctrl) {
            ("down", false) | ("n", true) if count > 0 => {
                menu.highlighted = (menu.highlighted + 1) % count;
            }
            ("up", false) | ("p", true) if count > 0 => {
                menu.highlighted = (menu.highlighted + count - 1) % count;
            }
            ("tab", false) => {
                return self.slash_menu_enter(cx);
            }
            ("escape", false) => {
                if menu.denials {
                    menu.denials = false;
                    menu.highlighted = 0;
                } else {
                    // As the reference, Escape in the review submenu closes
                    // the whole menu and leaves its `/` in the composer.
                    menu.review = false;
                    menu.mcp = false;
                    menu.project = false;
                    menu.fork = false;
                    menu.models = false;
                    menu.reasoning = false;
                    menu.dismissed = query;
                }
            }
            _ => return false,
        }
        let highlighted = menu.highlighted;
        if count > 0 {
            self.slash_scroll
                .scroll_to_item(self.slash_scroll_index(highlighted, cx));
        }
        cx.notify();
        true
    }

    fn slash_scroll_index(&self, index: usize, cx: &gpui::App) -> usize {
        match self.slash_menu.as_ref() {
            Some(menu) if menu.denials => index + 1,
            Some(menu) if menu.review => index + usize::from(index > 0),
            Some(menu) if menu.project => {
                index
                    + usize::from(self.project_picker.loading)
                    + usize::from(self.project_picker.error.is_some())
            }
            Some(menu) if menu.models => index + 1,
            Some(menu) if menu.mcp || menu.fork || menu.reasoning => index,
            _ => {
                index
                    + usize::from(
                        self.slash_items(cx)
                            .get(index)
                            .is_some_and(|item| matches!(item.command, SlashCommand::Skill { .. })),
                    )
            }
        }
    }

    /// Enter on the menu. Returns whether the menu took it.
    pub(super) fn slash_menu_enter(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.slash_menu_open(cx) {
            return false;
        }
        let Some(menu) = self.slash_menu.clone() else {
            return false;
        };
        if menu.mcp {
            return true;
        }
        if menu.models || menu.reasoning {
            self.select_command_model(menu.highlighted, cx);
        } else if menu.fork {
            self.select_command_fork(menu.highlighted, cx);
        } else if menu.project {
            self.select_command_project(menu.highlighted, cx);
        } else if menu.denials {
            self.select_denial(menu.highlighted, cx);
        } else if menu.review {
            self.select_review_row(menu.highlighted, cx);
        } else {
            let Some(item) = self.slash_items(cx).into_iter().nth(menu.highlighted) else {
                return true;
            };
            self.select_slash_command(item.command, cx);
        }
        true
    }

    /// Runs a command. The `/query` text is removed first.
    pub(crate) fn select_slash_command(&mut self, command: SlashCommand, cx: &mut Context<Self>) {
        if !self.command_available(command) {
            return;
        }
        if self.select_catalog_command(command, cx) {
            return;
        }
        if command == SlashCommand::Approve {
            if let Some(menu) = self.slash_menu.as_mut() {
                menu.denials = true;
                menu.highlighted = 0;
            }
            cx.notify();
            return;
        }
        if command == SlashCommand::CodeReview {
            if let Some((range, _)) = self.slash_token(cx) {
                self.prompt_editor
                    .update(cx, |editor, cx| editor.replace_range(range, "/", cx));
            }
            if let Some(menu) = self.slash_menu.as_mut() {
                menu.review = true;
                menu.highlighted = 0;
                menu.dismissed = None;
            }
            self.open_review_menu(cx);
            return;
        }
        self.slash_menu = None;
        self.remove_slash_token(cx);
        match command {
            SlashCommand::Goal => {
                if self.prompt_context.plan_mode == Some(true) {
                    self.prompt_context.plan_mode = Some(false);
                }
                self.set_goal_draft(true, cx);
                self.focus_prompt_pending = true;
            }
            SlashCommand::Compact => {
                if self.is_running() {
                    self.show_toast(
                        ToastKind::Danger,
                        crate::i18n::format!("聊天期间无法使用 Compact" => "Compact is disabled while a chat is in progress"),
                        cx,
                    );
                } else {
                    self.start_context_compaction(cx);
                }
            }
            SlashCommand::PlanMode => {
                let on = self.prompt_context.plan_mode != Some(true);
                self.prompt_context.plan_mode = Some(on);
                self.focus_prompt_pending = true;
            }
            SlashCommand::Memories => self.open_memories_dialog(cx),
            SlashCommand::Side
            | SlashCommand::Fork
            | SlashCommand::ForkWorktree
            | SlashCommand::Archive
            | SlashCommand::New
            | SlashCommand::Pin
            | SlashCommand::Rename => cx.emit(super::SlashChatAction(command)),
            SlashCommand::Status => self.open_command_status(cx),
            SlashCommand::Feedback => self.open_command_feedback(cx),
            SlashCommand::Sketch => self.open_sketch(cx),
            SlashCommand::Pet => self.toggle_pet(cx),
            SlashCommand::Local => {
                self.select_run_location(false, cx);
                self.focus_prompt_pending = true;
            }
            SlashCommand::Worktree => {
                self.select_run_location(!self.workspace_controls.worktree_mode, cx);
                self.focus_prompt_pending = true;
            }
            SlashCommand::Chat => {
                cx.emit(super::DraftProjectSelected(None));
                self.focus_prompt_pending = true;
            }
            SlashCommand::Approve
            | SlashCommand::CodeReview
            | SlashCommand::Init
            | SlashCommand::Mcp
            | SlashCommand::Project
            | SlashCommand::Model
            | SlashCommand::Reasoning
            | SlashCommand::Skill { .. } => {
                unreachable!("opens its submenu above")
            }
        }
        cx.emit(ConversationChanged);
        cx.notify();
    }

    /// Approves one denial from the submenu. The menu stays open while it
    /// records, and closes once the approval lands.
    pub(crate) fn select_denial(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.conversation.approving_review.is_some() {
            return;
        }
        let Some(item) = self.denial_items().into_iter().nth(index) else {
            return;
        };
        self.approve_review(item.review.review.key.clone(), cx);
    }

    /// A recorded approval closes the Approve submenu.
    pub(super) fn close_denial_menu(&mut self, cx: &mut Context<Self>) {
        if self.slash_menu.as_ref().is_some_and(|menu| menu.denials) {
            self.remove_slash_token(cx);
            self.slash_menu = None;
        }
    }

    pub(crate) fn denial_approval_in_flight(&self) -> bool {
        self.conversation.approving_review.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{collapse_repeats, composer_is_only_token, fuzzy, token_range as slash_token};

    #[test]
    fn the_token_starts_at_a_line_start_or_after_whitespace_and_ends_at_the_caret() {
        assert_eq!(slash_token("/go", 3), Some(0..3));
        assert_eq!(slash_token("fix this /go", 12), Some(9..12));
        assert_eq!(slash_token("and/or", 6), None, "not after a letter");
        assert_eq!(
            slash_token("/goal and/or", 12),
            Some(0..12),
            "spaces stay in the query"
        );
        assert_eq!(slash_token("line\n/co", 8), Some(5..8));
        assert_eq!(slash_token("/co\nnext", 9), None, "the caret's line only");
        assert_eq!(
            slash_token("/compact rest", 3),
            Some(0..3),
            "up to the caret"
        );
        assert!(composer_is_only_token("/compact foo"));
        assert!(!composer_is_only_token("hello /compact"));
    }

    #[test]
    fn fuzzy_prefers_prefixes_and_marks_the_matched_title_parts() {
        assert_eq!(
            fuzzy("Goal", "go"),
            Some((30, std::iter::once(0..2).collect()))
        );
        assert_eq!(
            fuzzy("Plan mode", "mode"),
            Some((20, std::iter::once(5..9).collect()))
        );
        assert_eq!(fuzzy("Plan mode", "pm"), Some((10, vec![0..1, 5..6])));
        assert_eq!(fuzzy("Compact", "x"), None);
        assert_eq!(collapse_repeats("gooo"), "go");
    }
}
