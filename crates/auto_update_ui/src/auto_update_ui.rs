use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_skills::GLOBAL_SKILLS_DIR_DISPLAY;
use auto_update::{AutoUpdateStatus, AutoUpdater, release_notes_url};
use client::zed_urls;
use db::kvp::Dismissable;
use editor::{Editor, MultiBuffer};
use gpui::{
    App, DismissEvent, Empty, Entity, EventEmitter, FocusHandle, Focusable, Subscription, TaskExt,
    Window, actions, prelude::*,
};
use markdown_preview::markdown_preview_view::{MarkdownPreviewMode, MarkdownPreviewView};
use prompt_store::rules_to_skills_migration;
use release_channel::{AppVersion, ReleaseChannel};
use semver::Version;
use serde::Deserialize;
use smol::io::AsyncReadExt;
use ui::{
    AnnouncementToast, CommonAnimationExt, ListBulletItem, ProgressBar, SkillsIllustration,
    Tooltip, prelude::*,
};
use util::{ResultExt as _, maybe};
use workspace::{
    Workspace,
    notifications::{
        Notification, NotificationId, SuppressEvent, dismiss_app_notification,
        show_app_notification, simple_message_notification::MessageNotification,
    },
    workspace_error::{ErrorAction, ErrorSeverity, WorkspaceError},
};
use zed_actions::ShowUpdateNotification;

actions!(
    auto_update,
    [
        /// Opens the release notes for the current version in a new tab.
        ViewReleaseNotesLocally
    ]
);

pub fn init(cx: &mut App) {
    notify_if_app_was_updated(cx);
    cx.observe_new(|workspace: &mut Workspace, _window, cx| {
        workspace.register_action(|workspace, _: &ViewReleaseNotesLocally, window, cx| {
            view_release_notes_locally(workspace, window, cx);
        });

        if matches!(
            ReleaseChannel::global(cx),
            ReleaseChannel::Nightly | ReleaseChannel::Dev
        ) {
            workspace.register_action(|_workspace, _: &ShowUpdateNotification, _window, cx| {
                show_update_notification(cx);
            });
        }
    })
    .detach();

    // zed-kask: dead code — retained from the removed D17 (GitHub update feed)
    // and D19 (update-progress popup) seams. The in-app GitHub feed was replaced
    // by the terminal-based `update-zed-kask.sh` script (D16); `auto_update_ui::init`
    // is never called in zed-kask (enforced by `check-zed-isolation.sh`), so this
    // popup is never wired. Kept to minimize the diff against upstream Zed's
    // `auto_update_ui` crate; the `AutoUpdater` status machine it observes is itself
    // dormant. Do not re-wire without re-introducing D17/D19 in DIVERGENCE.md.
    if let Some(updater) = AutoUpdater::get(cx) {
        cx.observe(&updater, |_updater, cx| {
            manage_update_progress_notification(cx)
        })
        .detach();
        manage_update_progress_notification(cx);
    }
}

#[derive(Deserialize)]
struct ReleaseNotesBody {
    title: String,
    release_notes: String,
}

fn notify_release_notes_failed_to_show(
    workspace: &mut Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let url = release_notes_url(cx);

    struct ReleaseNotesError {
        url: Option<String>,
    }

    impl WorkspaceError for ReleaseNotesError {
        fn primary_message(&self) -> SharedString {
            "Couldn't load release notes".into()
        }
        fn severity(&self) -> ErrorSeverity {
            ErrorSeverity::Error
        }
        fn primary_action(&self) -> ErrorAction {
            self.url
                .clone()
                .map(|url| ErrorAction::link("View in Browser", url))
                .unwrap_or_else(ErrorAction::dismiss)
        }
    }

    workspace.show_error(ReleaseNotesError { url }, cx);
}

