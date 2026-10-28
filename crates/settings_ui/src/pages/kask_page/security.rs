//! D9: database passphrase maintenance — schedule a change for the next
//! restart and review the confirmed inventory. Scheduling never rotates a
//! live database: the rotation runs at editor startup, before any database
//! opens (see `kask_bridge::passphrase_rotation`).
use super::*;
use settings::Settings as _;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The scheduled-change state as displayed: how many confirmed databases
/// the pending rotation covers, and the last application error if any.
#[derive(Clone)]
struct PendingDisplay {
    path_count: usize,
    last_error: Option<String>,
}

/// What the display cache knows so far. `NotLoaded` means the background
/// keychain read has not completed; the render shows a muted note until
/// the next refresh picks the loaded value up.
enum PendingDisplayLoad {
    NotLoaded,
    Loaded(Option<PendingDisplay>),
}

/// Display cache for the scheduled-change state. Outer `None` = not yet
/// loaded from the keychain; the background loader fills it once per
/// process (the same pattern as `ensure_keychain_prefetch`).
static PENDING_DISPLAY_CACHE: std::sync::Mutex<Option<Option<PendingDisplay>>> =
    std::sync::Mutex::new(None);
static PENDING_DISPLAY_LOAD_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
struct InventoryState {
    additional: String,
    exclusions: String,
    new_passphrase: String,
    preview: Option<Arc<kask_bridge::DatabaseInventory>>,
    confirmed: Option<kask_bridge::ConfirmedInventory>,
    error: Option<String>,
    busy: bool,
    action_busy: bool,
    generation: u64,
    configuration: Option<std::collections::HashMap<String, String>>,
}
impl gpui::Global for InventoryState {}

