//! zed-kask: Shared chrome for marketplace-style catalog pages (card
//! container, bordered search bar, empty-state row). Extracted from the
//! duplicated copies in `crates/extensions_ui` (upstream) and
//! `crates/kask_extensions_ui` so both pages render identical chrome from
//! one source. Upstream `extensions_ui` is intentionally left untouched
//! (minimal-divergence); this crate is the adoption point if the shared
//! components are ever upstreamed.

use editor::{Editor, EditorElement, EditorStyle};
use gpui::{AnyElement, App, Entity, KeyContext, TextStyle, Window, prelude::*, relative, rems};
use settings::Settings;
use smallvec::SmallVec;
use theme_settings::ThemeSettings;
use ui::prelude::*;

/// The bordered card container used by both extension and kask-skill
/// catalog pages. Callers supply the card's inner layout as children.
#[derive(IntoElement)]
pub struct MarketplaceCard {
    children: SmallVec<[AnyElement; 2]>,
}

impl MarketplaceCard {
    pub fn new() -> Self {
        Self {
            children: SmallVec::new(),
        }
    }
}

impl ParentElement for MarketplaceCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for MarketplaceCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div().w_full().child(
            v_flex()
                .mt_4()
                .w_full()
                .h(rems_from_px(110.))
                .p_3()
                .gap_2()
                .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
                .border_1()
                .border_color(cx.theme().colors().border_variant)
                .rounded_md()
                .children(self.children),
        )
    }
}

/// The bordered single-line search bar with a magnifying-glass icon,
/// identical on both catalog pages.
pub fn marketplace_search_bar(
    editor: &Entity<Editor>,
    query_contains_error: bool,
    cx: &mut App,
) -> Div {
    let mut key_context = KeyContext::new_with_defaults();
    key_context.add("BufferSearchBar");

    let editor_border = if query_contains_error {
        Color::Error.color(cx)
    } else {
        cx.theme().colors().border
    };

    h_flex()
        .key_context(key_context)
        .h_8()
        .min_w(rems_from_px(384.))
        .flex_1()
        .pl_1p5()
        .pr_2()
        .gap_2()
        .border_1()
        .border_color(editor_border)
        .rounded_md()
        .child(Icon::new(IconName::MagnifyingGlass).color(Color::Muted))
        .child(marketplace_text_input(editor, cx))
}

/// The themed single-line editor element used inside the search bar.
pub fn marketplace_text_input(editor: &Entity<Editor>, cx: &mut App) -> impl IntoElement {
    let settings = ThemeSettings::get_global(cx);
    let text_style = TextStyle {
        color: if editor.read(cx).read_only(cx) {
            cx.theme().colors().text_disabled
        } else {
            cx.theme().colors().text
        },
        font_family: settings.ui_font.family.clone(),
        font_features: settings.ui_font.features.clone(),
        font_fallbacks: settings.ui_font.fallbacks.clone(),
        font_size: rems(0.875).into(),
        font_weight: settings.ui_font.weight,
        line_height: relative(1.3),
        ..Default::default()
    };

    EditorElement::new(
        editor,
        EditorStyle {
            background: cx.theme().colors().editor_background,
            local_player: cx.theme().players().local(),
            text: text_style,
            ..Default::default()
        },
    )
}

/// The empty-state row shown when a catalog page has no entries to list.
/// Pass `failed = true` to prefix the message with a warning icon.
pub fn marketplace_empty_state(message: impl Into<SharedString>, failed: bool) -> impl IntoElement {
    h_flex()
        .py_4()
        .gap_1p5()
        .when(failed, |this| {
            this.child(
                Icon::new(IconName::Warning)
                    .size(IconSize::Small)
                    .color(Color::Warning),
            )
        })
        .child(Label::new(message.into()))
}
