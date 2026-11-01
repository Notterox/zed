//! NOTTEROX: the rows of the inline code actions menu ([`CodeActionsMenu`]): actions grouped
//! under headers by kind, the editor's own refactoring and navigation commands, and search by
//! typing while the menu is open. Kept apart from `code_context_menus.rs` so the fork's edits
//! to upstream files stay small hooks (see FORK.md).

use std::rc::Rc;

use gpui::{
    Action, AnyElement, Context, Entity, FontWeight, HighlightStyle, Keystroke, ScrollStrategy,
    StyledText, Subscription, Window,
};
use itertools::Itertools as _;
use language::Buffer;
use ui::{ListItem, ListSubHeader, prelude::*};

use crate::{
    CodeActionSource, Editor,
    actions::ConfirmCodeAction,
    code_context_menus::{
        CODE_ACTION_MENU_MAX_WIDTH, CODE_ACTION_MENU_MIN_WIDTH, CodeActionContents,
        CodeActionsItem, CodeActionsMenu, CodeContextMenu,
    },
};

/// Sections of the code actions menu, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CodeActionsGroup {
    QuickFix,
    Refactor,
    Extract,
    Inline,
    Source,
    Navigate,
    Run,
    Debug,
    Other,
}

impl CodeActionsGroup {
    fn for_item(item: &CodeActionsItem) -> Self {
        match item {
            CodeActionsItem::Task(..) => Self::Run,
            CodeActionsItem::DebugScenario(_) => Self::Debug,
            CodeActionsItem::CodeAction { action, .. } => {
                let kind = action.lsp_action.action_kind();
                match kind.as_ref().map(|kind| kind.as_str()) {
                    Some(kind) if kind.starts_with("quickfix") => Self::QuickFix,
                    Some(kind) if kind.starts_with("refactor.extract") => Self::Extract,
                    Some(kind) if kind.starts_with("refactor.inline") => Self::Inline,
                    Some(kind) if kind.starts_with("refactor") => Self::Refactor,
                    Some(kind) if kind.starts_with("source") => Self::Source,
                    _ => Self::Other,
                }
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::QuickFix => "Quick Fixes",
            Self::Refactor => "Refactor",
            Self::Extract => "Extract",
            Self::Inline => "Inline",
            Self::Source => "Source",
            Self::Navigate => "Navigate",
            Self::Run => "Run",
            Self::Debug => "Debug",
            Self::Other => "Other",
        }
    }
}

/// An editor command listed in the menu next to the language server's actions.
pub(crate) struct CodeActionsMenuCommand {
    label: SharedString,
    group: CodeActionsGroup,
    action: Box<dyn Action>,
}

impl CodeActionsMenuCommand {
    /// The editor's rename, navigation and source commands.
    fn defaults() -> Vec<Self> {
        use crate::actions::{
            FindAllReferences, Format, GoToImplementation, GoToTypeDefinition, OrganizeImports,
            Rename,
        };
        let command = |label: &'static str, group, action: Box<dyn Action>| Self {
            label: label.into(),
            group,
            action,
        };
        vec![
            command("Rename…", CodeActionsGroup::Refactor, Box::new(Rename)),
            command(
                "Find All References",
                CodeActionsGroup::Navigate,
                Box::new(FindAllReferences::default()),
            ),
            command(
                "Go to Implementation",
                CodeActionsGroup::Navigate,
                Box::new(GoToImplementation::default()),
            ),
            command(
                "Go to Type Definition",
                CodeActionsGroup::Navigate,
                Box::new(GoToTypeDefinition::default()),
            ),
            command(
                "Show Incoming Calls",
                CodeActionsGroup::Navigate,
                Box::new(zed_actions::ShowIncomingCalls),
            ),
            command(
                "Organize Imports",
                CodeActionsGroup::Source,
                Box::new(OrganizeImports),
            ),
            command(
                "Format Document",
                CodeActionsGroup::Source,
                Box::new(Format),
            ),
        ]
    }
}

/// What a row runs: an index into [`CodeActionContents`] or into the menu's editor commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeActionsMenuTarget {
    Action(usize),
    Command(usize),
}

#[derive(Clone)]
pub(crate) enum CodeActionsMenuRow {
    Header(CodeActionsGroup),
    Item {
        target: CodeActionsMenuTarget,
        label: SharedString,
        /// Byte offsets of the characters matching the search query.
        positions: Vec<usize>,
    },
}

