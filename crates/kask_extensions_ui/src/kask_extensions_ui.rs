mod components;
mod panel_button;
mod publish;

pub use panel_button::KaskExtensionsButton;
pub use publish::{generate_version, install_skill, publish_skill, unpublish_skill, vote_skill};

use std::sync::OnceLock;
use std::time::Duration;
use std::{any::TypeId, ops::Range, sync::Arc};

use anyhow::Context as _;
use cloud_api_types::{
    ExtensionMetadata, ExtensionProvides, GetKaskSkillsResponse, KaskSkillMetadata,
};
use collections::{BTreeMap, BTreeSet};
use command_palette_hooks::CommandPaletteFilter;
use editor::{Editor, EditorElement, EditorStyle};
use extension_host::{ExtensionManifest, ExtensionOperation, ExtensionStore};
use fuzzy::{StringMatch, StringMatchCandidate, match_strings};
use gpui::{
    Action, Anchor, App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, Focusable,
    InteractiveElement, KeyContext, ParentElement, Point, Render, Styled, Task, TaskExt, TextStyle,
    UniformListScrollHandle, WeakEntity, Window, actions, point, uniform_list,
};
use num_format::{Locale, ToFormattedString};
use picker::{Picker, PickerDelegate};
use project::DirectoryLister;
use release_channel::ReleaseChannel;
use schemars::JsonSchema;
use serde::Deserialize;
use settings::{Settings, SettingsContent};
#[allow(unused_imports)]
use strum::IntoEnumIterator as _;
use theme_settings::ThemeSettings;
use ui::{
    Banner, Chip, ContextMenu, Divider, ListItem, ListItemSpacing, PopoverMenu, ScrollableHandle,
    Switch, ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple,
    Tooltip, WithScrollbar, prelude::*,
};
use util::ResultExt;
use vim_mode_setting::VimModeSetting;
use workspace::{
    Workspace,
    item::{Item, ItemEvent},
    workspace_error::{ErrorAction, ErrorSeverity, WorkspaceError},
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
    cx.observe_new(move |workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
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
                        Box::new(extensions_page),
                        None,
                        true,
                        window,
                        cx,
                    )
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

#[allow(dead_code)]
// zed-kask: ExtensionStatus is kept as a type alias for the dead extension
// render code that hasn't been removed yet. The new kask render path uses
// KaskSkillStatus. This will be removed when the dead code is cleaned up.
type ExtensionStatus = KaskSkillStatus;

#[allow(dead_code)]
fn extension_provides_label(provides: ExtensionProvides) -> &'static str {
    match provides {
        ExtensionProvides::Themes => "Themes",
        ExtensionProvides::IconThemes => "Icon Themes",
        ExtensionProvides::Languages => "Languages",
        ExtensionProvides::Grammars => "Grammars",
        ExtensionProvides::LanguageServers => "Language Servers",
        ExtensionProvides::ContextServers => "MCP Servers",
        ExtensionProvides::AgentServers => "Agent Servers",
        ExtensionProvides::SlashCommands => "Slash Commands",
        ExtensionProvides::IndexedDocsProviders => "Indexed Docs Providers",
        ExtensionProvides::Snippets => "Snippets",
        ExtensionProvides::DebugAdapters => "Debug Adapters",
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum ExtensionFilter {
    All,
    Installed,
    NotInstalled,
}

impl ExtensionFilter {
    #[allow(dead_code)]
    pub fn include_dev_extensions(&self) -> bool {
        match self {
            Self::All | Self::Installed => true,
            Self::NotInstalled => false,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum Feature {
    AgentClaude,
    AgentCodex,
    AgentGemini,
    ExtensionBasedpyright,
    ExtensionRuff,
    ExtensionTailwind,
    ExtensionTy,
    Git,
    LanguageBash,
    LanguageC,
    LanguageCpp,
    LanguageGo,
    LanguagePython,
    LanguageReact,
    LanguageRust,
    LanguageTypescript,
    OpenIn,
    Vim,
}

fn keywords_by_feature() -> &'static BTreeMap<Feature, Vec<&'static str>> {
    static KEYWORDS_BY_FEATURE: OnceLock<BTreeMap<Feature, Vec<&'static str>>> = OnceLock::new();
    KEYWORDS_BY_FEATURE.get_or_init(|| {
        BTreeMap::from_iter([
            (
                Feature::AgentClaude,
                vec!["claude", "claude code", "claude agent"],
            ),
            (Feature::AgentCodex, vec!["codex", "codex cli"]),
            (Feature::AgentGemini, vec!["gemini", "gemini cli"]),
            (
                Feature::ExtensionBasedpyright,
                vec!["basedpyright", "pyright"],
            ),
            (Feature::ExtensionRuff, vec!["ruff"]),
            (Feature::ExtensionTailwind, vec!["tail", "tailwind"]),
            (Feature::ExtensionTy, vec!["ty"]),
            (Feature::Git, vec!["git"]),
            (Feature::LanguageBash, vec!["sh", "bash"]),
            (Feature::LanguageC, vec!["c", "clang"]),
            (Feature::LanguageCpp, vec!["c++", "cpp", "clang"]),
            (Feature::LanguageGo, vec!["go", "golang"]),
            (Feature::LanguagePython, vec!["python", "py"]),
            (Feature::LanguageReact, vec!["react"]),
            (Feature::LanguageRust, vec!["rust", "rs"]),
            (
                Feature::LanguageTypescript,
                vec!["type", "typescript", "ts"],
            ),
            (
                Feature::OpenIn,
                vec![
                    "github",
                    "gitlab",
                    "bitbucket",
                    "codeberg",
                    "sourcehut",
                    "permalink",
                    "link",
                    "open in",
                ],
            ),
            (Feature::Vim, vec!["vim"]),
        ])
    })
}

#[allow(dead_code)]
fn extension_button_id(extension_id: &Arc<str>, operation: ExtensionOperation) -> ElementId {
    (SharedString::from(extension_id.clone()), operation as usize).into()
}

#[allow(dead_code)]
struct ExtensionCardButtons {
    install_or_uninstall: Button,
    upgrade: Option<Button>,
    configure: Option<Button>,
}

pub struct KaskExtensionsPage {
    workspace: WeakEntity<Workspace>,
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
    upsells: BTreeSet<Feature>,
}

#[allow(dead_code)]
impl KaskExtensionsPage {
    pub fn new(
        workspace: &Workspace,
        _provides_filter: Option<ExtensionProvides>,
        _focus_skill_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let _workspace_handle = workspace.weak_handle();
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
                workspace: workspace.weak_handle(),
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
                upsells: BTreeSet::default(),
                query_editor,
            };
            this.fetch_kask_skills(cx);
            this
        })
    }

    fn on_extension_installed(
        &mut self,
        workspace: WeakEntity<Workspace>,
        extension_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let extension_store = ExtensionStore::global(cx).read(cx);
        let themes = extension_store
            .extension_themes(extension_id)
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        if !themes.is_empty() {
            workspace
                .update(cx, |_workspace, cx| {
                    window.dispatch_action(
                        zed_actions::theme_selector::Toggle {
                            themes_filter: Some(themes),
                        }
                        .boxed_clone(),
                        cx,
                    );
                })
                .ok();
            return;
        }

        let icon_themes = extension_store
            .extension_icon_themes(extension_id)
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        if !icon_themes.is_empty() {
            workspace
                .update(cx, |_workspace, cx| {
                    window.dispatch_action(
                        zed_actions::icon_theme_selector::Toggle {
                            themes_filter: Some(icon_themes),
                        }
                        .boxed_clone(),
                        cx,
                    );
                })
                .ok();
        }
    }

    /// Returns whether a dev extension currently exists for the extension with the given ID.
    fn dev_extension_exists(extension_id: &str, cx: &mut Context<Self>) -> bool {
        let extension_store = ExtensionStore::global(cx).read(cx);

        extension_store
            .dev_extensions()
            .any(|dev_extension| dev_extension.id.as_ref() == extension_id)
    }

    fn extension_status(extension_id: &str, cx: &mut Context<Self>) -> ExtensionStatus {
        let extension_store = ExtensionStore::global(cx).read(cx);

        match extension_store.outstanding_operations().get(extension_id) {
            Some(ExtensionOperation::Install) => ExtensionStatus::Installing,
            Some(ExtensionOperation::Remove) => ExtensionStatus::Removing,
            Some(ExtensionOperation::Upgrade) => ExtensionStatus::Upgrading,
            None => match extension_store.installed_extensions().get(extension_id) {
                Some(extension) => ExtensionStatus::Installed(extension.manifest.version.clone()),
                None => ExtensionStatus::NotInstalled,
            },
        }
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

        let url = http_client.build_zed_api_url("/api/kask-skills", &[]);
        cx.spawn(async move |this, cx| {
            let result = async {
                let url = url?;
                let mut response = http_client
                    .get(url.as_ref(), http_client::AsyncBody::empty(), true)
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
        let marketplace_dir = util::paths::home_dir().join(".agents/skills/_marketplace");

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

        let install_dir = util::paths::home_dir()
            .join(".agents/skills/_marketplace")
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

    fn render_dev_extension(
        &self,
        extension: &ExtensionManifest,
        cx: &mut Context<Self>,
    ) -> ExtensionCard {
        let status = Self::extension_status(&extension.id, cx);

        let repository_url = extension.repository.clone();

        let can_configure = !extension.context_servers.is_empty();

        ExtensionCard::new()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_end()
                            .child(Headline::new(extension.name.clone()).size(HeadlineSize::Medium))
                            .child(
                                Headline::new(format!("v{}", extension.version))
                                    .size(HeadlineSize::XSmall),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .justify_between()
                            .child(
                                Button::new(
                                    SharedString::from(format!("rebuild-{}", extension.id)),
                                    "Rebuild",
                                )
                                .color(Color::Accent)
                                .disabled(matches!(status, ExtensionStatus::Upgrading))
                                .on_click({
                                    let extension_id = extension.id.clone();
                                    move |_, _, cx| {
                                        ExtensionStore::global(cx).update(cx, |store, cx| {
                                            store.rebuild_dev_extension(extension_id.clone(), cx)
                                        });
                                    }
                                }),
                            )
                            .child(
                                Button::new(extension_button_id(&extension.id, ExtensionOperation::Remove), "Uninstall")
                                    .color(Color::Accent)
                                    .disabled(matches!(status, ExtensionStatus::Removing))
                                    .on_click({
                                        let extension_id = extension.id.clone();
                                        move |_, _, cx| {
                                            ExtensionStore::global(cx).update(cx, |store, cx| {
                                                store.uninstall_extension(extension_id.clone(), cx).detach_and_log_err(cx);
                                            });
                                        }
                                    }),
                            )
                            .when(can_configure, |this| {
                                this.child(
                                    Button::new(
                                        SharedString::from(format!("configure-{}", extension.id)),
                                        "Configure",
                                    )
                                    .color(Color::Accent)
                                    .disabled(matches!(status, ExtensionStatus::Installing))
                                    .on_click({
                                        let manifest = Arc::new(extension.clone());
                                        move |_, _, cx| {
                                            if let Some(events) =
                                                extension::ExtensionEvents::try_global(cx)
                                            {
                                                events.update(cx, |this, cx| {
                                                    this.emit(
                                                        extension::Event::ConfigureExtensionRequested(
                                                            manifest.clone(),
                                                        ),
                                                        cx,
                                                    )
                                                });
                                            }
                                        }
                                    }),
                                )
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .child(
                        Label::new(format!(
                            "{}: {}",
                            if extension.authors.len() > 1 {
                                "Authors"
                            } else {
                                "Author"
                            },
                            extension.authors.join(", ")
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                    )
                    .child(Label::new("<>").size(LabelSize::Small)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .children(extension.description.as_ref().map(|description| {
                        Label::new(description.clone())
                            .size(LabelSize::Small)
                            .color(Color::Default)
                            .truncate()
                    }))
                    .children(repository_url.map(|repository_url| {
                        IconButton::new(
                            SharedString::from(format!("repository-{}", extension.id)),
                            IconName::Github,
                        )
                        .icon_color(Color::Accent)
                        .icon_size(IconSize::Small)
                        .on_click(cx.listener({
                            let repository_url = repository_url.clone();
                            move |_, _, _, cx| {
                                cx.open_url(&repository_url);
                            }
                        }))
                        .tooltip(Tooltip::text(repository_url))
                    })),
            )
    }

    fn render_remote_extension(
        &self,
        extension: &ExtensionMetadata,
        cx: &mut Context<Self>,
    ) -> ExtensionCard {
        let this = cx.weak_entity();
        let status = Self::extension_status(&extension.id, cx);
        let has_dev_extension = Self::dev_extension_exists(&extension.id, cx);

        let extension_id = extension.id.clone();
        let buttons = self.buttons_for_entry(extension, &status, has_dev_extension, cx);
        let version = extension.manifest.version.clone();
        let repository_url = extension.manifest.repository.clone();
        let authors = extension.manifest.authors.clone();

        let installed_version = match status {
            ExtensionStatus::Installed(installed_version) => Some(installed_version),
            _ => None,
        };

        ExtensionCard::new()
            .overridden_by_dev_extension(has_dev_extension)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Headline::new(extension.manifest.name.clone())
                                    .size(HeadlineSize::Small),
                            )
                            .child(Headline::new(format!("v{version}")).size(HeadlineSize::XSmall))
                            .children(
                                installed_version
                                    .filter(|installed_version| *installed_version != version)
                                    .map(|installed_version| {
                                        Headline::new(format!("(v{installed_version} installed)",))
                                            .size(HeadlineSize::XSmall)
                                    }),
                            )
                            .map(|parent| {
                                if extension.manifest.provides.is_empty() {
                                    return parent;
                                }

                                parent.child(
                                    h_flex().gap_1().children(
                                        extension
                                            .manifest
                                            .provides
                                            .iter()
                                            .filter_map(|provides| {
                                                match provides {
                                                    ExtensionProvides::AgentServers
                                                    | ExtensionProvides::SlashCommands
                                                    | ExtensionProvides::IndexedDocsProviders => {
                                                        return None;
                                                    }
                                                    _ => {}
                                                }

                                                Some(Chip::new(extension_provides_label(*provides)))
                                            })
                                            .collect::<Vec<_>>(),
                                    ),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .children(buttons.upgrade)
                            .children(buttons.configure)
                            .child(buttons.install_or_uninstall),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .children(extension.manifest.description.as_ref().map(|description| {
                        Label::new(description.clone())
                            .size(LabelSize::Small)
                            .color(Color::Default)
                            .truncate()
                    }))
                    .child(
                        Label::new(format!(
                            "Downloads: {}",
                            extension.download_count.to_formatted_string(&Locale::en)
                        ))
                        .size(LabelSize::Small),
                    ),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .w_full()
                    .justify_between()
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_1()
                            .child(
                                Icon::new(IconName::Person)
                                    .size(IconSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(extension.manifest.authors.join(", "))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .truncate(),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_shrink_0()
                            .child({
                                let repo_url_for_tooltip = repository_url.clone();

                                IconButton::new(
                                    SharedString::from(format!("repository-{}", extension.id)),
                                    IconName::Github,
                                )
                                .icon_size(IconSize::Small)
                                .tooltip(move |_, cx| {
                                    Tooltip::with_meta(
                                        "Visit Extension Repository",
                                        None,
                                        repo_url_for_tooltip.clone(),
                                        cx,
                                    )
                                })
                                .on_click(cx.listener(
                                    move |_, _, _, cx| {
                                        cx.open_url(&repository_url);
                                    },
                                ))
                            })
                            .child(
                                PopoverMenu::new(SharedString::from(format!(
                                    "more-{}",
                                    extension.id
                                )))
                                .trigger(
                                    IconButton::new(
                                        SharedString::from(format!("more-{}", extension.id)),
                                        IconName::Ellipsis,
                                    )
                                    .icon_size(IconSize::Small),
                                )
                                .anchor(Anchor::TopRight)
                                .offset(Point {
                                    x: px(0.0),
                                    y: px(2.0),
                                })
                                .menu(move |window, cx| {
                                    this.upgrade().map(|this| {
                                        Self::render_remote_extension_context_menu(
                                            &this,
                                            extension_id.clone(),
                                            authors.clone(),
                                            window,
                                            cx,
                                        )
                                    })
                                }),
                            ),
                    ),
            )
    }

    fn render_remote_extension_context_menu(
        this: &Entity<Self>,
        extension_id: Arc<str>,
        authors: Vec<String>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<ContextMenu> {
        ContextMenu::build(window, cx, |context_menu, window, _| {
            context_menu
                .entry(
                    "Install Another Version...",
                    None,
                    window.handler_for(this, {
                        let extension_id = extension_id.clone();
                        move |this, window, cx| {
                            this.show_extension_version_list(extension_id.clone(), window, cx)
                        }
                    }),
                )
                .entry("Copy Extension ID", None, {
                    let extension_id = extension_id.clone();
                    move |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(extension_id.to_string()));
                    }
                })
                .entry("Copy Author Info", None, {
                    let authors = authors.clone();
                    move |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(authors.join(", ")));
                    }
                })
        })
    }

    fn show_extension_version_list(
        &mut self,
        extension_id: Arc<str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };

        cx.spawn_in(window, async move |this, cx| {
            let extension_versions_task = this.update(cx, |_, cx| {
                let extension_store = ExtensionStore::global(cx);

                extension_store.update(cx, |store, cx| {
                    store.fetch_extension_versions(&extension_id, cx)
                })
            })?;

            let extension_versions = extension_versions_task.await?;

            workspace.update_in(cx, |_workspace, _window, _cx| {
                // zed-kask: version selector removed — kask skills only have
                // one version (the latest). This dead code path is kept for
                // structural compatibility with the fork.
            })?;

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn buttons_for_entry(
        &self,
        extension: &ExtensionMetadata,
        status: &ExtensionStatus,
        has_dev_extension: bool,
        cx: &mut Context<Self>,
    ) -> ExtensionCardButtons {
        let is_compatible =
            extension_host::is_version_compatible(ReleaseChannel::global(cx), extension);

        if has_dev_extension {
            // If we have a dev extension for the given extension, just treat it as uninstalled.
            // The button here is a placeholder, as it won't be interactable anyways.
            return ExtensionCardButtons {
                install_or_uninstall: Button::new(
                    extension_button_id(&extension.id, ExtensionOperation::Install),
                    "Install",
                ),
                configure: None,
                upgrade: None,
            };
        }

        let is_configurable = extension
            .manifest
            .provides
            .contains(&ExtensionProvides::ContextServers);

        match status.clone() {
            ExtensionStatus::NotInstalled => ExtensionCardButtons {
                install_or_uninstall: Button::new(
                    extension_button_id(&extension.id, ExtensionOperation::Install),
                    "Install",
                )
                .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                .start_icon(
                    Icon::new(IconName::Download)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
                .on_click({
                    let extension_id = extension.id.clone();
                    move |_, _, cx| {
                        telemetry::event!("Extension Installed");
                        ExtensionStore::global(cx).update(cx, |store, cx| {
                            store.install_latest_extension(extension_id.clone(), cx)
                        });
                    }
                }),
                configure: None,
                upgrade: None,
            },
            ExtensionStatus::Installing => ExtensionCardButtons {
                install_or_uninstall: Button::new(
                    extension_button_id(&extension.id, ExtensionOperation::Install),
                    "Install",
                )
                .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                .start_icon(
                    Icon::new(IconName::Download)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
                .disabled(true),
                configure: None,
                upgrade: None,
            },
            ExtensionStatus::Upgrading => ExtensionCardButtons {
                install_or_uninstall: Button::new(
                    extension_button_id(&extension.id, ExtensionOperation::Remove),
                    "Uninstall",
                )
                .style(ButtonStyle::OutlinedGhost)
                .disabled(true),
                configure: is_configurable.then(|| {
                    Button::new(
                        SharedString::from(format!("configure-{}", extension.id)),
                        "Configure",
                    )
                    .disabled(true)
                }),
                upgrade: Some(
                    Button::new(
                        extension_button_id(&extension.id, ExtensionOperation::Upgrade),
                        "Upgrade",
                    )
                    .disabled(true),
                ),
            },
            ExtensionStatus::Installed(installed_version) => ExtensionCardButtons {
                install_or_uninstall: Button::new(
                    extension_button_id(&extension.id, ExtensionOperation::Remove),
                    "Uninstall",
                )
                .style(ButtonStyle::OutlinedGhost)
                .on_click({
                    let extension_id = extension.id.clone();
                    move |_, _, cx| {
                        telemetry::event!("Extension Uninstalled", extension_id);
                        ExtensionStore::global(cx).update(cx, |store, cx| {
                            store
                                .uninstall_extension(extension_id.clone(), cx)
                                .detach_and_log_err(cx);
                        });
                    }
                }),
                configure: is_configurable.then(|| {
                    Button::new(
                        SharedString::from(format!("configure-{}", extension.id)),
                        "Configure",
                    )
                    .style(ButtonStyle::OutlinedGhost)
                    .on_click({
                        let extension_id = extension.id.clone();
                        move |_, _, cx| {
                            if let Some(manifest) = ExtensionStore::global(cx)
                                .read(cx)
                                .extension_manifest_for_id(&extension_id)
                                .cloned()
                                && let Some(events) = extension::ExtensionEvents::try_global(cx)
                            {
                                events.update(cx, |this, cx| {
                                    this.emit(
                                        extension::Event::ConfigureExtensionRequested(manifest),
                                        cx,
                                    )
                                });
                            }
                        }
                    })
                }),
                upgrade: if installed_version == extension.manifest.version {
                    None
                } else {
                    Some(
                        Button::new(extension_button_id(&extension.id, ExtensionOperation::Upgrade), "Upgrade")
                          .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                            .when(!is_compatible, |upgrade_button| {
                                upgrade_button.disabled(true).tooltip({
                                    let version = extension.manifest.version.clone();
                                    move |_, cx| {
                                        Tooltip::simple(
                                            format!(
                                                "v{version} is not compatible with this version of Zed.",
                                            ),
                                             cx,
                                        )
                                    }
                                })
                            })
                            .disabled(!is_compatible)
                            .on_click({
                                let extension_id = extension.id.clone();
                                let version = extension.manifest.version.clone();
                                move |_, _, cx| {
                                    telemetry::event!("Extension Installed", extension_id, version);
                                    ExtensionStore::global(cx).update(cx, |store, cx| {
                                        store
                                            .upgrade_extension(
                                                extension_id.clone(),
                                                version.clone(),
                                                cx,
                                            )
                                            .detach_and_log_err(cx)
                                    });
                                }
                            }),
                    )
                },
            },
            ExtensionStatus::Removing => ExtensionCardButtons {
                install_or_uninstall: Button::new(
                    extension_button_id(&extension.id, ExtensionOperation::Remove),
                    "Uninstall",
                )
                .style(ButtonStyle::OutlinedGhost)
                .disabled(true),
                configure: is_configurable.then(|| {
                    Button::new(
                        SharedString::from(format!("configure-{}", extension.id)),
                        "Configure",
                    )
                    .disabled(true)
                }),
                upgrade: None,
            },
        }
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
        self.refresh_feature_upsells(cx);
    }

    pub fn focus_extension(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.query_editor.update(cx, |editor, cx| {
            editor.set_text(format!("id:{id}"), window, cx)
        });
        self.refresh_search(cx);
    }

    // zed-kask: provides_filter is extension-specific; kask skills have no
    // provides concept. This method is kept as a no-op for API compatibility
    // with any callers that still reference it.
    pub fn change_provides_filter(
        &mut self,
        _provides_filter: Option<ExtensionProvides>,
        cx: &mut Context<Self>,
    ) {
        self.refresh_search(cx);
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
            "Loading extensions…"
        } else if self.fetch_failed {
            "Failed to load extensions. Please check your connection and try again."
        } else {
            match self.filter {
                ExtensionFilter::All => {
                    if has_search {
                        "No extensions that match your search."
                    } else {
                        "No extensions."
                    }
                }
                ExtensionFilter::Installed => {
                    if has_search {
                        "No installed extensions that match your search."
                    } else {
                        "No installed extensions."
                    }
                }
                ExtensionFilter::NotInstalled => {
                    if has_search {
                        "No not installed extensions that match your search."
                    } else {
                        "No not installed extensions."
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

    fn update_settings(
        &mut self,
        selection: &ToggleState,

        cx: &mut Context<Self>,
        callback: impl 'static + Send + Fn(&mut SettingsContent, bool),
    ) {
        if let Some(workspace) = self.workspace.upgrade() {
            let fs = workspace.read(cx).app_state().fs.clone();
            let selection = *selection;
            settings::update_settings_file(fs, cx, move |settings, _| {
                let value = match selection {
                    ToggleState::Unselected => false,
                    ToggleState::Selected => true,
                    _ => return,
                };

                callback(settings, value)
            });
        }
    }

    fn refresh_feature_upsells(&mut self, cx: &mut Context<Self>) {
        let Some(search) = self.search_query(cx) else {
            self.upsells.clear();
            return;
        };

        if let Some(id) = search.strip_prefix("id:") {
            self.upsells.clear();

            let upsell = match id.to_lowercase().as_str() {
                "ruff" => Some(Feature::ExtensionRuff),
                "basedpyright" => Some(Feature::ExtensionBasedpyright),
                "ty" => Some(Feature::ExtensionTy),
                _ => None,
            };

            if let Some(upsell) = upsell {
                self.upsells.insert(upsell);
            }

            return;
        }

        let search = search.to_lowercase();
        let search_terms = search
            .split_whitespace()
            .map(|term| term.trim())
            .collect::<Vec<_>>();

        for (feature, keywords) in keywords_by_feature() {
            if keywords
                .iter()
                .any(|keyword| search_terms.contains(keyword))
            {
                self.upsells.insert(*feature);
            } else {
                self.upsells.remove(feature);
            }
        }
    }

    fn render_feature_upsell_banner(
        &self,
        label: SharedString,
        docs_url: SharedString,
        vim: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let docs_url_button = Button::new("open_docs", "View Documentation")
            .end_icon(Icon::new(IconName::ArrowUpRight).size(IconSize::Small))
            .on_click({
                move |_event, _window, cx| {
                    telemetry::event!(
                        "Documentation Viewed",
                        source = "Feature Upsell",
                        url = docs_url,
                    );
                    cx.open_url(&docs_url)
                }
            });

        div()
            .pt_4()
            .px_4()
            .child(
                Banner::new()
                    .severity(Severity::Success)
                    .child(Label::new(label).mt_0p5())
                    .map(|this| {
                        if vim {
                            this.action_slot(
                                h_flex()
                                    .gap_1()
                                    .child(docs_url_button)
                                    .child(Divider::vertical().color(ui::DividerColor::Border))
                                    .child(
                                        h_flex()
                                            .pl_1()
                                            .gap_1()
                                            .child(Label::new("Enable Vim mode"))
                                            .child(
                                                Switch::new(
                                                    "enable-vim",
                                                    if VimModeSetting::get_global(cx).0 {
                                                        ui::ToggleState::Selected
                                                    } else {
                                                        ui::ToggleState::Unselected
                                                    },
                                                )
                                                .on_click(cx.listener(
                                                    move |this, selection, _, cx| {
                                                        telemetry::event!(
                                                            "Vim Mode Toggled",
                                                            source = "Feature Upsell"
                                                        );
                                                        this.update_settings(
                                                            selection,
                                                            cx,
                                                            |setting, value| {
                                                                setting.vim_mode = Some(value)
                                                            },
                                                        );
                                                    },
                                                )),
                                            ),
                                    ),
                            )
                        } else {
                            this.action_slot(docs_url_button)
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_feature_upsells(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut container = v_flex();

        for feature in &self.upsells {
            let banner = match feature {
                Feature::AgentClaude => self.render_feature_upsell_banner(
                    "Claude Agent support is built-in to Zed!".into(),
                    "https://zed.dev/docs/ai/external-agents#claude-agent".into(),
                    false,
                    cx,
                ),
                Feature::AgentCodex => self.render_feature_upsell_banner(
                    "Codex CLI support is built-in to Zed!".into(),
                    "https://zed.dev/docs/ai/external-agents#codex-cli".into(),
                    false,
                    cx,
                ),
                Feature::AgentGemini => self.render_feature_upsell_banner(
                    "Gemini CLI support is built-in to Zed!".into(),
                    "https://zed.dev/docs/ai/external-agents#gemini-cli".into(),
                    false,
                    cx,
                ),
                Feature::ExtensionBasedpyright => self.render_feature_upsell_banner(
                    "Basedpyright (Python language server) support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/python#basedpyright".into(),
                    false,
                    cx,
                ),
                Feature::ExtensionRuff => self.render_feature_upsell_banner(
                    "Ruff (linter for Python) support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/python#code-formatting--linting".into(),
                    false,
                    cx,
                ),
                Feature::ExtensionTailwind => self.render_feature_upsell_banner(
                    "Tailwind CSS support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/tailwindcss".into(),
                    false,
                    cx,
                ),
                Feature::ExtensionTy => self.render_feature_upsell_banner(
                    "Ty (Python language server) support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/python".into(),
                    false,
                    cx,
                ),
                Feature::Git => self.render_feature_upsell_banner(
                    "Zed comes with basic Git support—more features are coming in the future."
                        .into(),
                    "https://zed.dev/docs/git".into(),
                    false,
                    cx,
                ),
                Feature::LanguageBash => self.render_feature_upsell_banner(
                    "Shell support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/bash".into(),
                    false,
                    cx,
                ),
                Feature::LanguageC => self.render_feature_upsell_banner(
                    "C support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/c".into(),
                    false,
                    cx,
                ),
                Feature::LanguageCpp => self.render_feature_upsell_banner(
                    "C++ support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/cpp".into(),
                    false,
                    cx,
                ),
                Feature::LanguageGo => self.render_feature_upsell_banner(
                    "Go support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/go".into(),
                    false,
                    cx,
                ),
                Feature::LanguagePython => self.render_feature_upsell_banner(
                    "Python support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/python".into(),
                    false,
                    cx,
                ),
                Feature::LanguageReact => self.render_feature_upsell_banner(
                    "React support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/typescript".into(),
                    false,
                    cx,
                ),
                Feature::LanguageRust => self.render_feature_upsell_banner(
                    "Rust support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/rust".into(),
                    false,
                    cx,
                ),
                Feature::LanguageTypescript => self.render_feature_upsell_banner(
                    "Typescript support is built-in to Zed!".into(),
                    "https://zed.dev/docs/languages/typescript".into(),
                    false,
                    cx,
                ),
                Feature::OpenIn => self.render_feature_upsell_banner(
                    "Zed supports linking to a source line on GitHub and others.".into(),
                    "https://zed.dev/docs/git#git-integrations".into(),
                    false,
                    cx,
                ),
                Feature::Vim => self.render_feature_upsell_banner(
                    "Vim support is built-in to Zed!".into(),
                    "https://zed.dev/docs/vim".into(),
                    true,
                    cx,
                ),
            };
            container = container.child(banner);
        }

        container
    }
}

struct DevExtensionRebuildPickerDelegate {
    entries: Vec<Arc<ExtensionManifest>>,
    matches: Vec<StringMatch>,
    selected_index: usize,
}

impl DevExtensionRebuildPickerDelegate {
    fn new(manifests: Vec<Arc<ExtensionManifest>>) -> Self {
        let matches = manifests
            .iter()
            .enumerate()
            .map(|(ix, manifest)| StringMatch {
                candidate_id: ix,
                score: 0.0,
                positions: Vec::new(),
                string: manifest.name.clone(),
            })
            .collect();

        Self {
            entries: manifests,
            matches,
            selected_index: 0,
        }
    }
}

impl PickerDelegate for DevExtensionRebuildPickerDelegate {
    type ListItem = ListItem;

    fn name() -> &'static str {
        "dev-extension-rebuild"
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn selected_index_changed(
        &self,
        _ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Box<dyn Fn(&mut Window, &mut App) + 'static>> {
        None
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let background = cx.background_executor().clone();
        let candidates = self
            .entries
            .iter()
            .enumerate()
            .map(|(ix, manifest)| StringMatchCandidate::new(ix, manifest.name.as_ref()))
            .collect::<Vec<_>>();

        cx.spawn_in(window, async move |this, cx| {
            let matches = if query.is_empty() {
                candidates
                    .into_iter()
                    .enumerate()
                    .map(|(index, candidate)| StringMatch {
                        candidate_id: index,
                        string: candidate.string,
                        positions: Vec::new(),
                        score: 0.0,
                    })
                    .collect()
            } else {
                match_strings(
                    &candidates,
                    &query,
                    false,
                    true,
                    100,
                    &Default::default(),
                    background,
                )
                .await
            };

            this.update(cx, |this, _cx| {
                this.delegate.matches = matches;
                this.delegate.selected_index = this
                    .delegate
                    .selected_index
                    .min(this.delegate.matches.len().saturating_sub(1));
            })
            .log_err();
        })
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(mat) = self.matches.get(self.selected_index) else {
            return;
        };

        let extension_id = self.entries[mat.candidate_id].id.clone();
        ExtensionStore::global(cx).update(cx, |store, cx| {
            store.rebuild_dev_extension(extension_id, cx);
        });

        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        Arc::from("Rebuild dev extension…")
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let mat = self.matches.get(ix)?;
        let entry = self.entries.get(mat.candidate_id)?;

        let item = ListItem::new(("dev-extension-list-item", mat.candidate_id))
            .inset(true)
            .spacing(ListItemSpacing::Sparse)
            .toggle_state(selected)
            .child(
                h_flex()
                    .w_full()
                    .py_px()
                    .justify_between()
                    .gap_2()
                    .child(Label::new(entry.name.clone()))
                    .child(
                        Label::new(format!("{} • v{}", entry.id, entry.version))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            );

        Some(item)
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some("No dev extensions found".into())
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
            // provides concept (v1 is skills-only per plan §0).
            .child(self.render_feature_upsells(cx))
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
        "Extensions".into()
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Extensions Page Opened")
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(workspace::item::ItemEvent)) {
        f(*event)
    }
}
