//! Self-contained GPUI view for the collapsible Nostr compose drawer and
//! per-relay publish receipt banner.
//!
//! Extracted from `nostr_viewer.rs` (Experiment 3d): owns the 5 composer fields
//! (`composer`, `composer_editor`, `composer_media_editor`,
//! `_composer_subscription`, `_publish_task`) and emits `NotePublished` when at
//! least one relay accepts a note or reply.

use gpui::{
    AnyElement, Context, Entity, EventEmitter, Focusable, IntoElement, Render, Task, Window,
};
use hkask_types::nostr_wire::{MAX_CONTENT_CHARS, MAX_MEDIA_ATTACHMENTS, NOSTR_CLIENT_SERVER};
use kask_bridge::KaskSettings;
use serde_json::json;
use settings::Settings as _;
use ui::{Label, LabelSize, prelude::*};
use util::ResultExt as _;

use crate::nostr_composer::{
    CHAR_WARNING_THRESHOLD, ComposerState, MAX_ARTICLE_CHARS, PUBLISH_ARTICLE_TOOL,
    PUBLISH_REPLY_TOOL, PUBLISH_TOOL, PublishOutcome, QuoteContext, ReplyContext,
    slugify_article_title,
};
use crate::nostr_event_model::host_of;
use crate::nostr_viewer::{invoke, short_hex};

/// Emitted when a note or reply is accepted by at least one relay.
pub(crate) struct NotePublished;

/// Self-contained GPUI view for the collapsible compose drawer + publish receipt.
pub(crate) struct NostrComposerView {
    composer: ComposerState,
    article_mode: bool,
    article_meta_expanded: bool,
    composer_editor: Option<Entity<editor::Editor>>,
    composer_media_editor: Option<Entity<editor::Editor>>,
    article_title_editor: Option<Entity<editor::Editor>>,
    article_slug_editor: Option<Entity<editor::Editor>>,
    article_summary_editor: Option<Entity<editor::Editor>>,
    article_hashtags_editor: Option<Entity<editor::Editor>>,
    _composer_subscription: Option<gpui::Subscription>,
    _publish_task: Option<Task<()>>,
}

impl EventEmitter<NotePublished> for NostrComposerView {}

