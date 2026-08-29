//! Searchable thread picker for Steer panels.
//!
//! Every Steer panel (media, kanban, swarm) embeds one `ThreadPicker` — an
//! "Open Thread" button that deploys a popover listing the threads in the
//! thread database (`ThreadStore`), fuzzy-filtered by a search box, mirroring
//! the sidebar's thread history. Selecting a thread invokes the panel's
//! select callback, which resumes it in the panel's Steer surface via
//! [`crate::open_steer_thread`].
//!
//! The picker is a `Picker` delegate (the same infrastructure as the command
//! palette and profile selector), so search, keyboard navigation, and
//! dismissal behavior come for free.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agent::DbThreadMetadata;
use fuzzy::{StringMatchCandidate, match_strings};
use gpui::{
    App, Context, DismissEvent, Entity, FocusHandle, Focusable, SharedString, Task, Window,
};
use picker::{Picker, PickerDelegate, popover_menu::PickerPopoverMenu};
use ui::{Button, Icon, IconName, IconSize, PopoverMenuHandle, Tooltip, prelude::*};

/// The type of the panel-supplied select callback: receives the chosen
/// thread's session id and opens it in the panel's Steer surface.
pub type ThreadSelectHandler =
    Rc<dyn Fn(agent_client_protocol::schema::v1::SessionId, &mut Window, &mut App)>;

/// Sort threads most-recently-updated first, so the picker mirrors the
/// sidebar's recency ordering regardless of store iteration order.
fn sorted_by_recency(threads: Vec<DbThreadMetadata>) -> Vec<DbThreadMetadata> {
    let mut threads = threads;
    threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    threads
}

/// The "Open Thread" button + popover listing database threads.
pub struct ThreadPicker {
    on_select: ThreadSelectHandler,
    picker: Option<Entity<Picker<ThreadPickerDelegate>>>,
    picker_handle: PopoverMenuHandle<Picker<ThreadPickerDelegate>>,
    focus_handle: FocusHandle,
}

impl ThreadPicker {
    /// The picker invokes `on_select` when the operator picks a thread; the
    /// panel resumes it via [`crate::open_steer_thread`].
    pub fn new(on_select: ThreadSelectHandler, cx: &App) -> Self {
        Self {
            on_select,
            picker: None,
            picker_handle: PopoverMenuHandle::default(),
            focus_handle: cx.focus_handle(),
        }
    }

    fn ensure_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Picker<ThreadPickerDelegate>> {
        if self.picker.is_none() {
            let delegate = ThreadPickerDelegate::new(
                self.on_select.clone(),
                cx.foreground_executor().clone(),
                cx.background_executor().clone(),
                cx,
            );
            self.picker = Some(cx.new(|cx| {
                Picker::list(delegate, window, cx)
                    .show_scrollbar(true)
                    .initial_width(rems(28.))
            }));
        }
        self.picker.as_ref().unwrap().clone()
    }
}

impl Focusable for ThreadPicker {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ThreadPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picker = self.ensure_picker(window, cx);

        let icon = if self.picker_handle.is_deployed() {
            IconName::ChevronUp
        } else {
            IconName::ChevronDown
        };

        let trigger_button = Button::new("open-thread", "Open Thread")
            .label_size(LabelSize::Small)
            .color(Color::Muted)
            .start_icon(Icon::new(IconName::Clock).size(IconSize::XSmall))
            .end_icon(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted));

        let tooltip = Box::new(Tooltip::text("Open a previous thread in this panel"));

        PickerPopoverMenu::new(
            picker,
            trigger_button,
            tooltip,
            gpui::Anchor::BottomLeft,
            cx,
        )
        .with_handle(self.picker_handle.clone())
        .render(window, cx)
    }
}

/// The `Picker` delegate listing database threads, fuzzy-matched on title.
struct ThreadPickerDelegate {
    threads: Vec<DbThreadMetadata>,
    string_candidates: Arc<Vec<StringMatchCandidate>>,
    matches: Vec<usize>,
    selected_index: usize,
    query: String,
    on_select: ThreadSelectHandler,
    foreground: gpui::ForegroundExecutor,
    background: gpui::BackgroundExecutor,
}

