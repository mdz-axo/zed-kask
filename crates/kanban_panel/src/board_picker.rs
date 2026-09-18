//! Searchable board picker for the kanban panel — the open-by-name control.
//!
//! Reference model R2/R3 (`kask/docs/research/kanban-board-reference-models.md`
//! §6.1, §6.2): boards are addressed by name, and the board list is the
//! navigation root. This is the same PopoverMenu+Picker shape as
//! hkask-steer's `ThreadPicker`, scoped to the panel's fetched board list
//! instead of the thread store. Selecting a board invokes the panel's
//! select callback, which opens it.
//!
//! Duplicate names are allowed (operator decision, §8.2) — rows whose name
//! is shared with another board in the list carry an id-suffix sub-label so
//! the operator can still tell them apart.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use fuzzy::{StringMatchCandidate, match_strings};
use gpui::{App, Context, DismissEvent, SharedString, Task, Window};
use picker::{Picker, PickerDelegate};
use ui::prelude::*;

/// The type of the panel-supplied select callback: receives the chosen
/// board's id and opens it in the panel.
pub(crate) type BoardSelectHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// One board row in the picker: the name the operator addresses boards by,
/// plus the id used for dispatch and (when duplicated) disambiguation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PickerBoard {
    pub board_id: String,
    pub name: String,
    /// Whether another board in the same list shares this name.
    pub duplicate_name: bool,
}

/// Flags which names are shared by more than one board, so rows carrying a
/// duplicated name can show a disambiguating suffix.
pub(crate) fn duplicate_name_flags<'a, I>(names: I) -> Vec<bool>
where
    I: IntoIterator<Item = &'a str>,
{
    let names: Vec<&str> = names.into_iter().collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for name in &names {
        *counts.entry(*name).or_default() += 1;
    }
    names.iter().map(|name| counts[name] > 1).collect()
}

/// Build the picker's rows from (id, name) pairs, marking duplicates.
pub(crate) fn rows_from_names<I>(boards: I) -> Vec<PickerBoard>
where
    I: IntoIterator<Item = (String, String)>,
{
    let boards: Vec<(String, String)> = boards.into_iter().collect();
    let flags = duplicate_name_flags(boards.iter().map(|(_, name)| name.as_str()));
    boards
        .into_iter()
        .zip(flags)
        .map(|((board_id, name), duplicate_name)| PickerBoard {
            board_id,
            name,
            duplicate_name,
        })
        .collect()
}

/// The id-suffix sub-label shown for a board whose name is duplicated in
/// the list (operator decision §8.2: no name uniqueness; the picker
/// disambiguates).
pub(crate) fn disambiguation_label(board: &PickerBoard) -> Option<SharedString> {
    if !board.duplicate_name {
        return None;
    }
    Some(board.board_id.chars().take(8).collect::<String>().into())
}

/// The `Picker` delegate listing the panel's boards, fuzzy-matched on name.
pub(crate) struct BoardPickerDelegate {
    boards: Vec<PickerBoard>,
    string_candidates: Arc<Vec<StringMatchCandidate>>,
    matches: Vec<usize>,
    selected_index: usize,
    query: String,
    on_select: BoardSelectHandler,
    foreground: gpui::ForegroundExecutor,
    background: gpui::BackgroundExecutor,
}

impl BoardPickerDelegate {
    pub(crate) fn new(
        boards: Vec<PickerBoard>,
        on_select: BoardSelectHandler,
        foreground: gpui::ForegroundExecutor,
        background: gpui::BackgroundExecutor,
    ) -> Self {
        let string_candidates = Arc::new(
            boards
                .iter()
                .enumerate()
                .map(|(ix, board)| StringMatchCandidate::new(ix, board.name.as_ref()))
                .collect(),
        );
        Self {
            boards,
            string_candidates,
            matches: (0..boards.len()).collect(),
            selected_index: 0,
            query: String::new(),
            on_select,
            foreground,
            background,
        }
    }