impl CodeActionsMenuRow {
    pub(crate) fn render(
        &self,
        ix: usize,
        selected_ix: usize,
        is_quick_action_bar: bool,
        cx: &mut Context<Editor>,
    ) -> AnyElement {
        let (label, positions) = match self {
            Self::Header(group) => {
                return div()
                    .child(ListSubHeader::new(group.label()).inset(true))
                    .into_any_element();
            }
            Self::Item {
                label, positions, ..
            } => (label, positions),
        };
        let selected = ix == selected_ix;
        let colors = cx.theme().colors();
        let match_highlights = positions.iter().map(|&start| {
            let len = label[start..].chars().next().map_or(0, char::len_utf8);
            (start..start + len, HighlightStyle::from(FontWeight::BOLD))
        });

        ListItem::new(ix)
            .inset(true)
            .toggle_state(selected)
            .overflow_x()
            .child(
                div()
                    .min_w(CODE_ACTION_MENU_MIN_WIDTH)
                    .max_w(CODE_ACTION_MENU_MAX_WIDTH)
                    .overflow_hidden()
                    .text_ellipsis()
                    .when(is_quick_action_bar, |this| this.text_ui(cx))
                    .when(selected, |this| this.text_color(colors.text_accent))
                    .child(StyledText::new(label.clone()).with_highlights(match_highlights)),
            )
            .on_click(cx.listener(move |editor, _, window, cx| {
                cx.stop_propagation();
                if let Some(CodeContextMenu::CodeActions(menu)) =
                    editor.context_menu.borrow_mut().as_mut()
                {
                    menu.selected_item = ix;
                }
                if let Some(task) =
                    editor.confirm_code_action(&ConfirmCodeAction { item_ix: None }, window, cx)
                {
                    task.detach_and_log_err(cx)
                }
            }))
            .into_any_element()
    }
}

pub(crate) struct CodeActionsMenuRows {
    commands: Vec<CodeActionsMenuCommand>,
    /// Text typed while the menu is open; filters the rows.
    query: String,
    /// One candidate per command, then one per action; ids index `targets`.
    candidates: Vec<fuzzy_nucleo::StringMatchCandidate>,
    targets: Vec<(CodeActionsMenuTarget, CodeActionsGroup)>,
    rows: Rc<[CodeActionsMenuRow]>,
}

impl CodeActionsMenuRows {
    pub(crate) fn new(actions: &CodeActionContents, commands: Vec<CodeActionsMenuCommand>) -> Self {
        let commands_iter = commands.iter().enumerate().map(|(ix, command)| {
            (
                CodeActionsMenuTarget::Command(ix),
                command.group,
                command.label.to_string(),
            )
        });
        let actions_iter = actions.iter().enumerate().map(|(ix, item)| {
            (
                CodeActionsMenuTarget::Action(ix),
                CodeActionsGroup::for_item(&item),
                item.menu_label(),
            )
        });
        let mut targets = Vec::new();
        let mut candidates = Vec::new();
        for (id, (target, group, label)) in commands_iter.chain(actions_iter).enumerate() {
            targets.push((target, group));
            candidates.push(fuzzy_nucleo::StringMatchCandidate::new(id, label));
        }

        let mut rows = Self {
            commands,
            query: String::new(),
            candidates,
            targets,
            rows: Rc::from(Vec::new()),
        };
        rows.filter();
        rows
    }

    /// Whether the menu has anything to show, before filtering.
    pub(crate) fn has_targets(&self) -> bool {
        !self.targets.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn rows(&self) -> Rc<[CodeActionsMenuRow]> {
        self.rows.clone()
    }

    pub(crate) fn target(&self, ix: usize) -> Option<CodeActionsMenuTarget> {
        match self.rows.get(ix)? {
            CodeActionsMenuRow::Item { target, .. } => Some(*target),
            CodeActionsMenuRow::Header(_) => None,
        }
    }

    pub(crate) fn command_action(&self, ix: usize) -> Option<Box<dyn Action>> {
        Some(self.commands.get(ix)?.action.boxed_clone())
    }

    pub(crate) fn label(
        &self,
        target: CodeActionsMenuTarget,
        actions: &CodeActionContents,
    ) -> Option<String> {
        Some(match target {
            CodeActionsMenuTarget::Action(ix) => actions.get(ix)?.menu_label(),
            CodeActionsMenuTarget::Command(ix) => self.commands.get(ix)?.label.to_string(),
        })
    }

    /// The row the list sizes its width from.
    pub(crate) fn widest_row(&self) -> Option<usize> {
        self.rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| match row {
                CodeActionsMenuRow::Item { label, .. } => label.chars().count(),
                CodeActionsMenuRow::Header(group) => group.label().chars().count(),
            })
            .map(|(ix, _)| ix)
    }

    /// The first selectable row, counted from the bottom when `from_end`.
    pub(crate) fn first(&self, from_end: bool) -> Option<usize> {
        let start = if from_end {
            self.rows.len().saturating_sub(1)
        } else {
            0
        };
        self.find_selectable(start, !from_end)
    }

    /// The selectable row after (or before) `from`, wrapping around and skipping headers.
    pub(crate) fn step(&self, from: usize, forward: bool) -> Option<usize> {
        let len = self.rows.len();
        if len == 0 {
            return None;
        }
        let start = if forward {
            (from + 1) % len
        } else {
            (from + len - 1) % len
        };
        self.find_selectable(start, forward)
    }

    fn find_selectable(&self, start: usize, forward: bool) -> Option<usize> {
        let len = self.rows.len();
        let mut ix = start;
        for _ in 0..len {
            if matches!(self.rows.get(ix), Some(CodeActionsMenuRow::Item { .. })) {
                return Some(ix);
            }
            ix = if forward {
                (ix + 1) % len
            } else {
                (ix + len - 1) % len
            };
        }
        None
    }

