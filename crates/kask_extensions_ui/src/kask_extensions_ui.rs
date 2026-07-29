mod components;
mod panel_button;
mod publish;

pub use panel_button::KaskExtensionsButton;
pub use publish::{generate_version, install_skill, publish_skill, unpublish_skill, vote_skill};

use std::time::Duration;
use std::{ops::Range, sync::Arc};

use anyhow::Context as _;
use cloud_api_types::{ExtensionProvides, GetKaskSkillsResponse, KaskSkillMetadata};
use editor::{Editor, EditorElement, EditorStyle};
use extension_host::ExtensionStore;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, InteractiveElement, KeyContext, ParentElement,
    Render, Styled, Task, TextStyle, UniformListScrollHandle, Window, actions, point, uniform_list,
};
use theme_settings::ThemeSettings;
use ui::{
    ScrollableHandle, ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle,
    ToggleButtonSimple, WithScrollbar, prelude::*,
};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, Settings},
};

use crate::components::ExtensionCard;

actions!(
    kask_extensions,
    [
        /// Deploys a new Kask Extensions page if none is open, else focuses the
        /// existing one. Used by the View menu entry and the status bar button.
        Toggle,
        /// Focuses an existing Kask Extensions page (no-op if none is open).
        ToggleFocus,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(move |workspace: &mut Workspace, window, _cx| {
        let Some(_window) = window else {
            return;
        };
        // zed-kask: `Toggle` deploys a new KaskExtensionsPage if none is open
        // in the active pane, else focuses the existing one. `ToggleFocus`
        // only focuses. Per the `.rules` trap "Center-pane Item Toggle vs
        // ToggleFocus", the View menu entry uses `Toggle` (not `ToggleFocus`)
        // so it deploys a new item if none exists.
        workspace
            .register_action(move |workspace, _: &Toggle, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<KaskExtensionsPage>());

                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                } else {
                    let extensions_page =
                        KaskExtensionsPage::new(workspace, None, None, window, cx);
                    workspace.add_item_to_active_pane(
                        Box::new(extensions_page.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    // zed-kask: explicitly focus the new page's query editor.
                    // Without this, the first Toggle click adds the item but
                    // leaves focus on the previous element (e.g. the View
                    // menu), so the user has to click a second time to actually
                    // interact with the page. `KaskExtensionsPage::focus_handle`
                    // delegates to the query editor, which is constructed
                    // inside `cx.new` and isn't reachable through the workspace
                    // focus chain on the same turn unless we focus it here.
                    extensions_page.focus_handle(cx).focus(window, cx);
                }
            })
            .register_action(move |workspace, _: &ToggleFocus, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<KaskExtensionsPage>());
                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                }
            });
    })
    .detach();
}