fn view_release_notes_locally(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let release_channel = ReleaseChannel::global(cx);

    if matches!(
        release_channel,
        ReleaseChannel::Nightly | ReleaseChannel::Dev
    ) {
        if let Some(url) = release_notes_url(cx) {
            cx.open_url(&url);
        }
        return;
    }

    let version = AppVersion::global(cx).to_string();

    let client = client::Client::global(cx).http_client();
    let url = client.build_url(&format!(
        "/api/release_notes/v2/{}/{}",
        release_channel.dev_name(),
        version
    ));

    let markdown = workspace
        .app_state()
        .languages
        .language_for_name("Markdown");

    cx.spawn_in(window, async move |workspace, cx| {
        let markdown = markdown.await.log_err();
        let response = client.get(&url, Default::default(), true).await;
        let Some(mut response) = response.log_err() else {
            workspace
                .update_in(cx, notify_release_notes_failed_to_show)
                .log_err();
            return;
        };

        let mut body = Vec::new();
        response.body_mut().read_to_end(&mut body).await.ok();

        let body: serde_json::Result<ReleaseNotesBody> = serde_json::from_slice(body.as_slice());

        let res: Option<()> = maybe!(async {
            let body = body.ok()?;
            let project = workspace
                .read_with(cx, |workspace, _| workspace.project().clone())
                .ok()?;
            let (language_registry, buffer) = project.update(cx, |project, cx| {
                (
                    project.languages().clone(),
                    project.create_buffer(markdown, false, cx),
                )
            });
            let buffer = buffer.await.ok()?;
            buffer.update(cx, |buffer, cx| {
                buffer.edit([(0..0, body.release_notes)], None, cx)
            });

            let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx).with_title(body.title));

            let ws_handle = workspace.clone();
            workspace
                .update_in(cx, |workspace, window, cx| {
                    let editor =
                        cx.new(|cx| Editor::for_multibuffer(buffer, Some(project), window, cx));
                    let markdown_preview: Entity<MarkdownPreviewView> = MarkdownPreviewView::new(
                        MarkdownPreviewMode::Default,
                        editor,
                        ws_handle,
                        language_registry,
                        window,
                        cx,
                    );
                    workspace.add_item_to_active_pane(
                        Box::new(markdown_preview),
                        None,
                        true,
                        window,
                        cx,
                    );
                    cx.notify();
                })
                .ok()
        })
        .await;
        if res.is_none() {
            workspace
                .update_in(cx, notify_release_notes_failed_to_show)
                .log_err();
        }
    })
    .detach();
}

#[derive(Clone)]
struct AnnouncementContent {
    heading: SharedString,
    description: SharedString,
    bullet_items: Vec<SharedString>,
    primary_action_label: SharedString,
    secondary_action_label: SharedString,
    primary_action_url: Option<SharedString>,
    primary_action_callback: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    secondary_action_url: Option<SharedString>,
    on_dismiss: Option<Arc<dyn Fn(&mut App) + Send + Sync>>,
}

struct SkillsAnnouncement;

impl Dismissable for SkillsAnnouncement {
    const KEY: &'static str = "skills_announcement_dismissed";
}

fn announcement_for_version(version: &Version, cx: &App) -> Option<AnnouncementContent> {
    let version_with_skills = match ReleaseChannel::global(cx) {
        ReleaseChannel::Stable => Version::new(1, 4, 0),
        ReleaseChannel::Dev | ReleaseChannel::Nightly | ReleaseChannel::Preview => {
            Version::new(1, 4, 0)
        }
    };

    if *version >= version_with_skills && !SkillsAnnouncement::dismissed(cx) {
        // Only mention the Rules → Skills migration if the user actually
        // had Rules that got migrated. New users (and existing users who
        // never created a Rule) would otherwise be confused by a bullet
        // referring to "your rules" that don't exist.
        let migrated_anything =
            rules_to_skills_migration::migration_result().is_some_and(|result| !result.is_empty());

        let mut bullet_items: Vec<SharedString> = Vec::with_capacity(3);
        bullet_items
            .push(format!("Skills live in {GLOBAL_SKILLS_DIR_DISPLAY}/<name>/SKILL.md").into());
        bullet_items.push("Type / to manually invoke a skill".into());
        if migrated_anything {
            bullet_items.push(
                "The Rules Library is making way for skills: your default rules are now in a global AGENTS.md, and your other rules have been converted to skills".into(),
            );
        }

        Some(AnnouncementContent {
            heading: "Introducing Skills Support".into(),
            description: "Extend the agent with focused instructions and domain knowledge.".into(),
            bullet_items,
            primary_action_label: "Try Now".into(),
            secondary_action_label: "Read Documentation".into(),
            primary_action_url: None,
            primary_action_callback: Some(Arc::new(move |window, cx| {
                window.dispatch_action(Box::new(zed_actions::assistant::FocusAgent), cx);
            })),
            on_dismiss: Some(Arc::new(|cx| SkillsAnnouncement::set_dismissed(true, cx))),
            secondary_action_url: Some(zed_urls::skills_docs(cx).into()),
        })
    } else {
        None
    }
}