    /// Applies a keystroke to the search query and refilters. Returns false for keystrokes that
    /// aren't search input, which then go through normal key dispatch.
    fn handle_search_keystroke(&mut self, keystroke: &Keystroke) -> bool {
        let modifiers = &keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return false;
        }
        if keystroke.key == "backspace" {
            if self.query.pop().is_none() {
                return false;
            }
        } else {
            match &keystroke.key_char {
                Some(text)
                    if !text.is_empty()
                        && !text.chars().any(char::is_control)
                        // Leave a leading space to key bindings such as a space leader.
                        && !(self.query.is_empty() && text.trim().is_empty()) =>
                {
                    self.query.push_str(text);
                }
                _ => return false,
            }
        }
        self.filter();
        true
    }

    /// Rebuilds the rows from the search query. Matching runs synchronously: the menu holds
    /// a handful of rows, and filtering on the keystroke avoids a frame of stale rows.
    fn filter(&mut self) {
        let mut matches = fuzzy_nucleo::match_strings(
            &self.candidates,
            &self.query,
            fuzzy_nucleo::Case::Ignore,
            fuzzy_nucleo::LengthPenalty::On,
            self.candidates.len(),
        );
        // Best matches first within each group; headers only when there are several groups.
        matches.sort_by_key(|mat| self.targets[mat.candidate_id].1);
        let show_headers = matches
            .iter()
            .map(|mat| self.targets[mat.candidate_id].1)
            .dedup()
            .nth(1)
            .is_some();

        let mut rows = Vec::with_capacity(matches.len());
        let mut last_group = None;
        for mat in matches {
            let (target, group) = self.targets[mat.candidate_id];
            if show_headers && last_group != Some(group) {
                rows.push(CodeActionsMenuRow::Header(group));
                last_group = Some(group);
            }
            rows.push(CodeActionsMenuRow::Item {
                target,
                label: mat.string,
                positions: mat.positions,
            });
        }
        self.rows = rows.into();
    }

    /// The search query line, or a note when nothing matches it.
    pub(crate) fn render_status(&self) -> Option<AnyElement> {
        if self.rows.is_empty() {
            return Some(
                div()
                    .px_2()
                    .py_0p5()
                    .child(
                        Label::new("No matching actions")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            );
        }
        (!self.query.is_empty()).then(|| {
            h_flex()
                .px_2()
                .py_0p5()
                .gap_1()
                .child(
                    Icon::new(IconName::MagnifyingGlass)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
                .child(Label::new(self.query.clone()).size(LabelSize::Small))
                .into_any_element()
        })
    }
}

impl CodeActionsMenu {
    pub(crate) fn select_row(&mut self, ix: Option<usize>, cx: &mut Context<Editor>) {
        if let Some(ix) = ix {
            self.selected_item = ix;
            self.scroll_handle.scroll_to_item(ix, ScrollStrategy::Top);
            cx.notify();
        }
    }

    /// The rows as text, headers as `# Header`, the selected row prefixed with `> `.
    #[cfg(test)]
    pub(crate) fn row_labels(&self) -> Vec<String> {
        self.rows
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| match row {
                CodeActionsMenuRow::Header(group) => format!("# {}", group.label()),
                CodeActionsMenuRow::Item { label, .. } if ix == self.selected_item => {
                    format!("> {label}")
                }
                CodeActionsMenuRow::Item { label, .. } => label.to_string(),
            })
            .collect()
    }
}

impl Editor {
    /// The editor commands to list in a code actions menu: only when it is opened at the cursor
    /// in a buffer with a language.
    pub(crate) fn code_actions_menu_commands(
        &self,
        buffer: &Entity<Buffer>,
        deployed_from: &Option<CodeActionSource>,
        cx: &App,
    ) -> Vec<CodeActionsMenuCommand> {
        if deployed_from.is_none() && self.project.is_some() && buffer.read(cx).language().is_some()
        {
            CodeActionsMenuCommand::defaults()
        } else {
            Vec::new()
        }
    }

    /// Searches the code actions menu while it is open. The returned interceptor runs before
    /// key bindings, so printable keys and backspace edit the search query instead of reaching
    /// the editor or vim; it is dropped together with the menu.
    pub(crate) fn code_actions_menu_search(&self, cx: &mut Context<Self>) -> Subscription {
        let editor = cx.weak_entity();
        cx.intercept_keystrokes(move |event, window, cx| {
            editor
                .update(cx, |editor, cx| {
                    editor.search_code_actions_menu(&event.keystroke, window, cx)
                })
                .ok();
        })
    }

    fn search_code_actions_menu(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        let mut context_menu = self.context_menu.borrow_mut();
        let Some(CodeContextMenu::CodeActions(menu)) = context_menu.as_mut() else {
            return;
        };
        if !menu.rows.handle_search_keystroke(keystroke) {
            return;
        }
        let first = menu.rows.first(menu.scroll_handle.y_flipped());
        menu.select_row(first, cx);
        cx.stop_propagation();
        cx.notify();
    }
}