// zed-kask: kask skill status mirrors ExtensionStatus but for kask skills.
// Tracks whether a skill is installed, installing, or not installed.
#[derive(Clone, Debug)]
pub enum KaskSkillStatus {
    NotInstalled,
    Installing,
    Installed(Arc<str>),
    Removing,
    Upgrading,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum ExtensionFilter {
    All,
    Installed,
    NotInstalled,
}

pub struct KaskExtensionsPage {
    list: UniformListScrollHandle,
    is_fetching_extensions: bool,
    fetch_failed: bool,
    filter: ExtensionFilter,
    // zed-kask: kask skill catalog entries (replaces remote_extension_entries)
    remote_skill_entries: Vec<KaskSkillMetadata>,
    filtered_remote_skill_indices: Vec<usize>,
    query_editor: Entity<Editor>,
    query_contains_error: bool,
    _subscriptions: [gpui::Subscription; 2],
    skill_fetch_task: Option<Task<()>>,
    // zed-kask: track in-flight install/uninstall operations by skill id
    outstanding_operations: collections::BTreeMap<Arc<str>, KaskSkillStatus>,
    // zed-kask: track the HTTP client for catalog fetches and install/vote
    http_client: Option<Arc<http_client::HttpClientWithUrl>>,
    // zed-kask: track the fs for install/uninstall
    fs: Option<Arc<dyn fs::Fs>>,
    // zed-kask: track the client for credentials (auth headers)
    client: Option<Arc<client::Client>>,
}

impl KaskExtensionsPage {
    pub fn new(
        _workspace: &Workspace,
        _provides_filter: Option<ExtensionProvides>,
        _focus_skill_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let app_state = workspace::AppState::global(cx);
            let http_client = app_state.client.http_client();
            let fs = app_state.fs.clone();

            // zed-kask: subscribe to ExtensionStore for dev-extension
            // rebuild visibility (kept from the fork). The kask catalog is
            // fetched separately via the kask skills API.
            let store = ExtensionStore::global(cx);
            let subscriptions = [
                cx.observe(&store, |_: &mut Self, _, cx| cx.notify()),
                cx.subscribe_in(
                    &store,
                    window,
                    move |this, _, event, _window, cx| match event {
                        extension_host::Event::ExtensionsUpdated => {
                            this.fetch_kask_skills(cx);
                        }
                        _ => {}
                    },
                ),
            ];

            let query_editor = cx.new(|cx| {
                let mut input = Editor::single_line(window, cx);
                input.set_placeholder_text("Search kask skills...", window, cx);
                input
            });
            cx.subscribe(&query_editor, Self::on_query_change).detach();

            let scroll_handle = UniformListScrollHandle::new();

            let mut this = Self {
                list: scroll_handle,
                is_fetching_extensions: false,
                fetch_failed: false,
                filter: ExtensionFilter::All,
                remote_skill_entries: Vec::new(),
                filtered_remote_skill_indices: Vec::new(),
                query_contains_error: false,
                _subscriptions: subscriptions,
                skill_fetch_task: None,
                outstanding_operations: collections::BTreeMap::default(),
                http_client: Some(http_client),
                fs: Some(fs),
                client: Some(app_state.client.clone()),
                query_editor,
            };
            this.fetch_kask_skills(cx);
            this
        })
    }

    fn filter_extension_entries(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter;
        let indices: Vec<usize> = self
            .remote_skill_entries
            .iter()
            .enumerate()
            .filter(|(_, skill)| match filter {
                ExtensionFilter::All => true,
                ExtensionFilter::Installed => {
                    let status = self.skill_status(&skill.id, cx);
                    matches!(status, KaskSkillStatus::Installed(_))
                }
                ExtensionFilter::NotInstalled => {
                    let status = self.skill_status(&skill.id, cx);
                    matches!(status, KaskSkillStatus::NotInstalled)
                }
            })
            .map(|(ix, _)| ix)
            .collect();
        self.filtered_remote_skill_indices = indices;

        cx.notify();
    }

    fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        self.list.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// zed-kask: Fetch the kask skill catalog from `GET /api/kask-skills`.
    /// Replaces the extension fetch. The catalog is the source of truth for
    /// what skills are available to install.
    fn fetch_kask_skills(&mut self, cx: &mut Context<Self>) {
        let Some(http_client) = self.http_client.clone() else {
            log::warn!(
                "kask-extensions: no HTTP client available; cannot fetch skill catalog. \
                 Remediation: ensure the user is logged in."
            );
            return;
        };

        self.is_fetching_extensions = true;
        self.fetch_failed = false;
        cx.notify();

        let url = crate::publish::kask_marketplace_url(&http_client, "/api/kask-skills", &[]);
        cx.spawn(async move |this, cx| {
            let result = async {
                let url = url?;
                let mut response = http_client
                    .get(&url, http_client::AsyncBody::empty(), true)
                    .await?;
                let mut body = Vec::new();
                futures::AsyncReadExt::read_to_end(response.body_mut(), &mut body)
                    .await
                    .context("error reading kask skills response")?;
                if response.status().is_client_error() {
                    let text = String::from_utf8_lossy(body.as_slice());
                    anyhow::bail!(
                        "status error {}, response: {text:?}",
                        response.status().as_u16()
                    );
                }
                let response: GetKaskSkillsResponse = serde_json::from_slice(&body)?;
                Ok::<_, anyhow::Error>(response.data)
            }
            .await;

            this.update(cx, |this, cx| {
                this.is_fetching_extensions = false;
                match result {
                    Ok(skills) => {
                        this.fetch_failed = false;
                        this.remote_skill_entries = skills;
                        this.filter_extension_entries(cx);
                    }
                    Err(err) => {
                        this.fetch_failed = true;
                        this.filter_extension_entries(cx);
                        log::warn!(
                            "kask-extensions: failed to fetch skill catalog: {err:#}. \
                             Remediation: check network connectivity and server availability."
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// zed-kask: Check the install status of a kask skill. Mirrors
    /// `extension_status` but checks the `SkillIndex` for installed
    /// marketplace skills and the `outstanding_operations` map for in-flight
    /// operations.
    fn skill_status(&self, skill_id: &str, cx: &mut Context<Self>) -> KaskSkillStatus {
        if let Some(status) = self.outstanding_operations.get(skill_id) {
            return status.clone();
        }
        // Check if the skill is installed in the SkillIndex.
        if let Some(index) = cx.try_global::<agent_skills::SkillIndex>() {
            let is_installed = index
                .global_skills
                .iter()
                .any(|s| matches!(&s.source, agent_skills::SkillSource::Public { original_skill_id, .. } if original_skill_id.as_ref() == skill_id));
            if is_installed {
                return KaskSkillStatus::Installed("latest".into());
            }
        }
        KaskSkillStatus::NotInstalled
    }

    fn render_extensions(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<ExtensionCard> {
        let mut cards = Vec::new();
        for ix in range {
            if ix >= self.filtered_remote_skill_indices.len() {
                break;
            }
            let skill_ix = self.filtered_remote_skill_indices[ix];
            let skill = self.remote_skill_entries[skill_ix].clone();
            let card = self.render_skill_card(skill, cx);
            cards.push(card);
        }
        cards
    }

    /// zed-kask: Render a kask skill card with install/uninstall/vote buttons.
    fn render_skill_card(
        &mut self,
        skill: KaskSkillMetadata,
        cx: &mut Context<Self>,
    ) -> ExtensionCard {
        let status = self.skill_status(&skill.id, cx);
        let skill_id = skill.id.clone();
        let skill_id_for_uninstall = skill.id.clone();
        let skill_id_for_vote_up = skill.id.clone();
        let skill_id_for_vote_down = skill.id.clone();
        let http_client = self.http_client.clone();
        let fs = self.fs.clone();
        let marketplace_dir = agent_skills::global_skills_dir().join("_marketplace");

        ExtensionCard::new().child(
            h_flex()
                .w_full()
                .gap_2()
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Label::new(skill.id.clone()).color(Color::Default))
                                .child(
                                    Label::new(format!("↓ {}", skill.download_count))
                                        .color(Color::Muted),
                                )
                                .child(
                                    Label::new(format!("▲ {}", skill.upvote_count))
                                        .color(Color::Muted),
                                )
                                .child(
                                    Label::new(format!("▼ {}", skill.downvote_count))
                                        .color(Color::Muted),
                                ),
                        )
                        .child(Label::new(skill.manifest.description.clone()).color(Color::Muted)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .when(matches!(status, KaskSkillStatus::NotInstalled), |this| {
                            this.child(
                                Button::new(
                                    SharedString::from(format!("install-{}", skill_id)),
                                    "Install",
                                )
                                .style(ButtonStyle::Filled)
                                .disabled(matches!(status, KaskSkillStatus::Installing))
                                .on_click(cx.listener(
                                    move |this, _event, _window, cx| {
                                        this.install_kask_skill(
                                            skill_id.clone(),
                                            http_client.clone(),
                                            fs.clone(),
                                            marketplace_dir.clone(),
                                            cx,
                                        );
                                    },
                                )),
                            )
                        })
                        .when(matches!(status, KaskSkillStatus::Installed(_)), |this| {
                            this.child(
                                Button::new(
                                    SharedString::from(format!(
                                        "uninstall-{}",
                                        skill_id_for_uninstall
                                    )),
                                    "Uninstall",
                                )
                                .style(ButtonStyle::Subtle)
                                .disabled(matches!(status, KaskSkillStatus::Removing))
                                .on_click(cx.listener(
                                    move |this, _event, _window, cx| {
                                        this.uninstall_kask_skill(
                                            skill_id_for_uninstall.clone(),
                                            cx,
                                        );
                                    },
                                )),
                            )
                        })
                        .when(matches!(status, KaskSkillStatus::Installing), |this| {
                            this.child(Label::new("Installing...").color(Color::Muted))
                        })
                        .when(matches!(status, KaskSkillStatus::Removing), |this| {
                            this.child(Label::new("Removing...").color(Color::Muted))
                        })
                        .child(
                            IconButton::new(
                                SharedString::from(format!("vote-up-{}", skill_id_for_vote_up)),
                                IconName::ThumbsUp,
                            )
                            .icon_size(IconSize::Small)
                            .on_click(cx.listener(
                                move |this, _event, _window, cx| {
                                    this.vote_kask_skill(skill_id_for_vote_up.clone(), 1, cx);
                                },
                            )),
                        )
                        .child(
                            IconButton::new(
                                SharedString::from(format!("vote-down-{}", skill_id_for_vote_down)),
                                IconName::ThumbsDown,
                            )
                            .icon_size(IconSize::Small)
                            .on_click(cx.listener(
                                move |this, _event, _window, cx| {
                                    this.vote_kask_skill(skill_id_for_vote_down.clone(), -1, cx);
                                },
                            )),
                        ),
                ),
        )
    }

    /// zed-kask: Install a kask skill from the marketplace.
    fn install_kask_skill(
        &mut self,
        skill_id: Arc<str>,
        http_client: Option<Arc<http_client::HttpClientWithUrl>>,
        fs: Option<Arc<dyn fs::Fs>>,
        marketplace_dir: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(http_client) = http_client else {
            log::warn!(
                "kask-extensions: no HTTP client available; cannot install skill '{}'. \
                 Remediation: ensure the user is logged in.",
                skill_id
            );
            return;
        };
        let Some(fs) = fs else {
            log::warn!(
                "kask-extensions: no filesystem available; cannot install skill '{}'. \
                 Remediation: ensure the app state is initialized.",
                skill_id
            );
            return;
        };

        // Find the skill in the catalog to get the SHA256.
        let Some(skill) = self.remote_skill_entries.iter().find(|s| s.id == skill_id) else {
            log::warn!(
                "kask-extensions: skill '{}' not found in catalog; cannot install.",
                skill_id
            );
            return;
        };
        let sha256 = skill.manifest.tarball_sha256.clone();
        let dependencies = skill.manifest.dependencies.clone();
        let skill_id_str = skill_id.to_string();

        // zed-kask: Check if the skill's dependencies are installed. If not,
        // log a warning so the user knows they need to install them too.
        // We don't block the install — the user may want to install deps
        // separately. But we notify them so they're not surprised when the
        // skill fails at runtime.
        if !dependencies.is_empty() {
            let installed_names: std::collections::HashSet<String> = cx
                .try_global::<agent_skills::SkillIndex>()
                .map(|idx| idx.global_skills.iter().map(|s| s.name.clone()).collect())
                .unwrap_or_default();
            let missing: Vec<&str> = dependencies
                .iter()
                .filter(|dep| !installed_names.contains(*dep))
                .map(|s| s.as_str())
                .collect();
            if !missing.is_empty() {
                log::warn!(
                    "kask-extensions: skill '{}' depends on {} that are not installed: {}. \
                     The skill will be installed but will fail at runtime until its dependencies are installed. \
                     Install them via the Kask Extensions panel.",
                    skill_id,
                    if missing.len() == 1 {
                        "a skill"
                    } else {
                        "skills"
                    },
                    missing.join(", "),
                );
            }
        }

        self.outstanding_operations
            .insert(skill_id.clone(), KaskSkillStatus::Installing);
        cx.notify();

        let http_client = http_client;
        let fs = fs.clone();
        cx.spawn(async move |this, cx| {
            let result = install_skill(
                fs.as_ref(),
                &http_client,
                &skill_id_str,
                &sha256,
                &marketplace_dir,
            )
            .await;
            this.update(cx, |this, cx| {
                this.outstanding_operations.remove(&skill_id);
                match result {
                    Ok(_install_dir) => {
                        log::info!(
                            "kask-extensions: successfully installed skill '{}'",
                            skill_id
                        );
                        // Fire SkillsUpdatedHook so the Settings page refreshes.
                        let hook = cx
                            .try_global::<agent_skills::SkillsUpdatedHook>()
                            .map(|h| h.0.clone());
                        if let Some(hook) = hook {
                            hook(cx);
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "kask-extensions: failed to install skill '{}': {err:#}. \
                             Remediation: check network connectivity and disk space.",
                            skill_id
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// zed-kask: Uninstall a kask skill.
    fn uninstall_kask_skill(&mut self, skill_id: Arc<str>, cx: &mut Context<Self>) {
        let Some(fs) = self.fs.clone() else {
            log::warn!(
                "kask-extensions: no filesystem available; cannot uninstall skill '{}'.",
                skill_id
            );
            return;
        };

        let (source_user, skill_name) = match skill_id.split_once('/') {
            Some(parts) => parts,
            None => {
                log::warn!(
                    "kask-extensions: invalid skill id '{}'; cannot uninstall.",
                    skill_id
                );
                return;
            }
        };

        let install_dir = agent_skills::global_skills_dir()
            .join("_marketplace")
            .join(source_user)
            .join(skill_name);

        self.outstanding_operations
            .insert(skill_id.clone(), KaskSkillStatus::Removing);
        cx.notify();

        let skill_id_for_hook = skill_id.clone();
        cx.spawn(async move |this, cx| {
            let result = fs
                .remove_dir(
                    &install_dir,
                    fs::RemoveOptions {
                        recursive: true,
                        ignore_if_not_exists: true,
                    },
                )
                .await;
            this.update(cx, |this, cx| {
                this.outstanding_operations.remove(&skill_id);
                match result {
                    Ok(()) => {
                        log::info!(
                            "kask-extensions: successfully uninstalled skill '{}'",
                            skill_id_for_hook
                        );
                        let hook = cx
                            .try_global::<agent_skills::SkillsUpdatedHook>()
                            .map(|h| h.0.clone());
                        if let Some(hook) = hook {
                            hook(cx);
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "kask-extensions: failed to uninstall skill '{}': {err:#}.",
                            skill_id_for_hook
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// zed-kask: Vote on a kask skill (+1 or -1).
    fn vote_kask_skill(&mut self, skill_id: Arc<str>, vote: i8, cx: &mut Context<Self>) {
        let Some(http_client) = self.http_client.clone() else {
            log::warn!(
                "kask-extensions: no HTTP client available; cannot vote on skill '{}'.",
                skill_id
            );
            return;
        };
        let Some(client) = self.client.clone() else {
            log::warn!(
                "kask-extensions: no client available; cannot vote on skill '{}'.",
                skill_id
            );
            return;
        };
        let Some(credentials) = client.credentials() else {
            log::warn!(
                "kask-extensions: not logged in; cannot vote on skill '{}'. \
                 Remediation: sign in to Zed to vote.",
                skill_id
            );
            return;
        };

        let skill_id_str = skill_id.to_string();
        cx.spawn(async move |this, cx| {
            let result = vote_skill(&http_client, &credentials, &skill_id_str, vote).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((up, down)) => {
                        // Update the local catalog entry with the new counts.
                        if let Some(skill) = this
                            .remote_skill_entries
                            .iter_mut()
                            .find(|s| s.id.as_ref() == skill_id_str)
                        {
                            skill.upvote_count = up;
                            skill.downvote_count = down;
                        }
                        log::info!(
                            "kask-extensions: voted {} on skill '{}': ▲{} ▼{}",
                            vote,
                            skill_id_str,
                            up,
                            down
                        );
                    }
                    Err(err) => {
                        log::warn!(
                            "kask-extensions: failed to vote on skill '{}': {err:#}.",
                            skill_id_str
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_search(&self, cx: &mut Context<Self>) -> Div {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("BufferSearchBar");

        let editor_border = if self.query_contains_error {
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
            .child(self.render_text_input(&self.query_editor, cx))
    }

    fn render_text_input(
        &self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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

    fn on_query_change(
        &mut self,
        _: Entity<Editor>,
        event: &editor::EditorEvent,
        cx: &mut Context<Self>,
    ) {
        if let editor::EditorEvent::Edited { .. } = event {
            self.query_contains_error = false;
            self.refresh_search(cx);
        }
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        // zed-kask: debounce the catalog fetch, then filter locally.
        // The kask skills API returns the full catalog; search filtering
        // happens client-side via `filter_extension_entries`.
        self.skill_fetch_task = Some(cx.spawn(async move |this, cx| {
            let search = this
                .update(cx, |this, cx| this.search_query(cx))
                .ok()
                .flatten();

            if search.is_some() {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
            };

            this.update(cx, |this, cx| {
                this.fetch_kask_skills(cx);
                this.filter_extension_entries(cx);
                this.scroll_to_top(cx);
            })
            .ok();
        }));
    }

    pub fn search_query(&self, cx: &mut App) -> Option<String> {
        let search = self.query_editor.read(cx).text(cx);
        if search.trim().is_empty() {
            None
        } else {
            Some(search)
        }
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_search = self.search_query(cx).is_some();

        let message = if self.is_fetching_extensions {
            "Loading kask skills…"
        } else if self.fetch_failed {
            "Failed to load kask skills. Please check your connection and try again."
        } else {
            match self.filter {
                ExtensionFilter::All => {
                    if has_search {
                        "No kask skills that match your search."
                    } else {
                        "No kask skills."
                    }
                }
                ExtensionFilter::Installed => {
                    if has_search {
                        "No installed kask skills that match your search."
                    } else {
                        "No installed kask skills."
                    }
                }
                ExtensionFilter::NotInstalled => {
                    if has_search {
                        "No not installed kask skills that match your search."
                    } else {
                        "No not installed kask skills."
                    }
                }
            }
        };

        h_flex()
            .py_4()
            .gap_1p5()
            .when(self.fetch_failed, |this| {
                this.child(
                    Icon::new(IconName::Warning)
                        .size(IconSize::Small)
                        .color(Color::Warning),
                )
            })
            .child(Label::new(message))
    }
}

impl Render for KaskExtensionsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .child(
                v_flex()
                    .gap_4()
                    .pt_4()
                    .px_4()
                    .bg(cx.theme().colors().editor_background)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .justify_between()
                            .child(Headline::new("Kask Extensions").size(HeadlineSize::Large)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .gap_2()
                            .child(self.render_search(cx))
                            .child(
                                div().child(
                                    ToggleButtonGroup::single_row(
                                        "filter-buttons",
                                        [
                                            ToggleButtonSimple::new(
                                                "All",
                                                cx.listener(|this, _event, _, cx| {
                                                    this.filter = ExtensionFilter::All;
                                                    this.filter_extension_entries(cx);
                                                    this.scroll_to_top(cx);
                                                }),
                                            ),
                                            ToggleButtonSimple::new(
                                                "Installed",
                                                cx.listener(|this, _event, _, cx| {
                                                    this.filter = ExtensionFilter::Installed;
                                                    this.filter_extension_entries(cx);
                                                    this.scroll_to_top(cx);
                                                }),
                                            ),
                                            ToggleButtonSimple::new(
                                                "Not Installed",
                                                cx.listener(|this, _event, _, cx| {
                                                    this.filter = ExtensionFilter::NotInstalled;
                                                    this.filter_extension_entries(cx);
                                                    this.scroll_to_top(cx);
                                                }),
                                            ),
                                        ],
                                    )
                                    .style(ToggleButtonGroupStyle::Outlined)
                                    .size(ToggleButtonGroupSize::Custom(rems_from_px(30.))) // Perfectly matches the input
                                    .label_size(LabelSize::Default)
                                    .auto_width()
                                    .selected_index(match self.filter {
                                        ExtensionFilter::All => 0,
                                        ExtensionFilter::Installed => 1,
                                        ExtensionFilter::NotInstalled => 2,
                                    })
                                    .into_any_element(),
                                ),
                            ),
                    ),
            )
            // zed-kask: provides filter row removed — kask skills have no
            // provides concept (v1 is skills-only per plan §0). Extension
            // upsell banners are also irrelevant to kask skills and removed.
            .child(v_flex().px_4().size_full().overflow_y_hidden().map(|this| {
                // zed-kask: count is just the filtered skill entries.
                let count = self.filtered_remote_skill_indices.len();

                if count == 0 {
                    this.child(self.render_empty_state(cx)).into_any_element()
                } else {
                    let scroll_handle = &self.list;
                    this.child(
                        uniform_list("entries", count, cx.processor(Self::render_extensions))
                            .flex_grow_1()
                            .pb_4()
                            .track_scroll(scroll_handle),
                    )
                    .vertical_scrollbar_for(scroll_handle, window, cx)
                    .into_any_element()
                }
            }))
    }
}

impl EventEmitter<ItemEvent> for KaskExtensionsPage {}

impl Focusable for KaskExtensionsPage {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.query_editor.read(cx).focus_handle(cx)
    }
}

impl Item for KaskExtensionsPage {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Kask Extensions".into()
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Kask Extensions Page Opened")
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        // zed-kask: per the extensions-panel plan §3 step 4, reuse the kask
        // logo icon for visual consistency with the kask panel tab.
        Some(Icon::new(IconName::Kask).color(Color::Muted))
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(workspace::item::ItemEvent)) {
        f(*event)
    }
}
