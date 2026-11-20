//! Curator sub-pages:
//! - Curator: `interaction_mode` (dyad level) + `always_on` toggle +
//!   `algedonic_threshold`.

use super::*;
use settings::CuratorInteractionMode;
use ui::{ContextMenu, DropdownMenu, DropdownStyle, IconPosition};

const INTERACTION_MODES: [CuratorInteractionMode; 3] = [
    CuratorInteractionMode::Control,
    CuratorInteractionMode::Collaboration,
    CuratorInteractionMode::LearningCollaboration,
];

fn interaction_mode_label(mode: CuratorInteractionMode) -> &'static str {
    match mode {
        CuratorInteractionMode::Control => "Level 1 — Control",
        CuratorInteractionMode::Collaboration => "Level 2 — Collaboration",
        CuratorInteractionMode::LearningCollaboration => "Level 3 — Learning collaboration",
    }
}

// ---------------------------------------------------------------------------
// Curator sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_curator_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let curator: kask_bridge::KaskCuratorSettings = raw
        .and_then(|c| c.curator)
        .map(Into::into)
        .unwrap_or_default();
    let always_on = curator.always_on;
    let algedonic_threshold = curator.algedonic_threshold.to_string();
    let interaction_mode = curator.interaction_mode;

    let interaction_mode_menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
        for mode in INTERACTION_MODES {
            menu = menu.toggleable_entry(
                interaction_mode_label(mode),
                mode == interaction_mode,
                IconPosition::Start,
                None,
                move |_, cx| {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .curator
                                .get_or_insert_default()
                                .interaction_mode = Some(mode);
                        },
                    );
                },
            );
        }
        menu
    });
    let interaction_mode_dropdown = DropdownMenu::new(
        "kask-curator-interaction-mode",
        interaction_mode_label(interaction_mode),
        interaction_mode_menu,
    )
    .style(DropdownStyle::Outlined)
    .full_width(true);

    let always_on_toggle = kask_toggle(
        "kask-curator-always-on",
        Some("Always On"),
        Some(
            "Whether the Curator agent is always-on (runs regulation loops in background).".into(),
        ),
        always_on,
        0,
        move |enabled, settings| {
            settings
                .kask
                .get_or_insert_default()
                .curator
                .get_or_insert_default()
                .always_on = Some(enabled);
        },
    );

    let threshold_input = kask_input_field(
        "kask-curator-algedonic-threshold",
        0,
        algedonic_threshold,
        "0.8",
        "Algedonic Threshold",
        |text| text.parse::<f64>().ok(),
        move |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .curator
                .get_or_insert_default()
                .algedonic_threshold = Some(parsed);
        },
    );

    v_flex()
        .id("kask-curator-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Curator"))
                .child(
                    Label::new(
                        "The Curator agent runs regulation loops and monitors algedonic signals.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Interaction Mode"))
                .child(
                    Label::new(
                        "How you work with the Curator. Level 1: you control and the Curator \
                         proposes. Level 2: you collaborate toward shared understanding and \
                         decisions. Level 3: you collaborate and learn from and challenge each \
                         other. Applies to new Curator threads.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(interaction_mode_dropdown),
        )
        .child(Divider::horizontal())
        .child(always_on_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Algedonic Threshold"))
                .child(
                    Label::new(
                        "Algedonic signal threshold (0.0–1.0). Signals above this trigger the Curator.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(threshold_input),
        )
        .into_any_element()
}