pub(crate) fn render_security_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    if !cx.has_global::<InventoryState>() {
        cx.set_global(InventoryState::default());
    }
    ensure_pending_rotation_display(cx);
    let configuration = kask_bridge::KaskSettings::get_global(cx).mcp_env();
    let state = cx.global_mut::<InventoryState>();
    if state.configuration.as_ref() != Some(&configuration) {
        state.configuration = Some(configuration);
        state.generation = state.generation.saturating_add(1);
        state.preview = None;
        state.confirmed = None;
    }
    let busy = state.busy;
    let action_busy = state.action_busy;
    let preview = state.preview.clone();
    let confirmed_count = state
        .confirmed
        .as_ref()
        .map(|receipt| receipt.rotate_paths().len());
    let confirmed = state.confirmed.is_some();
    let error = state.error.clone();
    let additional = state.additional.clone();
    let exclusions = state.exclusions.clone();
    let pending = pending_display_cached();
    let mut content = v_flex().gap_3().min_w_0()
        .child(SettingsSectionHeader::new("Database Passphrase Maintenance"))
        .child(Label::new("One shared passphrase encrypts every kask memory database. It starts as the default \"allostery\" and can be changed below; the change re-encrypts every database in the confirmed inventory the next time the editor restarts, so no database is ever rotated while it is open.").size(LabelSize::Small));
    // ── Scheduled-change state or the change form ──
    match pending {
        PendingDisplayLoad::NotLoaded => {
            content = content.child(
                Label::new("Checking for a scheduled passphrase change…")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            );
        }
        PendingDisplayLoad::Loaded(None) => {
            content = content
                .child(Label::new("New passphrase").size(LabelSize::Small))
                .child(
                    SettingsInputField::new("kask-maintenance-new-passphrase")
                        .with_placeholder("at least 8 characters")
                        .aria_label("New database passphrase")
                        .on_change(|value, cx| {
                            let state = cx.global_mut::<InventoryState>();
                            if state.new_passphrase != value {
                                state.new_passphrase = value;
                            }
                        }),
                )
                .child(
                    Button::new(
                        "kask-maintenance-change",
                        if action_busy {
                            "Scheduling…"
                        } else {
                            "Change passphrase at next restart"
                        },
                    )
                    .disabled(action_busy || busy || !confirmed)
                    .on_click(|_, window, cx| confirm_change(window, cx)),
                );
            if !confirmed {
                content = content.child(
                    Label::new(
                        "Confirm the database inventory below before changing the passphrase.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                );
            }
        }
        PendingDisplayLoad::Loaded(Some(pending)) => {
            content = content.child(
                Label::new(format!(
                    "A passphrase change is scheduled for {} database(s) — restart the \
                     editor to apply it. Nothing is re-encrypted until then; cancelling \
                     here removes the change without touching any database.",
                    pending.path_count
                ))
                .size(LabelSize::Small),
            );
            if let Some(last_error) = pending.last_error {
                content = content.child(
                    Label::new(format!("The last application attempt failed: {last_error}"))
                        .size(LabelSize::Small)
                        .color(Color::Error),
                );
            }
            content = content.child(
                Button::new(
                    "kask-maintenance-cancel",
                    if action_busy {
                        "Cancelling…"
                    } else {
                        "Cancel scheduled change"
                    },
                )
                .disabled(action_busy)
                .on_click(|_, _, cx| cancel_change(cx)),
            );
        }
    }
    if let Some(error) = error {
        content = content.child(Label::new(error).color(Color::Error).size(LabelSize::Small));
    }
    // ── Advanced: database inventory review ──
    content = content
        .child(SettingsSectionHeader::new("Database inventory review (advanced)"))
        .child(Label::new("Only needed for databases outside kask's standard locations: list their absolute paths as a JSON array (example: [\"/path/to/older.db\"]) so they are re-encrypted together with everything else. Standard kask databases are always included. Directory symlinks are not searched.").size(LabelSize::Small))
        .child(SettingsInputField::new("kask-maintenance-additional-paths").with_initial_text(additional).with_placeholder("[]").aria_label("Additional absolute database paths as JSON")
            .on_change(|value, cx| update_input(true, value, cx)))
        .child(Label::new("Databases with their own independent key or no encryption must be listed here as a JSON object mapping each path to a reason (example: {\"/path/to/plain.db\": \"unencrypted\"}), or the change cannot be confirmed. Standard kask databases cannot be excluded.").size(LabelSize::Small))
        .child(SettingsInputField::new("kask-maintenance-exclusions").with_initial_text(exclusions).with_placeholder("{}").aria_label("Database exclusions with reasons as JSON")
            .on_change(|value, cx| update_input(false, value, cx)))
        .child(Button::new("kask-maintenance-preview", if busy { "Reading inventory…" } else { "Preview inventory" }).disabled(busy)
            .on_click(|_, _, cx| start_review(false, cx)));
    if let Some(preview) = preview {
        content = content.child(
            Label::new(format!(
                "{} database paths; {} searched roots",
                preview.entries.len(),
                preview.search_roots.len()
            ))
            .size(LabelSize::Small),
        );
        content = content.child(
            gpui::uniform_list(
                "kask-maintenance-paths",
                preview.entries.len(),
                move |range, _, _| {
                    preview
                        .entries
                        .iter()
                        .enumerate()
                        .skip(range.start)
                        .take(range.len())
                        .map(|(index, entry)| {
                            let status = if entry.recovery_artifact.is_some() {
                                "RECOVERY REQUIRED"
                            } else if !entry.exists {
                                "MISSING — will not create"
                            } else if entry.configured {
                                "CONFIGURED"
                            } else {
                                "REVIEW KEY SCOPE"
                            };
                            div()
                                .id(("maintenance-path", index))
                                .h_8()
                                .min_w_0()
                                .overflow_x_scroll()
                                .whitespace_nowrap()
                                .text_sm()
                                .child(format!("{status}: {}", entry.path.display()))
                        })
                        .collect()
                },
            )
            .h(px(240.0))
            .w_full(),
        );
        content = content.child(
            Button::new("kask-maintenance-confirm", "Confirm inventory")
                .disabled(busy)
                .on_click(|_, window, cx| confirm_prompt(window, cx)),
        );
    }
    if let Some(count) = confirmed_count {
        content = content.child(
            Label::new(format!(
                "Inventory confirmed for {count} existing databases. The change form above \
                 is enabled; nothing is re-encrypted until you schedule a change and restart \
                 the editor."
            ))
            .size(LabelSize::Small),
        );
    } else {
        content = content.child(
            Label::new("Preview and confirm the inventory to enable the change form above.")
                .size(LabelSize::Small)
                .color(Color::Muted),
        );
    }
    v_flex()
        .id("kask-security-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(content)
        .into_any_element()
}

fn pending_display_from_bridge() -> Option<PendingDisplay> {
    kask_bridge::read_pending_rotation_state()
        .ok()
        .flatten()
        .map(|state| PendingDisplay {
            path_count: state.rotate_paths.len(),
            last_error: state.last_error,
        })
}

fn ensure_pending_rotation_display(cx: &impl gpui::AppContext) {
    if PENDING_DISPLAY_LOAD_STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    cx.background_spawn(async move {
        let display = pending_display_from_bridge();
        if let Ok(mut guard) = PENDING_DISPLAY_CACHE.lock() {
            *guard = Some(display);
        }
    })
    .detach();
}

fn pending_display_cached() -> PendingDisplayLoad {
    match PENDING_DISPLAY_CACHE.lock() {
        Ok(guard) => match guard.clone() {
            Some(display) => PendingDisplayLoad::Loaded(display),
            None => PendingDisplayLoad::NotLoaded,
        },
        Err(_) => PendingDisplayLoad::NotLoaded,
    }
}

fn set_pending_display_cached(display: Option<PendingDisplay>) {
    if let Ok(mut guard) = PENDING_DISPLAY_CACHE.lock() {
        *guard = Some(display);
    }
}

fn confirm_change(window: &mut Window, cx: &mut App) {
    let state = cx.global::<InventoryState>();
    let Some(receipt) = state.confirmed.clone() else {
        return;
    };
    let generation = state.generation;
    let new_passphrase = state.new_passphrase.clone();
    if new_passphrase.trim().is_empty() {
        let state = cx.global_mut::<InventoryState>();
        state.error = Some("Enter a new passphrase first.".to_string());
        cx.refresh_windows();
        return;
    }
    let answer = window.prompt(
        gpui::PromptLevel::Warning,
        "Schedule the passphrase change for the next restart?",
        Some("Every database in the confirmed inventory is re-encrypted with the new passphrase when the editor next starts, before anything opens them. If any database fails, all of them keep the old passphrase. Cancel on this page removes the scheduled change."),
        &["Schedule change", "Cancel"],
        cx,
    );
    cx.spawn(async move |cx| {
        if !matches!(answer.await, Ok(0)) {
            return;
        }
        let schedule = cx
            .background_spawn(async move {
                kask_bridge::schedule_db_passphrase_rotation(&new_passphrase, &receipt)
            })
            .await;
        let display = cx
            .background_spawn(async move { pending_display_from_bridge() })
            .await;
        cx.update(|cx| {
            let state = cx.global_mut::<InventoryState>();
            state.action_busy = false;
            state.new_passphrase.clear();
            if state.generation != generation {
                state.error = Some(
                    "Inventory changed while scheduling; preview and confirm again.".to_string(),
                );
            } else {
                match schedule {
                    Ok(()) => state.error = None,
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            set_pending_display_cached(display);
            cx.refresh_windows();
        });
    })
    .detach();
}

fn cancel_change(cx: &mut App) {
    cx.spawn(async move |cx| {
        let cancel = cx
            .background_spawn(async move { kask_bridge::cancel_pending_db_rotation() })
            .await;
        let display = cx
            .background_spawn(async move { pending_display_from_bridge() })
            .await;
        cx.update(|cx| {
            let state = cx.global_mut::<InventoryState>();
            state.action_busy = false;
            match cancel {
                Ok(()) => state.error = None,
                Err(error) => state.error = Some(error.to_string()),
            }
            set_pending_display_cached(display);
            cx.refresh_windows();
        });
    })
    .detach();
}

fn update_input(additional: bool, value: String, cx: &mut App) {
    let state = cx.global_mut::<InventoryState>();
    if (if additional {
        &state.additional
    } else {
        &state.exclusions
    }) == &value
    {
        return;
    }
    if additional {
        state.additional = value;
    } else {
        state.exclusions = value;
    }
    state.generation = state.generation.saturating_add(1);
    state.preview = None;
    state.confirmed = None;
    state.error = None;
    cx.refresh_windows();
}

fn confirm_prompt(window: &mut Window, cx: &mut App) {
    let generation = cx.global::<InventoryState>().generation;
    let answer = window.prompt(gpui::PromptLevel::Warning, "Confirm database key scope and completeness?", Some("I have included all historical/external shared-key databases. Every non-excluded existing path uses the shared passphrase; each exclusion is independent and justified. This confirms inventory only, not rotation."), &["Confirm inventory", "Cancel"], cx);
    cx.spawn(async move |cx| {
        if matches!(answer.await, Ok(0)) {
            cx.update(|cx| {
                if cx.global::<InventoryState>().generation == generation {
                    start_review(true, cx);
                } else {
                    cx.global_mut::<InventoryState>().error =
                        Some("Inputs changed; preview and confirm again.".into());
                    cx.refresh_windows();
                }
            })
        }
    })
    .detach();
}

fn start_review(confirm: bool, cx: &mut App) {
    let settings = kask_bridge::KaskSettings::get_global(cx).clone();
    let configuration = settings.mcp_env();
    let state = cx.global_mut::<InventoryState>();
    if state.busy {
        return;
    }
    state.busy = true;
    state.error = None;
    state.confirmed = None;
    state.generation = state.generation.saturating_add(1);
    let generation = state.generation;
    let additional = state.additional.clone();
    let exclusions = state.exclusions.clone();
    let previous = state.preview.clone();
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move {
                let additional: Vec<PathBuf> =
                    serde_json::from_str(if additional.trim().is_empty() {
                        "[]"
                    } else {
                        &additional
                    })
                    .map_err(|error| format!("Invalid additional paths JSON: {error}"))?;
                let exclusions: BTreeMap<PathBuf, String> =
                    serde_json::from_str(if exclusions.trim().is_empty() {
                        "{}"
                    } else {
                        &exclusions
                    })
                    .map_err(|error| format!("Invalid exclusions JSON: {error}"))?;
                let current = kask_bridge::preview_database_inventory(&settings, &additional)
                    .map_err(|error| error.to_string())?;
                let receipt = if confirm {
                    Some(
                        previous
                            .ok_or_else(|| "Preview the inventory first".to_string())?
                            .confirm(&current, &exclusions, true)
                            .map_err(|error| error.to_string())?,
                    )
                } else {
                    None
                };
                Ok::<_, String>((Arc::new(current), receipt))
            })
            .await;
        cx.update(|cx| {
            let current_configuration = kask_bridge::KaskSettings::get_global(cx).mcp_env();
            let state = cx.global_mut::<InventoryState>();
            state.busy = false;
            if configuration != current_configuration {
                state.preview = None;
                state.confirmed = None;
                state.generation = state.generation.saturating_add(1);
                state.configuration = Some(current_configuration);
                state.error = Some("Settings changed; preview the inventory again.".into());
            }
            if state.generation == generation {
                match result {
                    Ok((preview, receipt)) => {
                        state.preview = Some(preview);
                        state.confirmed = receipt;
                    }
                    Err(error) => {
                        state.error = Some(error);
                        state.confirmed = None;
                    }
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    /// D9: the change-passphrase control must dispatch through the bridge
    /// scheduler, stay gated on a confirmed inventory, surface an existing
    /// scheduled change, and keep it cancellable.
    #[test]
    fn change_passphrase_control_schedules_through_the_bridge_and_is_gated() {
        let page = include_str!("security.rs");
        assert!(
            page.contains("schedule_db_passphrase_rotation"),
            "the change action must call the bridge scheduler"
        );
        assert!(
            page.contains(".disabled(action_busy || busy || !confirmed)"),
            "the change action must be gated on a confirmed inventory"
        );
        assert!(
            page.contains("read_pending_rotation_state"),
            "the page must surface an existing scheduled change"
        );
        assert!(
            page.contains("cancel_pending_db_rotation"),
            "a scheduled change must be cancellable"
        );
    }
}