impl ThreadPickerDelegate {
    fn new(
        on_select: ThreadSelectHandler,
        foreground: gpui::ForegroundExecutor,
        background: gpui::BackgroundExecutor,
        cx: &App,
    ) -> Self {
        let threads =
            sorted_by_recency(agent::ThreadStore::global(cx).read(cx).entries().collect());
        let string_candidates = Arc::new(Self::string_candidates(&threads));
        Self {
            threads,
            string_candidates,
            matches: Vec::new(),
            selected_index: 0,
            query: String::new(),
            on_select,
            foreground,
            background,
        }
    }

    fn string_candidates(threads: &[DbThreadMetadata]) -> Vec<StringMatchCandidate> {
        threads
            .iter()
            .enumerate()
            .map(|(index, thread)| StringMatchCandidate::new(index, thread.title.as_ref()))
            .collect()
    }

    fn selected_thread(&self, ix: usize) -> Option<&DbThreadMetadata> {
        self.matches
            .get(ix)
            .and_then(|candidate| self.threads.get(*candidate))
    }
}

impl PickerDelegate for ThreadPickerDelegate {
    type ListItem = AnyElement;

    fn name() -> &'static str {
        "steer thread picker"
    }

    fn placeholder_text(&self, _: &mut Window, _: &mut App) -> Arc<str> {
        "Search threads…".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        let text = if self.threads.is_empty() {
            "No saved threads.".into()
        } else {
            "No threads match your search.".into()
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
        if query.is_empty() {
            self.query.clear();
            self.matches = (0..self.threads.len()).collect();
            self.selected_index = 0;
            cx.notify();
            return Task::ready(());
        }

        self.query = query.clone();
        // The thread list is small (hundreds at most); match synchronously on
        // the foreground executor so the list updates without a flash — the
        // same approach as the profile selector's `search_blocking`.
        let matches = self.foreground.block_on(match_strings(
            self.string_candidates.as_ref(),
            &query,
            false,
            true,
            100,
            &AtomicBool::new(false),
            self.background.clone(),
        ));

        self.matches = matches.into_iter().map(|mat| mat.candidate_id).collect();
        self.selected_index = 0;
        cx.notify();
        Task::ready(())
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(thread) = self.selected_thread(self.selected_index) {
            let session_id = thread.id.clone();
            (self.on_select)(session_id, window, cx);
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
        let thread = self.selected_thread(ix)?;
        let title = thread.title.clone();
        let timestamp = agent_ui::threads_archive_view::format_history_entry_timestamp(
            thread.created_at.unwrap_or(thread.updated_at),
        );
        let row = h_flex()
            .id(("thread-picker-row", ix))
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .when(selected, |this| this.bg(cx.theme().colors().element_active))
            .child(Label::new(title).size(LabelSize::Small).truncate())
            .child(
                Label::new(timestamp)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            );
        Some(row.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, title: &str, updated_minutes_ago: i64) -> DbThreadMetadata {
        DbThreadMetadata {
            id: agent_client_protocol::schema::v1::SessionId::new(id),
            parent_session_id: None,
            title: title.into(),
            updated_at: chrono::Utc::now() - chrono::Duration::minutes(updated_minutes_ago),
            created_at: None,
            folder_paths: util::path_list::PathList::default(),
        }
    }

    #[test]
    fn sorts_threads_most_recent_first() {
        let threads = vec![
            thread("old", "Old thread", 120),
            thread("new", "New thread", 5),
            thread("middle", "Middle thread", 60),
        ];
        let sorted = sorted_by_recency(threads);
        assert_eq!(
            sorted
                .iter()
                .map(|thread| thread.id.0.to_string())
                .collect::<Vec<_>>(),
            vec!["new", "middle", "old"]
        );
    }
}