struct AnnouncementToastNotification {
    focus_handle: FocusHandle,
    content: AnnouncementContent,
}

impl AnnouncementToastNotification {
    fn new(content: AnnouncementContent, cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content,
        }
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        if let Some(on_dismiss) = &self.content.on_dismiss {
            on_dismiss(cx);
        }
    }
}

impl Focusable for AnnouncementToastNotification {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for AnnouncementToastNotification {}
impl EventEmitter<SuppressEvent> for AnnouncementToastNotification {}
impl Notification for AnnouncementToastNotification {}

impl Render for AnnouncementToastNotification {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        AnnouncementToast::new()
            .illustration(SkillsIllustration::new())
            .heading(self.content.heading.clone())
            .description(self.content.description.clone())
            .bullet_items(
                self.content
                    .bullet_items
                    .iter()
                    .map(|item| ListBulletItem::new(item.clone())),
            )
            .primary_action_label(self.content.primary_action_label.clone())
            .secondary_action_label(self.content.secondary_action_label.clone())
            .primary_on_click(cx.listener({
                let url = self.content.primary_action_url.clone();
                let callback = self.content.primary_action_callback.clone();
                move |this, _, window, cx| {
                    telemetry::event!("Skills Announcement Main Click");
                    if let Some(callback) = &callback {
                        callback(window, cx);
                    }
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                    this.dismiss(cx);
                }
            }))
            .secondary_on_click(cx.listener({
                let url = self.content.secondary_action_url.clone();
                move |_, _, _window, cx| {
                    telemetry::event!("Skills Announcement Secondary Click");
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                }
            }))
            .dismiss_on_click(cx.listener(|this, _, _window, cx| {
                telemetry::event!("Skills Announcement Dismiss");
                this.dismiss(cx);
            }))
    }
}

struct UpdateNotification;

fn show_update_notification(cx: &mut App) {
    let Some(updater) = AutoUpdater::get(cx) else {
        return;
    };

    let mut version = updater.read(cx).current_version();
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    let app_name = ReleaseChannel::global(cx).display_name();

    if let Some(content) = announcement_for_version(&version, cx) {
        show_app_notification(
            NotificationId::unique::<UpdateNotification>(),
            cx,
            move |cx| cx.new(|cx| AnnouncementToastNotification::new(content.clone(), cx)),
        );
    } else {
        show_app_notification(
            NotificationId::unique::<UpdateNotification>(),
            cx,
            move |cx| {
                let workspace_handle = cx.entity().downgrade();
                cx.new(|cx| {
                    MessageNotification::new(format!("Updated to {app_name} {}", version), cx)
                        .primary_message("View Release Notes")
                        .primary_on_click(move |window, cx| {
                            if let Some(workspace) = workspace_handle.upgrade() {
                                workspace.update(cx, |workspace, cx| {
                                    crate::view_release_notes_locally(workspace, window, cx);
                                })
                            }
                            cx.emit(DismissEvent);
                        })
                        .show_suppress_button(false)
                })
            },
        );
    }
}

/// Shows a notification across all workspaces if an update was previously automatically installed
/// and this notification had not yet been shown.
pub fn notify_if_app_was_updated(cx: &mut App) {
    let Some(updater) = AutoUpdater::get(cx) else {
        return;
    };

    if let ReleaseChannel::Nightly = ReleaseChannel::global(cx) {
        return;
    }

    let should_show_notification = updater.read(cx).should_show_update_notification(cx);

    cx.spawn(async move |cx| {
        let should_show_notification = should_show_notification.await?;

        if should_show_notification {
            cx.update(|cx| {
                show_update_notification(cx);
                updater.update(cx, |updater, cx| {
                    updater
                        .set_should_show_update_notification(false, cx)
                        .detach_and_log_err(cx);
                });
            });
        }
        anyhow::Ok(())
    })
    .detach();
}