impl NostrComposerView {
    pub(crate) fn new() -> Self {
        Self {
            composer: ComposerState::new(),
            article_mode: false,
            article_meta_expanded: false,
            composer_editor: None,
            composer_media_editor: None,
            article_title_editor: None,
            article_slug_editor: None,
            article_summary_editor: None,
            article_hashtags_editor: None,
            _composer_subscription: None,
            _publish_task: None,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.composer.is_open()
    }

    pub(crate) fn has_draft(&self) -> bool {
        self.composer.has_draft()
    }

    fn ensure_article_editors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<editor::Editor>, Entity<editor::Editor>) {
        let title_ed = if let Some(existing) = &self.article_title_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::single_line(window, cx);
                e.set_placeholder_text("Article title (NIP-23 kind:30023)…", window, cx);
                e
            });
            self.article_title_editor = Some(ed.clone());
            ed
        };
        let slug_ed = if let Some(existing) = &self.article_slug_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::single_line(window, cx);
                e.set_placeholder_text("Optional slug (auto-generated from title)", window, cx);
                e
            });
            self.article_slug_editor = Some(ed.clone());
            ed
        };
        (title_ed, slug_ed)
    }

    fn ensure_article_meta_editors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<editor::Editor>, Entity<editor::Editor>) {
        let summary_ed = if let Some(existing) = &self.article_summary_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::single_line(window, cx);
                e.set_placeholder_text("Optional summary (<=500 chars)…", window, cx);
                e
            });
            self.article_summary_editor = Some(ed.clone());
            ed
        };
        let hashtags_ed = if let Some(existing) = &self.article_hashtags_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::single_line(window, cx);
                e.set_placeholder_text("Optional hashtags (comma-separated, max 16)…", window, cx);
                e
            });
            self.article_hashtags_editor = Some(ed.clone());
            ed
        };
        (summary_ed, hashtags_ed)
    }

    fn browse_media_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_publishing() || self.composer.media_paths.len() >= MAX_MEDIA_ATTACHMENTS
        {
            return;
        }
        let paths_receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach media to Nostr note".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths_receiver.await else {
                return;
            };
            this.update(cx, |this, cx| {
                for path in paths {
                    if this.composer.media_paths.len() >= MAX_MEDIA_ATTACHMENTS {
                        break;
                    }
                    if let Some(path_str) = path.to_str()
                        && let Err(err) = this.composer.add_media_path(path_str.to_string())
                    {
                        this.composer.error = Some(err);
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Ensure the multi-line composer `Editor` exists and syncs edits into `ComposerState`.
    fn ensure_composer_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<editor::Editor> {
        if let Some(existing) = &self.composer_editor {
            return existing.clone();
        }
        let initial_draft = self.composer.draft.clone();
        let editor = cx.new(|cx| {
            let mut ed = editor::Editor::multi_line(window, cx);
            ed.disable_scrollbars_and_minimap(window, cx);
            ed.set_show_gutter(false, cx);
            ed.set_show_line_numbers(false, cx);
            ed.set_show_git_diff_gutter(false, cx);
            ed.set_show_code_actions(false, cx);
            ed.set_show_runnables(false, cx);
            ed.set_show_bookmarks(false, cx);
            ed.set_show_breakpoints(false, cx);
            ed.set_show_wrap_guides(false, cx);
            ed.set_placeholder_text("Write a Nostr note…", window, cx);
            if !initial_draft.is_empty() {
                ed.set_text(initial_draft, window, cx);
            }
            ed
        });
        let sub = cx.subscribe(&editor, |this, ed, event: &editor::EditorEvent, cx| {
            if matches!(event, editor::EditorEvent::BufferEdited) {
                let text = ed.read(cx).text(cx);
                this.composer.set_draft(text);
                cx.notify();
            }
        });
        self.composer_editor = Some(editor.clone());
        self._composer_subscription = Some(sub);
        editor
    }

    /// Open and focus the collapsible compose drawer (`nostr_panel::Compose`).
    pub(crate) fn open_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.open_compose();
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            if ed.text(cx) != self.composer.draft {
                ed.set_text(self.composer.draft.clone(), window, cx);
            }
            window.focus(&ed.focus_handle(cx), cx);
        });
        cx.notify();
    }

    fn ensure_composer_media_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<editor::Editor> {
        if let Some(existing) = &self.composer_media_editor {
            return existing.clone();
        }
        let editor = cx.new(|cx| {
            let mut ed = editor::Editor::single_line(window, cx);
            ed.set_placeholder_text("Local media file path to attach (Blossom)…", window, cx);
            ed
        });
        self.composer_media_editor = Some(editor.clone());
        editor
    }

    /// Toggle the collapsible compose drawer; collapsing always preserves the draft.
    pub(crate) fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.toggle();
        if self.composer.is_open() {
            let editor = self.ensure_composer_editor(window, cx);
            editor.update(cx, |ed, cx| {
                if ed.text(cx) != self.composer.draft {
                    ed.set_text(self.composer.draft.clone(), window, cx);
                }
                window.focus(&ed.focus_handle(cx), cx);
            });
        }
        cx.notify();
    }

    fn collapse_compose(&mut self, cx: &mut Context<Self>) {
        if self.composer.is_open() && !self.composer.is_publishing() {
            self.composer.toggle();
            cx.notify();
        }
    }

    fn clear_article_editors(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ed) = &self.article_title_editor {
            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
        if let Some(ed) = &self.article_slug_editor {
            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
        if let Some(ed) = &self.article_summary_editor {
            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
        if let Some(ed) = &self.article_hashtags_editor {
            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
    }

    fn discard_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_publishing() {
            return;
        }
        self.composer.discard();
        if let Some(editor) = &self.composer_editor {
            editor.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
        self.clear_article_editors(window, cx);
        cx.notify();
    }

    /// Open the compose drawer pre-bound to a NIP-10 / NIP-22 reply target.
    pub(crate) fn open_reply(
        &mut self,
        target: ReplyContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.open_reply(target);
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            if ed.text(cx) != self.composer.draft {
                ed.set_text(self.composer.draft.clone(), window, cx);
            }
            window.focus(&ed.focus_handle(cx), cx);
        });
        cx.notify();
    }

    /// Open the compose drawer pre-bound to a NIP-10 quote target (`q` tag).
    pub(crate) fn open_quote(
        &mut self,
        target: QuoteContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.open_quote(target);
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            if ed.text(cx) != self.composer.draft {
                ed.set_text(self.composer.draft.clone(), window, cx);
            }
            window.focus(&ed.focus_handle(cx), cx);
        });
        cx.notify();
    }

    fn submit_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_publishing() {
            return;
        }
        if let Some(editor) = &self.composer_editor {
            let current_text = editor.read(cx).text(cx);
            self.composer.set_draft(current_text);
        }
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let is_article_mode = self.article_mode
            && self.composer.reply_target.is_none()
            && self.composer.quote_target.is_none();
        let max_chars = if is_article_mode {
            MAX_ARTICLE_CHARS
        } else {
            MAX_CONTENT_CHARS
        };
        let payload = match self.composer.begin_publish_with_limit(now_secs, max_chars) {
            Ok(payload) => payload,
            Err(_) => {
                cx.notify();
                return;
            }
        };
        let reply_target = self.composer.reply_target.clone();
        let quote_target = self.composer.quote_target.clone();
        let is_article = self.article_mode && reply_target.is_none() && quote_target.is_none();
        let (article_title, article_slug) = if is_article {
            let title = self
                .article_title_editor
                .as_ref()
                .map(|ed| ed.read(cx).text(cx).trim().to_string())
                .unwrap_or_default();
            let raw_slug = self
                .article_slug_editor
                .as_ref()
                .map(|ed| ed.read(cx).text(cx).trim().to_string())
                .unwrap_or_default();
            let slug = if raw_slug.is_empty() {
                slugify_article_title(&title)
            } else {
                raw_slug
            };
            if title.is_empty() || slug.is_empty() {
                self.composer.finish_publish_err(
                    "Enter an article title (and ASCII slug) before publishing.".to_string(),
                );
                cx.notify();
                return;
            }
            (title, slug)
        } else {
            (String::new(), String::new())
        };
        cx.notify();

        let (tool, args) = if is_article {
            let summary = self
                .article_summary_editor
                .as_ref()
                .map(|ed| ed.read(cx).text(cx).trim().to_string())
                .filter(|s| !s.is_empty());
            let hashtags: Vec<String> = self
                .article_hashtags_editor
                .as_ref()
                .map(|ed| {
                    let mut out = Vec::new();
                    for raw in ed.read(cx).text(cx).split([',', ' ']) {
                        let clean = raw.trim().trim_start_matches('#').to_string();
                        if !clean.is_empty() && !out.contains(&clean) {
                            out.push(clean);
                            if out.len() >= 16 {
                                break;
                            }
                        }
                    }
                    out
                })
                .unwrap_or_default();
            let mut article_args = json!({
                "slug": article_slug,
                "title": article_title,
                "content": payload.content,
            });
            if let Some(summary) = summary {
                article_args["summary"] = json!(summary);
            }
            if !hashtags.is_empty() {
                article_args["hashtags"] = json!(hashtags);
            }
            (PUBLISH_ARTICLE_TOOL, article_args)
        } else {
            match reply_target {
                Some(target) => (
                    PUBLISH_REPLY_TOOL,
                    json!({
                        "content": payload.content,
                        "root_event_id": target.root_event_id,
                        "reply_to_event_id": target.reply_to_event_id,
                        "target_pubkey": target.target_pubkey,
                        "root_kind": target.root_kind,
                        "created_at": payload.created_at,
                    }),
                ),
                None => {
                    let mut note_args = json!({
                        "content": payload.content,
                        "created_at": payload.created_at,
                    });
                    if !payload.media_paths.is_empty() {
                        note_args["media_paths"] = json!(payload.media_paths);
                    }
                    if let Some(quote) = quote_target {
                        note_args["quote_event_id"] = json!(quote.event_id);
                        note_args["quote_pubkey"] = json!(quote.target_pubkey);
                    }
                    (PUBLISH_TOOL, note_args)
                }
            }
        };
        let task = match invoke(NOSTR_CLIENT_SERVER, tool, args) {
            Ok(task) => task,
            Err(err) => {
                self.composer.finish_publish_err(err);
                cx.notify();
                return;
            }
        };

        let editor_handle = self.composer_editor.clone();
        let title_handle = self.article_title_editor.clone();
        let slug_handle = self.article_slug_editor.clone();
        let summary_handle = self.article_summary_editor.clone();
        let hashtags_handle = self.article_hashtags_editor.clone();
        let window_handle = window.window_handle();
        self._publish_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(raw_json) => {
                        let any_accepted = this.composer.finish_publish_ok(&raw_json);
                        if any_accepted {
                            window_handle
                                .update(cx, |_, window, cx| {
                                    if let Some(ed) = &editor_handle {
                                        ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                    }
                                    if is_article {
                                        if let Some(ed) = &title_handle {
                                            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                        }
                                        if let Some(ed) = &slug_handle {
                                            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                        }
                                        if let Some(ed) = &summary_handle {
                                            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                        }
                                        if let Some(ed) = &hashtags_handle {
                                            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                        }
                                    }
                                })
                                .log_err();
                            cx.emit(NotePublished);
                        }
                    }
                    Err(err) => {
                        this.composer.finish_publish_err(err.to_string());
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn retry_last_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.composer.restore_last_for_retry() {
            return;
        }
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            ed.set_text(self.composer.draft.clone(), window, cx);
        });
        self.submit_compose(window, cx);
    }

    fn render_publish_receipt(&self, outcome: &PublishOutcome, cx: &Context<Self>) -> AnyElement {
        let receipt = outcome.receipt();
        let (summary, tone) = match outcome {
            PublishOutcome::Published(_) => (
                format!(
                    "Published to {}/{} relays{}",
                    receipt.accepted.len(),
                    receipt.accepted.len(),
                    if receipt.read_back_found {
                        " · verified"
                    } else {
                        ""
                    }
                ),
                Color::Success,
            ),
            PublishOutcome::PartialFailure(_) => (
                format!(
                    "Published to {}/{} relays ({} failed)",
                    receipt.accepted.len(),
                    receipt.accepted.len() + receipt.failed.len(),
                    receipt.failed.len()
                ),
                Color::Warning,
            ),
            PublishOutcome::AllFailed(_) => (
                format!("Failed on all {} relays", receipt.failed.len()),
                Color::Error,
            ),
        };

        v_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_1()
            .min_w_0()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .bg(cx.theme().colors().surface_background)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .min_w_0()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(summary)
                                .size(LabelSize::Small)
                                .color(tone)
                                .truncate(),
                        ),
                    )
                    .when(self.composer.can_retry_last(), |row| {
                        row.child(
                            ui::Button::new("nostr-receipt-retry", "Retry")
                                .label_size(LabelSize::XSmall)
                                .tooltip(ui::Tooltip::text(
                                    "Re-submit the preserved note payload to relays",
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.retry_last_compose(window, cx);
                                })),
                        )
                    })
                    .child(
                        ui::IconButton::new("nostr-receipt-dismiss", ui::IconName::Close)
                            .icon_size(ui::IconSize::XSmall)
                            .tooltip(ui::Tooltip::text("Dismiss publish receipt"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composer.dismiss_receipt();
                                cx.notify();
                            })),
                    ),
            )
            .children(receipt.failed.iter().take(3).map(|relay| {
                let detail = match &relay.reason {
                    Some(reason) => {
                        format!("{} ({}): {}", host_of(&relay.address), relay.state, reason)
                    }
                    None => format!("{} ({})", host_of(&relay.address), relay.state),
                };
                Label::new(detail)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .truncate()
            }))
            .into_any_element()
    }

    fn render_composer_drawer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.composer.is_open() {
            return None;
        }
        let editor = self.ensure_composer_editor(window, cx);
        let top_level =
            self.composer.reply_target.is_none() && self.composer.quote_target.is_none();
        let is_article = self.article_mode && top_level;
        let article_editors = if is_article {
            Some(self.ensure_article_editors(window, cx))
        } else {
            None
        };
        let article_meta_editors = if is_article && self.article_meta_expanded {
            Some(self.ensure_article_meta_editors(window, cx))
        } else {
            None
        };
        let article_meta_expanded = self.article_meta_expanded;
        let media_editor = if self.composer.reply_target.is_none() && !is_article {
            Some(self.ensure_composer_media_editor(window, cx))
        } else {
            None
        };
        let chars = self.composer.char_count();
        let max_chars = if is_article {
            MAX_ARTICLE_CHARS
        } else {
            MAX_CONTENT_CHARS
        };
        let warn_threshold = if is_article {
            MAX_ARTICLE_CHARS.saturating_sub(2_000)
        } else {
            CHAR_WARNING_THRESHOLD
        };
        let publishing = self.composer.is_publishing();
        let can_publish = !publishing && self.composer.has_draft() && chars <= max_chars;
        let counter_color = if chars > max_chars {
            Color::Error
        } else if chars >= warn_threshold {
            Color::Warning
        } else {
            Color::Muted
        };
        let (npub_short, relay_count) = {
            let nostr_cfg = &KaskSettings::get_global(cx).nostr;
            let trimmed = nostr_cfg.npub.trim();
            let npub = if trimmed.is_empty() {
                "no identity".to_string()
            } else {
                short_hex(trimmed)
            };
            let relays = 1 + nostr_cfg.seed_relays.len();
            (npub, relays)
        };
        let status_text = if publishing {
            "Awaiting approval / publishing…".to_string()
        } else {
            format!("{chars}/{max_chars} · {npub_short} · {relay_count} relays")
        };
        let media_paths = self.composer.media_paths.clone();
        let error = self.composer.error.clone();
        let reply_banner = self
            .composer
            .reply_target
            .as_ref()
            .map(|target| {
                format!(
                    "Replying to {} ({})",
                    target.target_label,
                    target.nip_label()
                )
            })
            .or_else(|| {
                self.composer
                    .quote_target
                    .as_ref()
                    .map(|q| format!("Quoting {} (NIP-10)", q.target_label))
            });

        Some(
            v_flex()
                .id("nostr-compose-drawer")
                .w_full()
                .px_2()
                .py_2()
                .gap_2()
                .min_w_0()
                .flex_shrink_0()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .bg(cx.theme().colors().elevated_surface_background)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.collapse_compose(cx);
                        cx.stop_propagation();
                    } else if event.keystroke.key == "enter"
                        && (event.keystroke.modifiers.secondary(cx)
                            || event.keystroke.modifiers.control)
                    {
                        this.submit_compose(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .when(top_level, |this| {
                    this.child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .child(
                                ui::Button::new("nostr-compose-mode-note", "Note")
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(!is_article)
                                    .disabled(publishing)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.article_mode = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                ui::Button::new("nostr-compose-mode-article", "Article (NIP-23)")
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(is_article)
                                    .disabled(publishing)
                                    .tooltip(ui::Tooltip::text(
                                        "Compose a long-form Markdown article (NIP-23 kind:30023)",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.article_mode = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .when_some(article_editors, |this, (title_ed, slug_ed)| {
                    this.child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                div()
                                    .w_full()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(cx.theme().colors().border)
                                    .bg(cx.theme().colors().editor_background)
                                    .child(title_ed),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(cx.theme().colors().border)
                                    .bg(cx.theme().colors().editor_background)
                                    .child(slug_ed),
                            )
                            .child(
                                ui::Button::new(
                                    "nostr-article-meta-toggle",
                                    if article_meta_expanded {
                                        "▾ Hide summary & hashtags"
                                    } else {
                                        "▸ Summary & hashtags (optional)"
                                    },
                                )
                                .label_size(LabelSize::XSmall)
                                .disabled(publishing)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.article_meta_expanded = !this.article_meta_expanded;
                                    cx.notify();
                                })),
                            )
                            .when_some(article_meta_editors, |meta, (summary_ed, hashtags_ed)| {
                                meta.child(
                                    div()
                                        .w_full()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(cx.theme().colors().border)
                                        .bg(cx.theme().colors().editor_background)
                                        .child(summary_ed),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(cx.theme().colors().border)
                                        .bg(cx.theme().colors().editor_background)
                                        .child(hashtags_ed),
                                )
                            }),
                    )
                })
                .when_some(reply_banner, |this, banner| {
                    this.child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .min_w_0()
                            .items_center()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Label::new(banner)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Accent)
                                        .truncate(),
                                ),
                            )
                            .child(
                                ui::IconButton::new("nostr-reply-clear", ui::IconName::Close)
                                    .icon_size(ui::IconSize::XSmall)
                                    .disabled(publishing)
                                    .tooltip(ui::Tooltip::text("Switch back to a top-level note"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.composer.clear_reply_target();
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .min_h(px(64.))
                        .max_h(px(120.))
                        .overflow_hidden()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(if error.is_some() {
                            cx.theme().status().error_border
                        } else {
                            cx.theme().colors().border
                        })
                        .bg(cx.theme().colors().editor_background)
                        .child(editor),
                )
                .when_some(media_editor, |drawer, med_ed| {
                    let at_limit = media_paths.len() >= MAX_MEDIA_ATTACHMENTS;
                    drawer
                        .child(
                            h_flex()
                                .w_full()
                                .gap_1()
                                .min_w_0()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .px_1()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(cx.theme().colors().border)
                                        .bg(cx.theme().colors().editor_background)
                                        .child(med_ed.clone()),
                                )
                                .child(
                                    ui::Button::new("nostr-compose-browse", "Browse…")
                                        .label_size(LabelSize::XSmall)
                                        .disabled(publishing || at_limit)
                                        .tooltip(ui::Tooltip::text(format!(
                                            "Pick local media file(s) for Blossom upload (max {MAX_MEDIA_ATTACHMENTS})"
                                        )))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.browse_media_files(window, cx);
                                        })),
                                )
                                .child(
                                    ui::Button::new("nostr-compose-attach", "Attach")
                                        .label_size(LabelSize::XSmall)
                                        .disabled(publishing || at_limit)
                                        .tooltip(ui::Tooltip::text(format!(
                                            "Attach local media file for Blossom upload (max {MAX_MEDIA_ATTACHMENTS})"
                                        )))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            let path = med_ed.read(cx).text(cx);
                                            match this.composer.add_media_path(path) {
                                                Ok(()) => {
                                                    med_ed.update(cx, |ed, cx| {
                                                        ed.set_text("", window, cx);
                                                    });
                                                }
                                                Err(err) => {
                                                    this.composer.error = Some(err);
                                                }
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(!media_paths.is_empty(), |d| {
                            d.child(h_flex().w_full().gap_1().flex_wrap().children(
                                media_paths.iter().enumerate().map(|(m_ix, path)| {
                                    let file_label = std::path::Path::new(path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(path.as_str());
                                    ui::Button::new(
                                        ("nostr-compose-media-rm", m_ix),
                                        format!("📎 {file_label} ×"),
                                    )
                                    .label_size(LabelSize::XSmall)
                                    .disabled(publishing)
                                    .tooltip(ui::Tooltip::text(format!(
                                        "Remove attachment: {path}"
                                    )))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.composer.remove_media_path(m_ix);
                                        cx.notify();
                                    }))
                                }),
                            ))
                        })
                })
                .when_some(error, |this, err| {
                    this.child(
                        Label::new(err)
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    )
                })
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .min_w_0()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Label::new(status_text)
                                    .size(LabelSize::Small)
                                    .color(counter_color)
                                    .truncate(),
                            ),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .min_w(px(32.))
                                .flex()
                                .justify_center()
                                .child(
                                    ui::IconButton::new("nostr-compose-discard", ui::IconName::Trash)
                                        .icon_size(ui::IconSize::Small)
                                        .disabled(publishing)
                                        .tooltip(ui::Tooltip::text("Discard draft"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.discard_compose(window, cx);
                                        })),
                                ),
                        )
                        .child(
                            div().flex_shrink_0().child(
                                ui::Button::new(
                                    "nostr-compose-publish",
                                    if publishing { "Publishing…" } else { "Publish" },
                                )
                                .style(ui::ButtonStyle::Filled)
                                .disabled(!can_publish)
                                .tooltip(ui::Tooltip::text(if is_article {
                                    "Publish NIP-23 kind:30023 article (Ctrl/Cmd+Enter). Always asks editor approval before signing."
                                } else if self.composer.reply_target.is_some() {
                                    "Publish reply/comment (Ctrl/Cmd+Enter). Editor asks approval before signing."
                                } else {
                                    "Publish kind:1 note (Ctrl/Cmd+Enter). Editor asks approval before signing."
                                }))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_compose(window, cx);
                                })),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for NostrComposerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compose_drawer = self.render_composer_drawer(window, cx);
        let publish_receipt = self
            .composer
            .last_outcome
            .as_ref()
            .map(|outcome| self.render_publish_receipt(outcome, cx));
        if compose_drawer.is_none() && publish_receipt.is_none() {
            return div().into_any_element();
        }
        v_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .when_some(compose_drawer, |this, drawer| this.child(drawer))
            .when_some(publish_receipt, |this, receipt| this.child(receipt))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_composer_view_starts_closed_with_no_draft() {
        let view = NostrComposerView::new();
        assert!(!view.is_open());
        assert!(!view.has_draft());
    }
}
