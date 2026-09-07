//! D9: explicit, read-only inventory gate for database passphrase maintenance.
//! Confirmation is not rotation; the shutdown/recovery coordinator is still pending.
use super::*;
use settings::Settings as _;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Default)]
struct InventoryState {
    additional: String,
    exclusions: String,
    preview: Option<Arc<kask_bridge::DatabaseInventory>>,
    confirmed: Option<kask_bridge::ConfirmedInventory>,
    error: Option<String>,
    busy: bool,
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
    let configuration = kask_bridge::KaskSettings::get_global(cx).mcp_env();
    let state = cx.global_mut::<InventoryState>();
    if state.configuration.as_ref() != Some(&configuration) {
        state.configuration = Some(configuration);
        state.generation = state.generation.saturating_add(1);
        state.preview = None;
        state.confirmed = None;
    }
    let busy = state.busy;
    let preview = state.preview.clone();
    let error = state.error.clone();
    let confirmed = state
        .confirmed
        .as_ref()
        .map(|receipt| receipt.rotate_paths().len());
    let mut content = v_flex().gap_3().min_w_0()
        .child(SettingsSectionHeader::new("Database Passphrase Maintenance"))
        .child(Label::new("Review the complete shared-key database inventory before maintenance. Preview and confirmation do not open databases, read keys, or change the passphrase.").size(LabelSize::Small))
        .child(Label::new("Add historical/external database paths as a JSON array of absolute paths. Directory symlinks are not searched. The catalogue and lease markers cannot discover every database created by older or independent programs.").size(LabelSize::Small))
        .child(SettingsInputField::new("kask-maintenance-additional-paths").with_initial_text(state.additional.clone()).with_placeholder("[]").aria_label("Additional absolute database paths as JSON")
            .on_change(|value, cx| update_input(true, value, cx)))
        .child(Label::new("Exclude independent-key or unencrypted databases using a JSON object mapping each absolute path to its reason. Configured shared-key databases cannot be excluded.").size(LabelSize::Small))
        .child(SettingsInputField::new("kask-maintenance-exclusions").with_initial_text(state.exclusions.clone()).with_placeholder("{}").aria_label("Database exclusions with reasons as JSON")
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
    if let Some(error) = error {
        content = content.child(Label::new(error).color(Color::Error).size(LabelSize::Small));
    }
    if let Some(count) = confirmed {
        content = content.child(Label::new(format!("Inventory confirmed for {count} existing databases. No rotation occurred. Maintenance restart and recovery orchestration are not yet available; the unsafe live-rotation action is disabled.")).size(LabelSize::Small));
    } else {
        content = content.child(Label::new("Maintenance restart is being implemented. No database/keychain mutation can be started from this page yet.").size(LabelSize::Small).color(Color::Muted));
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
            });
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