// zed-kask: dead code — retained from the removed D17 (GitHub update feed) and
// D19 (update-progress popup) seams. The title bar already shows a compact
// `UpdateButton` with a circular progress ring; this popup added a larger,
// horizontal `ProgressBar` surface for the single-click `Update Zed-Kask` flow.
// That flow was replaced by the terminal-based `update-zed-kask.sh` script (D16);
// `auto_update_ui::init` is never called in zed-kask (enforced by
// `check-zed-isolation.sh`), so this code is never wired. Kept to minimize the
// diff against upstream Zed's `auto_update_ui` crate.

/// Marker type for the app-global progress notification id.
struct UpdateProgressNotificationId;

/// Tracks whether the progress popup is currently shown so the App-level
/// observer only calls `show_app_notification` / `dismiss_app_notification` on
/// rising / falling edges. Without edge detection, every progress tick (which
/// `cx.notify()`s the `AutoUpdater`) would tear down and rebuild the
/// notification entity.
static PROGRESS_NOTIFICATION_ACTIVE: AtomicBool = AtomicBool::new(false);

struct UpdateProgressNotification {
    focus_handle: FocusHandle,
    updater: Entity<AutoUpdater>,
    _subscription: Subscription,
}

impl UpdateProgressNotification {
    fn new(updater: Entity<AutoUpdater>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        // Re-render in place on every `AutoUpdater::notify` (i.e. every progress
        // tick) so the bar fills smoothly without re-creating the notification.
        let _subscription = cx.observe(&updater, |_this, _updater, cx| cx.notify());
        Self {
            focus_handle,
            updater,
            _subscription,
        }
    }

    fn dismiss_current(&mut self, cx: &mut Context<Self>) {
        let status = self.updater.read(cx).status();
        self.updater
            .update(cx, |updater, cx| updater.dismiss_status(status, cx));
        cx.emit(DismissEvent);
    }
}

impl Focusable for UpdateProgressNotification {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for UpdateProgressNotification {}
impl EventEmitter<SuppressEvent> for UpdateProgressNotification {}
impl Notification for UpdateProgressNotification {}

impl Render for UpdateProgressNotification {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let updater = self.updater.read(cx);
        let status = updater.status();
        let is_manual = updater.update_check_type().is_manual();

        let title: SharedString;
        let body: AnyElement;
        // Close button is only offered for terminal states (Updated / Errored),
        // mirroring the title-bar `UpdateVersion` which only wires `on_dismiss`
        // for those states. Dismissing in-progress work would just re-show on the
        // next tick.
        let show_close: bool;
        let mut actions = h_flex().gap_1();