    /// Replace the board list (the panel refreshes the rows on every
    /// board-list read) and re-run the current query against it.
    pub(crate) fn set_boards(&mut self, boards: Vec<PickerBoard>, cx: &mut Context<Picker<Self>>) {
        self.boards = boards;
        self.string_candidates = Arc::new(
            self.boards
                .iter()
                .enumerate()
                .map(|(ix, board)| StringMatchCandidate::new(ix, board.name.as_ref()))
                .collect(),
        );
        self.rematch();
        cx.notify();
    }

    /// Recompute `matches` for the current query. An empty query matches
    /// every board — the popover opens as the full board list (reference
    /// model R3), with search narrowing it.
    fn rematch(&mut self) {
        if self.query.is_empty() {
            self.matches = (0..self.boards.len()).collect();
            self.selected_index = 0;
            return;
        }
        // The board list is small; match synchronously so the list updates
        // without a flash — the same approach as ThreadPicker.
        let matches = self.foreground.block_on(match_strings(
            self.string_candidates.as_ref(),
            &self.query,
            false,
            true,
            100,
            &AtomicBool::new(false),
            self.background.clone(),
        ));
        self.matches = matches.into_iter().map(|mat| mat.candidate_id).collect();
        self.selected_index = 0;
    }

    fn selected_board(&self, ix: usize) -> Option<&PickerBoard> {
        self.matches
            .get(ix)
            .and_then(|candidate| self.boards.get(*candidate))
    }
}

impl PickerDelegate for BoardPickerDelegate {
    type ListItem = AnyElement;

    fn name() -> &'static str {
        "board picker"
    }

    fn placeholder_text(&self, _: &mut Window, _: &mut App) -> Arc<str> {
        "Search boards…".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        let text = if self.boards.is_empty() {
            "No boards yet.".into()
        } else {
            "No boards match your search.".into()
        };
        Some(text)
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.selected_index = ix.min(self.matches.len().saturating_sub(1));
        cx.notify();
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        self.query = query;
        self.rematch();
        cx.notify();
        Task::ready(())
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(board) = self.selected_board(self.selected_index) {
            let board_id = board.board_id.clone();
            (self.on_select)(board_id, window, cx);
        }
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let board = self.selected_board(ix)?;
        let row = h_flex()
            .id(("board-picker-row", ix))
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .when(selected, |this| this.bg(cx.theme().colors().element_active))
            .child(
                Label::new(board.name.clone())
                    .size(LabelSize::Small)
                    .truncate(),
            )
            .when_some(disambiguation_label(board), |this, suffix| {
                this.child(
                    Label::new(suffix)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            });
        Some(row.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Operator decision §8.2: duplicate names are allowed; the picker
    /// disambiguates them instead of the create path blocking them.
    #[test]
    fn duplicate_name_flags_mark_only_repeated_names() {
        let flags = duplicate_name_flags(["Alpha", "Beta", "Alpha", "Gamma"]);
        assert_eq!(flags, [true, false, true, false]);
        assert_eq!(duplicate_name_flags(["Only", "Child"]), [false, false]);
    }

    /// Rows carry the duplicate flag; duplicated rows show an id-suffix
    /// sub-label, unique rows show none.
    #[test]
    fn rows_disambiguate_duplicate_names_with_an_id_suffix() {
        let rows = rows_from_names([
            ("b19c4f2a-0000".to_string(), "Alpha".to_string()),
            ("c27d1e3b-0000".to_string(), "Alpha".to_string()),
            ("d35e9c4f-0000".to_string(), "Beta".to_string()),
        ]);
        assert!(
            rows[0].duplicate_name,
            "both Alpha rows are duplicates of each other"
        );
        assert_eq!(
            disambiguation_label(&rows[0]),
            Some("b19c4f2a".into()),
            "a duplicated name shows its id prefix so the operator can tell the two apart"
        );
        assert_eq!(disambiguation_label(&rows[2]), None);
    }
}