        match status {
            AutoUpdateStatus::Checking if is_manual => {
                title = "Checking for zed-kask updates".into();
                body = Label::new("Looking up the latest GitHub release…")
                    .color(Color::Muted)
                    .size(LabelSize::Small)
                    .into_any_element();
                show_close = false;
            }
            AutoUpdateStatus::Downloading { version, progress } => {
                title = format!("Downloading zed-kask {version}").into();
                let pct = progress.map(|p| (p.clamp(0.0, 1.0) * 100.0).round() as u32);
                body = v_flex()
                    .gap_1()
                    .child(match progress {
                        Some(p) => ProgressBar::new("update-download", p.clamp(0.0, 1.0), 1.0, cx)
                            .into_any_element(),
                        None => Icon::new(IconName::LoadCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                            .with_rotate_animation(2)
                            .into_any_element(),
                    })
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                Label::new(match progress {
                                    Some(_) => "Downloading…",
                                    None => "Downloading… (size unknown)",
                                })
                                .color(Color::Muted)
                                .size(LabelSize::Small),
                            )
                            .when_some(pct, |el, pct| {
                                el.child(
                                    Label::new(format!("{pct}%"))
                                        .color(Color::Muted)
                                        .size(LabelSize::Small),
                                )
                            }),
                    )
                    .into_any_element();
                show_close = false;
            }
            AutoUpdateStatus::Installing { version } => {
                title = format!("Installing zed-kask {version}").into();
                body = Label::new("Applying the update…")
                    .color(Color::Muted)
                    .size(LabelSize::Small)
                    .into_any_element();
                show_close = false;
            }
            AutoUpdateStatus::Updated { version } => {
                title = format!("zed-kask {version} is ready").into();
                body = Label::new("Restart to apply the update.")
                    .color(Color::Muted)
                    .size(LabelSize::Small)
                    .into_any_element();
                show_close = true;
                actions = actions.child(
                    Button::new(("restart", cx.entity_id()), "Restart")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|_this, _, _, cx| {
                            workspace::reload(cx);
                        })),
                );
            }
            // zed-kask: dead code (removed D19) — positive feedback for a manual check that found no
            // update. Without this the popup flashed "Checking…" then vanished,
            // looking like nothing happened. Auto-dismissed after a few seconds
            // (see `manage_update_progress_notification`) since it's
            // informational, not actionable.
            AutoUpdateStatus::UpToDate { version } => {
                title = "zed-kask is up to date".into();
                body = Label::new(format!("zed-kask {version} is the latest version."))
                    .color(Color::Muted)
                    .size(LabelSize::Small)
                    .into_any_element();
                show_close = true;
            }
            AutoUpdateStatus::Errored { error } => {
                title = "Update failed".into();
                body = Label::new(error.to_string())
                    .color(Color::Muted)
                    .size(LabelSize::Small)
                    .into_any_element();
                show_close = true;
                actions = actions.child(
                    Button::new(("view-logs", cx.entity_id()), "View Logs")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|_this, _, window, cx| {
                            window.dispatch_action(Box::new(workspace::OpenLog), cx);
                        })),
                );
            }
            // Idle or an automatic (background) check: the title bar still
            // shows the compact indicator; the popup should not be visible.
            AutoUpdateStatus::Idle | AutoUpdateStatus::Checking => {
                return Empty.into_any_element();
            }
        }

        let close = show_close.then(|| {
            IconButton::new(("update-progress-close", cx.entity_id()), IconName::Close)
                .icon_size(IconSize::Indicator)
                .tooltip(Tooltip::text("Dismiss"))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_current(cx)))
        });

        let header_actions = h_flex()
            .flex_shrink_0()
            .gap_1()
            .child(actions)
            .children(close);

        v_flex()
            .id(("update-progress-notification", cx.entity_id()))
            .occlude()
            .p_3()
            .gap_2()
            .elevation_3(cx)
            .child(
                h_flex()
                    .gap_4()
                    .justify_between()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(Label::new(title))
                            .child(div().max_w_96().child(body)),
                    )
                    .child(header_actions),
            )
            .into_any_element()
    }
}

/// Show or hide the progress popup based on the current `AutoUpdater` status,
/// but only on rising / falling edges of the active state (see
/// `PROGRESS_NOTIFICATION_ACTIVE`). Gating mirrors the title-bar `UpdateVersion`:
/// `Checking` is surfaced only for manual checks, while `Downloading`,
/// `Installing`, `Updated`, and `Errored` are surfaced for any check type.
/// A dismissed status (set by the popup's close button) keeps the popup hidden.
fn manage_update_progress_notification(cx: &mut App) {
    let Some(updater) = AutoUpdater::get(cx) else {
        return;
    };
    let reader = updater.read(cx);
    let status = reader.status();
    let is_manual = reader.update_check_type().is_manual();
    let should_show = should_show_progress(&status, is_manual, reader.dismissed_status().as_ref());

    let was_active = PROGRESS_NOTIFICATION_ACTIVE.swap(should_show, Ordering::SeqCst);
    if should_show && !was_active {
        let updater_for_notification = updater.clone();
        show_app_notification(
            NotificationId::unique::<UpdateProgressNotificationId>(),
            cx,
            move |cx| {
                let updater = updater_for_notification.clone();
                cx.new(|cx| UpdateProgressNotification::new(updater, cx))
            },
        );

        // zed-kask: dead code (removed D19) — the "up to date" popup is informational, not
        // actionable, so auto-dismiss it after a short delay rather than
        // leaving it for the user to close. The guard re-checks the status
        // after the timer fires so a state change (e.g. a new manual check)
        // isn't dismissed out from under the user.
        if matches!(status, AutoUpdateStatus::UpToDate { .. }) {
            let updater = updater.clone();
            let executor = cx.background_executor().clone();
            cx.spawn(async move |cx| {
                executor.timer(Duration::from_secs(6)).await;
                let _ = cx.update(|cx| {
                    if matches!(updater.read(cx).status(), AutoUpdateStatus::UpToDate { .. }) {
                        updater.update(cx, |updater, cx| {
                            updater.dismiss_status(updater.status(), cx);
                        });
                    }
                });
            })
            .detach();
        }
    } else if !should_show && was_active {
        dismiss_app_notification(
            &NotificationId::unique::<UpdateProgressNotificationId>(),
            cx,
        );
    }
}

/// Pure decision extracted from `manage_update_progress_notification` so the
/// gating contract can be unit-tested without the HTTP-mock update harness.
/// Mirrors the title-bar `UpdateVersion` gating: `Checking` surfaces only for
/// manual checks; `Downloading` / `Installing` / `Updated` / `Errored` surface
/// for any check type; a dismissed status suppresses only the matching state.
fn should_show_progress(
    status: &AutoUpdateStatus,
    is_manual: bool,
    dismissed_status: Option<&AutoUpdateStatus>,
) -> bool {
    let dismissed = dismissed_status == Some(status);
    !dismissed
        && match status {
            AutoUpdateStatus::Idle => false,
            AutoUpdateStatus::Checking if !is_manual => false,
            AutoUpdateStatus::Checking
            | AutoUpdateStatus::Downloading { .. }
            | AutoUpdateStatus::Installing { .. }
            | AutoUpdateStatus::Updated { .. }
            | AutoUpdateStatus::UpToDate { .. }
            | AutoUpdateStatus::Errored { .. } => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn version() -> semver::Version {
        semver::Version::new(1, 0, 0)
    }

    // zed-kask: dead code (removed D19) — pins the popup gating contract: Idle never shows;
    // Checking shows only for manual checks; Downloading / Installing /
    // Updated / Errored show for any check type; a dismissed status suppresses
    // only the matching state.
    #[test]
    fn progress_popup_gating() {
        // Idle never shows, regardless of check type or dismiss.
        assert!(!should_show_progress(&AutoUpdateStatus::Idle, true, None));
        assert!(!should_show_progress(&AutoUpdateStatus::Idle, false, None));

        // Checking surfaces only for a manual (single-click) check.
        assert!(should_show_progress(
            &AutoUpdateStatus::Checking,
            true,
            None
        ));
        assert!(!should_show_progress(
            &AutoUpdateStatus::Checking,
            false,
            None
        ));

        // Downloading / Installing / Updated / Errored surface for any check
        // type (matches the title-bar `UpdateVersion` gating).
        assert!(should_show_progress(
            &AutoUpdateStatus::Downloading {
                version: version(),
                progress: Some(0.5)
            },
            false,
            None
        ));
        assert!(should_show_progress(
            &AutoUpdateStatus::Installing { version: version() },
            false,
            None
        ));
        assert!(should_show_progress(
            &AutoUpdateStatus::Updated { version: version() },
            false,
            None
        ));
        // zed-kask: dead code (removed D19) — a manual check that found no update surfaces a
        // positive "up to date" popup, for any check type (only manual checks
        // ever set this status in production).
        assert!(should_show_progress(
            &AutoUpdateStatus::UpToDate { version: version() },
            true,
            None
        ));
        assert!(should_show_progress(
            &AutoUpdateStatus::UpToDate { version: version() },
            false,
            None
        ));
        let error: Arc<anyhow::Error> = Arc::new(anyhow::anyhow!("network timeout"));
        assert!(should_show_progress(
            &AutoUpdateStatus::Errored { error },
            false,
            None
        ));

        // A dismissed status suppresses the matching state…
        let downloading = AutoUpdateStatus::Downloading {
            version: version(),
            progress: Some(0.3),
        };
        assert!(!should_show_progress(
            &downloading,
            true,
            Some(&downloading)
        ));
        // …but does not suppress a different active state.
        assert!(should_show_progress(
            &AutoUpdateStatus::Installing { version: version() },
            true,
            Some(&downloading)
        ));
    }
}
