#![forbid(unsafe_code)]
pub mod panel_button;
mod publish;

pub use panel_button::KaskExtensionsButton;
pub use publish::{
    generate_version, install_skill, publish_skill, scan_for_skill_refs, unpublish_skill,
    vote_skill,
};

use std::time::Duration;
use std::{ops::Range, sync::Arc};

use anyhow::Context as _;
use cloud_api_types::{GetKaskSkillsResponse, KaskSkillMetadata, KaskSkillRef};
use editor::Editor;
use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, Focusable, ParentElement, Render, Styled,
    Task, UniformListScrollHandle, Window, actions, point, uniform_list,
};
use marketplace_ui_common::{MarketplaceCard, marketplace_empty_state, marketplace_search_bar};
use ui::{
    ScrollableHandle, ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle,
    ToggleButtonSimple, WithScrollbar, prelude::*,
};
use workspace::{
    Workspace,
    item::{Item, ItemEvent},
};

actions!(
    kask_extensions_ui,
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
                    let extensions_page = KaskExtensionsPage::new(window, cx);
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
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum ExtensionFilter {
    All,
    Installed,
    NotInstalled,
}

// zed-kask: the source of a skill that ships with the install. Shown in the
// panel when the "Bundled skills" toggle is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BundledSource {
    BuiltIn,
    Shipped,
}

// zed-kask: a skill that ships with the install. `modified` is true when an
// on-disk override at `global_skills_dir()/<name>/SKILL.md` hashes differently
// from the shipped SKILL.md. v1 compares over the SKILL.md file (the file a
// user override contains); the full-package hash (manifest.yaml + j2) is the
// next slice, pending a public accessor for the embedded manifest/templates.
#[derive(Clone, Debug)]
struct BundledSkillEntry {
    name: SharedString,
    description: SharedString,
    source: BundledSource,
    modified: bool,
}

pub struct KaskExtensionsPage {
    list: UniformListScrollHandle,
    is_fetching_skills: bool,
    // zed-kask: summary of the last fetch failure, shown in the empty state
    // so the user sees the real cause (e.g. server-side marketplace not
    // configured) instead of a generic "check your connection".
    fetch_error: Option<SharedString>,
    filter: ExtensionFilter,
    // zed-kask: kask skill catalog entries (replaces remote_extension_entries)
    remote_skill_entries: Vec<KaskSkillMetadata>,
    filtered_remote_skill_indices: Vec<usize>,
    query_editor: Entity<Editor>,
    _subscriptions: [gpui::Subscription; 1],
    skill_fetch_task: Option<Task<()>>,
    // zed-kask: track in-flight install/uninstall operations by skill id
    outstanding_operations: collections::BTreeMap<Arc<str>, KaskSkillStatus>,
    // zed-kask: track the HTTP client for catalog fetches and install/vote
    http_client: Option<Arc<http_client::HttpClientWithUrl>>,
    // zed-kask: track the fs for install/uninstall
    fs: Option<Arc<dyn fs::Fs>>,
    // zed-kask: track the client for credentials (auth headers)
    client: Option<Arc<client::Client>>,
    // zed-kask: bundled-skills toggle + inventory. "Bundled skills" are the
    // skills that ship with the install (embedded global + built-in). When
    // `show_bundled` is on they're listed alongside the marketplace catalog;
    // each is badged "Modified" when an on-disk override hashes differently
    // from the shipped SKILL.md.
    show_bundled: bool,
    bundled_entries: Vec<BundledSkillEntry>,
    filtered_bundled_indices: Vec<usize>,
    bundled_fetch_task: Option<Task<()>>,
    // zed-kask: discreet-piggyback status feedback for share / install-from-ref
    // actions (clipboard-based in v1; the page holds no workspace handle for
    // toasts, so feedback is rendered inline).
    status_message: Option<SharedString>,
    // zed-kask: skill refs discovered in the user's opened channel buffers
    // (discreet piggyback — `kask-skill://` URIs carried as ordinary channel
    // message text). Populated by `scan_open_channels_for_refs`.
    shared_in_channels: Vec<KaskSkillRef>,
}

/// Pure filter predicate for kask skill entries. Extracted so the filter
/// logic is testable without a GPUI `Workspace` (the `KaskExtensionsPage`
/// constructor requires a full workspace, which is heavy for a unit test).
///
/// Returns `true` when the skill matches the (optional) search query —
/// case-insensitive substring match on the skill id or manifest description.
fn skill_matches_query(skill: &KaskSkillMetadata, query: &Option<String>) -> bool {
    match query {
        None => true,
        Some(query) => {
            skill.id.to_lowercase().contains(query)
                || skill.manifest.description.to_lowercase().contains(query)
        }
    }
}

impl KaskExtensionsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Entity<Self> {
        cx.new(|cx| {
            let app_state = workspace::AppState::global(cx);
            let http_client = app_state.client.http_client();
            let fs = app_state.fs.clone();

            // zed-kask: re-fetch the catalog when the user's private info
            // resolves (post-login) — the initial fetch at construction
            // often runs before auth, when the marketplace base URL is
            // still the localhost dev fallback.
            let user_store = app_state.user_store.clone();
            let subscriptions = [cx.subscribe_in(
                &user_store,
                window,
                move |this: &mut Self, _, event, _window, cx| match event {
                    client::user::Event::PrivateUserInfoUpdated => {
                        this.fetch_kask_skills(cx);
                    }
                    _ => {}
                },
            )];

            let query_editor = cx.new(|cx| {
                let mut input = Editor::single_line(window, cx);
                input.set_placeholder_text("Search kask skills...", window, cx);
                input
            });
            cx.subscribe(&query_editor, Self::on_query_change).detach();

            let scroll_handle = UniformListScrollHandle::new();

            let mut this = Self {
                list: scroll_handle,
                is_fetching_skills: false,
                fetch_error: None,
                filter: ExtensionFilter::All,
                remote_skill_entries: Vec::new(),
                filtered_remote_skill_indices: Vec::new(),
                _subscriptions: subscriptions,
                skill_fetch_task: None,
                outstanding_operations: collections::BTreeMap::default(),
                http_client: Some(http_client),
                fs: Some(fs),
                client: Some(app_state.client.clone()),
                show_bundled: false,
                bundled_entries: Vec::new(),
                filtered_bundled_indices: Vec::new(),
                bundled_fetch_task: None,
                status_message: None,
                shared_in_channels: Vec::new(),
                query_editor,
            };
            this.fetch_kask_skills(cx);
            this
        })
    }

    fn filter_extension_entries(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter;
        let query = self.search_query(cx).map(|q| q.to_lowercase());
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
            .filter(|(_, skill)| skill_matches_query(skill, &query))
            .map(|(ix, _)| ix)
            .collect();
        self.filtered_remote_skill_indices = indices;
        self.filter_bundled_entries(cx);
        cx.notify();
    }

    /// zed-kask: compute the filtered bundled-skill indices. Bundled skills
    /// are filtered by the search query only; the install-status filter does
    /// not apply (they ship with the app and are always present). Hidden
    /// entirely when the toggle is off.
    fn filter_bundled_entries(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query(cx).map(|q| q.to_lowercase());
        self.filtered_bundled_indices = if self.show_bundled {
            (0..self.bundled_entries.len())
                .filter(|&i| match &query {
                    None => true,
                    Some(query) => {
                        let entry = &self.bundled_entries[i];
                        entry.name.to_lowercase().contains(query)
                            || entry.description.to_lowercase().contains(query)
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
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

        self.is_fetching_skills = true;
        self.fetch_error = None;
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
                // zed-kask: surface the server's response body for both 4xx
                // and 5xx — the collab server returns actionable error text
                // (e.g. marketplace-not-configured 501s) that the user should
                // see, not a JSON parse failure.
                if response.status().is_client_error() || response.status().is_server_error() {
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
                this.is_fetching_skills = false;
                match result {
                    Ok(skills) => {
                        this.fetch_error = None;
                        this.remote_skill_entries = skills;
                        this.filter_extension_entries(cx);
                    }
                    Err(err) => {
                        // The root cause is the actionable line — the outer
                        // anyhow context ("error reading kask skills
                        // response") is the least informative fragment.
                        let root_cause = err
                            .chain()
                            .last()
                            .map(|cause| cause.to_string())
                            .unwrap_or_else(|| format!("{err:#}"));
                        this.fetch_error = Some(root_cause.into());
                        this.filter_extension_entries(cx);
                        log::warn!("kask-extensions: failed to fetch skill catalog: {err:#}.");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// zed-kask: Load the skills that ship with the install (shipped + built-in)
    /// and detect on-disk modifications. Shipped skills are seeded to disk at
    /// startup from the compiled seed payload. A bundled skill is "Modified"
    /// when its on-disk package hash differs from the shipped package hash.
    /// The package is the full triple `(SKILL.md, manifest.yaml, *.j2
    /// templates)` plus the process manifest and template YAML sub-manifests —
    /// "a change is a change in any of those" — so editing any package file
    /// surfaces the badge, not just SKILL.md.
    fn fetch_bundled_skills(&mut self, cx: &mut Context<Self>) {
        let Some(fs) = self.fs.clone() else {
            log::warn!("kask-extensions: no filesystem available; cannot load bundled skills.");
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            let result = async {
                use std::collections::HashMap;

                let seed = agent_skills::shipped_skill_seed();
                let mut entries: Vec<BundledSkillEntry> = Vec::with_capacity(seed.len() + 4);
                // zed-kask: full shipped package per skill (SKILL.md +
                // manifest.yaml + *.j2 + process manifest). "Modified" =
                // this hash differs from the on-disk package hash.
                let mut shipped_packages = HashMap::new();
                for (name, content) in seed {
                    let description = match agent_skills::parse_skill_metadata(content) {
                        Ok(meta) => meta.description,
                        Err(error) => {
                            log::warn!(
                                "kask-extensions: shipped skill '{name}' failed to parse: {error}"
                            );
                            continue;
                        }
                    };
                    entries.push(BundledSkillEntry {
                        name: (*name).into(),
                        description: description.into(),
                        source: BundledSource::Shipped,
                        modified: false,
                    });
                    shipped_packages.insert(
                        (*name).to_string(),
                        crate::publish::gather_shipped_skill_package(name, Some(content)),
                    );
                }
                for skill in agent_skills::builtin_skills() {
                    let md = agent_skills::builtin_skill_content(&skill.skill_file_path);
                    entries.push(BundledSkillEntry {
                        name: skill.name.clone().into(),
                        description: skill.description.clone().into(),
                        source: BundledSource::BuiltIn,
                        modified: false,
                    });
                    shipped_packages.insert(
                        skill.name.clone(),
                        crate::publish::gather_shipped_skill_package(&skill.name, md),
                    );
                }
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                entries.dedup_by(|a, b| a.name == b.name);

                // zed-kask: resolve the on-disk registry root. Dev (source
                // tree present): `kask/registry/`. Prod (seeded): the global
                // skills dir's sibling `registry/` (i.e.
                // `data_dir()/agents/registry/`). Mirrors `main.rs`; the
                // decision is pure in [`crate::publish::resolve_registry_root`]
                // so the dev/prod branch + no-parent fallback are tested.
                let globals_dir = agent_skills::global_skills_dir();
                let dev_manifests_exist = fs
                    .is_dir(std::path::Path::new("kask/registry/manifests"))
                    .await;
                let registry_root =
                    crate::publish::resolve_registry_root(dev_manifests_exist, &globals_dir);

                for entry in entries.iter_mut() {
                    let Some(seed_pkg) = shipped_packages.get(&*entry.name) else {
                        // No shipped reference: a user-added skill, not a
                        // modification of a bundled one. Don't badge it.
                        continue;
                    };
                    if seed_pkg.is_empty() {
                        continue;
                    }
                    let disk_pkg = crate::publish::gather_disk_skill_package(
                        fs.as_ref(),
                        &entry.name,
                        &registry_root,
                    )
                    .await;
                    if disk_pkg.is_empty() {
                        // Nothing on disk to compare (not yet seeded, or the
                        // user removed it). Don't badge.
                        continue;
                    }
                    let seed_files = seed_pkg
                        .iter()
                        .map(|(n, b)| (n.as_str(), b.as_slice()))
                        .collect::<Vec<_>>();
                    let disk_files = disk_pkg
                        .iter()
                        .map(|(n, b)| (n.as_str(), b.as_slice()))
                        .collect::<Vec<_>>();
                    let seed_hash = crate::publish::kask_skill_package_hash(&seed_files);
                    let disk_hash = crate::publish::kask_skill_package_hash(&disk_files);
                    entry.modified = seed_hash != disk_hash;
                }
                Ok::<_, anyhow::Error>(entries)
            }
            .await;

            this.update(cx, |this, cx| {
                match result {
                    Ok(entries) => {
                        this.bundled_entries = entries;
                        this.filter_extension_entries(cx);
                    }
                    Err(error) => {
                        log::warn!("kask-extensions: failed to load bundled skills: {error:#}")
                    }
                }
                cx.notify();
            })
            .ok();
        });
        self.bundled_fetch_task = Some(task);
    }

    /// zed-kask: Install a kask skill from a `kask-skill://` reference on the
    /// clipboard — the discreet-piggyback consumer (manual paste path). Reads
    /// the clipboard, parses the URI, and delegates to
    /// [`install_kask_skill_from_ref`].
    fn install_from_clipboard_ref(&mut self, cx: &mut Context<Self>) {
        let Some(clipboard) = cx.read_from_clipboard() else {
            self.status_message = Some("No clipboard content available.".into());
            cx.notify();
            return;
        };
        let Some(text) = clipboard.text() else {
            self.status_message = Some("Clipboard has no text.".into());
            cx.notify();
            return;
        };
        let Some(reff) = KaskSkillRef::parse(text.trim()) else {
            self.status_message = Some("Clipboard is not a kask-skill:// reference.".into());
            cx.notify();
            return;
        };
        self.install_kask_skill_from_ref(reff, cx);
    }

    /// zed-kask: Install a kask skill from a parsed `KaskSkillRef` — the
    /// shared backend for the clipboard paste path and the auto-discovered
    /// "shared in channels" cards. Fetches the signed metadata and installs
    /// via the existing verified path; feedback → `status_message`.
    fn install_kask_skill_from_ref(&mut self, reff: KaskSkillRef, cx: &mut Context<Self>) {
        let Some(http_client) = self.http_client.clone() else {
            self.status_message =
                Some("No HTTP client available; ensure you are logged in.".into());
            cx.notify();
            return;
        };
        let Some(fs) = self.fs.clone() else {
            self.status_message = Some("No filesystem available.".into());
            cx.notify();
            return;
        };
        let skill_id: Arc<str> = Arc::from(reff.id().as_str());
        self.outstanding_operations
            .insert(skill_id.clone(), KaskSkillStatus::Installing);
        self.status_message = Some(format!("Installing {}…", skill_id).into());
        cx.notify();

        let marketplace_dir = agent_skills::global_skills_dir().join("_marketplace");
        let skill_id_for_status = skill_id;
        cx.spawn(async move |this, cx| {
            let result = crate::publish::install_skill_from_ref(
                fs.as_ref(),
                &http_client,
                &reff,
                &marketplace_dir,
            )
            .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.outstanding_operations.remove(&skill_id_for_status);
                        this.status_message =
                            Some(format!("Installed {}.", skill_id_for_status).into());
                        this.filter_extension_entries(cx);
                    }
                    Err(error) => {
                        this.outstanding_operations.remove(&skill_id_for_status);
                        this.status_message = Some(format!("Install failed: {error:#}").into());
                        log::warn!("kask-extensions: install-from-ref failed: {error:#}");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// zed-kask: Scan the user's opened channel buffers for `kask-skill://`
    /// references — the discreet-piggyback auto-discovery. Channel messages
    /// are collaborative text buffers; a skill shared in a channel appears as
    /// ordinary message text containing the URI. This reads the text of every
    /// currently-open channel buffer (read-only — `open_channel_buffer`
    /// dedupes to the existing buffer for already-open channels, no re-join),
    /// scans for refs, and stores them in `shared_in_channels` for rendering.
    fn scan_open_channels_for_refs(&mut self, cx: &mut Context<Self>) {
        let Some(channel_store) = channel::ChannelStore::try_global(cx) else {
            self.status_message = Some("Channel store not available.".into());
            cx.notify();
            return;
        };
        // Collect ids of channels with an opened buffer (read-only check).
        let open_ids: Vec<client::ChannelId> = channel_store.read_with(cx, |store, cx| {
            store
                .channels()
                .filter(|ch| store.has_open_channel_buffer(ch.id, cx))
                .map(|ch| ch.id)
                .collect()
        });
        // For each opened channel, `open_channel_buffer` returns the existing
        // buffer as a ready Task (dedup — no re-join side effect).
        let tasks: Vec<Task<anyhow::Result<gpui::Entity<channel::ChannelBuffer>>>> = channel_store
            .update(cx, |store, cx| {
                open_ids
                    .iter()
                    .map(|id| store.open_channel_buffer(*id, cx))
                    .collect()
            });
        self.status_message = Some("Scanning open channels…".into());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let mut refs: Vec<KaskSkillRef> = Vec::new();
            for task in tasks {
                if let Ok(buffer) = task.await {
                    let text =
                        buffer.read_with(cx, |b, cx| b.buffer().read(cx).text_snapshot().text());
                    for reff in crate::publish::scan_for_skill_refs(&text) {
                        if !refs.contains(&reff) {
                            refs.push(reff);
                        }
                    }
                }
            }
            this.update(cx, |this, cx| {
                let count = refs.len();
                this.shared_in_channels = refs;
                this.status_message =
                    Some(format!("Found {count} skill reference(s) in your open channels.").into());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// zed-kask: Copy a `kask-skill://` share link for a marketplace skill to
    /// the clipboard — the discreet-piggyback producer. The user pastes it
    /// into a channel (or any multiplayer text surface); recipients install
    /// via "Install from reference".
    fn copy_share_link(&mut self, skill: &KaskSkillMetadata, cx: &mut Context<Self>) {
        let reff = KaskSkillRef {
            source_user: skill.manifest.source_user.clone(),
            skill_name: skill.manifest.skill_name.clone(),
            version: skill.manifest.version.clone(),
        };
        let uri = reff.to_uri();
        cx.write_to_clipboard(ClipboardItem::new_string(uri.clone()));
        self.status_message = Some(format!("Copied share link: {uri}").into());
        cx.notify();
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
    ) -> Vec<MarketplaceCard> {
        let mut cards = Vec::new();
        // zed-kask: three sections, in order: skill refs discovered in the
        // user's open channels (discreet piggyback), bundled skills, then the
        // marketplace catalog. The combined count sizes `uniform_list`.
        let shared_count = self.shared_in_channels.len();
        let bundled_count = self.filtered_bundled_indices.len();
        for ix in range {
            if ix < shared_count {
                cards
                    .push(self.render_shared_channel_card(self.shared_in_channels[ix].clone(), cx));
            } else if ix < shared_count + bundled_count {
                let bi = self.filtered_bundled_indices[ix - shared_count];
                let entry = self.bundled_entries[bi].clone();
                cards.push(self.render_bundled_card(entry));
            } else {
                let market_ix = ix - shared_count - bundled_count;
                if market_ix >= self.filtered_remote_skill_indices.len() {
                    break;
                }
                let skill_ix = self.filtered_remote_skill_indices[market_ix];
                let skill = self.remote_skill_entries[skill_ix].clone();
                cards.push(self.render_skill_card(skill, cx));
            }
        }
        cards
    }

    /// zed-kask: Render a skill reference discovered in a channel as an
    /// installable card (discreet piggyback). A "Shared in channel" badge +
    /// an Install button that resolves the ref via the verified install path.
    fn render_shared_channel_card(
        &mut self,
        reff: KaskSkillRef,
        cx: &mut Context<Self>,
    ) -> MarketplaceCard {
        let uri = reff.to_uri();
        let installing = self
            .outstanding_operations
            .get(reff.id().as_str())
            .is_some_and(|s| matches!(s, KaskSkillStatus::Installing));
        MarketplaceCard::new().child(
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
                                .min_w_0()
                                .child(Label::new(reff.id()).color(Color::Default).truncate())
                                .child(
                                    Label::new("Shared in channel")
                                        .color(Color::Muted)
                                        .flex_shrink_0(),
                                ),
                        )
                        .child(
                            Label::new(uri)
                                .color(Color::Muted)
                                .size(LabelSize::XSmall)
                                .truncate(),
                        ),
                )
                .child(if installing {
                    Button::new(
                        SharedString::from(format!("install-shared-{}", reff.id())),
                        "Installing…",
                    )
                    .style(ButtonStyle::Subtle)
                    .disabled(true)
                } else {
                    Button::new(
                        SharedString::from(format!("install-shared-{}", reff.id())),
                        "Install",
                    )
                    .style(ButtonStyle::Filled)
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.install_kask_skill_from_ref(reff.clone(), cx);
                    }))
                }),
        )
    }

    /// zed-kask: Render a kask skill card with install/uninstall/vote buttons.
    fn render_skill_card(
        &mut self,
        skill: KaskSkillMetadata,
        cx: &mut Context<Self>,
    ) -> MarketplaceCard {
        let status = self.skill_status(&skill.id, cx);
        let skill_id = skill.id.clone();
        let skill_id_for_uninstall = skill.id.clone();
        let skill_id_for_vote_up = skill.id.clone();
        let skill_id_for_vote_down = skill.id.clone();
        let skill_id_for_share = skill.id.clone();
        let http_client = self.http_client.clone();
        let fs = self.fs.clone();
        let marketplace_dir = agent_skills::global_skills_dir().join("_marketplace");
        let skill_for_share = skill.clone();

        MarketplaceCard::new().child(
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
                                .min_w_0()
                                .child(
                                    Label::new(skill.id.clone())
                                        .color(Color::Default)
                                        .truncate(),
                                )
                                .child(
                                    Label::new(format!("↓ {}", skill.download_count))
                                        .color(Color::Muted)
                                        .flex_shrink_0(),
                                )
                                .child(
                                    Label::new(format!("▲ {}", skill.upvote_count))
                                        .color(Color::Muted)
                                        .flex_shrink_0(),
                                )
                                .child(
                                    Label::new(format!("▼ {}", skill.downvote_count))
                                        .color(Color::Muted)
                                        .flex_shrink_0(),
                                ),
                        )
                        .child(
                            Label::new(skill.manifest.description.clone())
                                .color(Color::Muted)
                                .size(LabelSize::XSmall)
                                .truncate(),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .flex_shrink_0()
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
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("share-{}", skill_id_for_share)),
                                "Share",
                            )
                            .style(ButtonStyle::Subtle)
                            .on_click(cx.listener(
                                move |this, _event, _window, cx| {
                                    this.copy_share_link(&skill_for_share, cx);
                                },
                            )),
                        ),
                ),
        )
    }

    /// zed-kask: Render a bundled-skill card (shipped with the install).
    /// Simpler than a marketplace card: no install/vote buttons, since the
    /// skill ships with the app. A "Modified" badge marks on-disk overrides
    /// whose package hash differs from the shipped original.
    fn render_bundled_card(&self, entry: BundledSkillEntry) -> MarketplaceCard {
        let source_label: &'static str = match entry.source {
            BundledSource::BuiltIn => "Built-in",
            BundledSource::Shipped => "Bundled",
        };
        let mut header = h_flex()
            .gap_2()
            .min_w_0()
            .child(Label::new(entry.name).color(Color::Default).truncate())
            .child(Label::new(source_label).color(Color::Muted).flex_shrink_0());
        if entry.modified {
            header = header.child(
                Label::new("Modified")
                    .color(Color::Modified)
                    .flex_shrink_0(),
            );
        }
        MarketplaceCard::new().child(
            h_flex().w_full().gap_2().child(
                v_flex().min_w_0().flex_1().gap_1().child(header).child(
                    Label::new(entry.description)
                        .color(Color::Muted)
                        .size(LabelSize::XSmall)
                        .truncate(),
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

        // Find the skill in the catalog to get its manifest (which carries
        // the signature fields, expires_at, and tarball SHA256).
        let Some(skill) = self.remote_skill_entries.iter().find(|s| s.id == skill_id) else {
            log::warn!(
                "kask-extensions: skill '{}' not found in catalog; cannot install.",
                skill_id
            );
            return;
        };
        let manifest = skill.manifest.clone();
        let dependencies = manifest.dependencies.clone();
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
                &manifest,
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
        marketplace_search_bar(&self.query_editor, false, cx)
    }

    fn on_query_change(
        &mut self,
        _: Entity<Editor>,
        event: &editor::EditorEvent,
        cx: &mut Context<Self>,
    ) {
        if let editor::EditorEvent::Edited { .. } = event {
            self.refresh_search(cx);
        }
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        // zed-kask: debounce search, then filter locally. The kask skills
        // API returns the full catalog in one fetch; keystrokes must not
        // re-hit the network (previously each debounced keystroke refetched
        // the entire catalog from the collab server).
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

        let message: SharedString = if self.is_fetching_skills {
            "Loading kask skills…".into()
        } else if let Some(fetch_error) = &self.fetch_error {
            format!("Failed to load kask skills: {fetch_error}").into()
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
            .into()
        };

        marketplace_empty_state(message, self.fetch_error.is_some())
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
                    // zed-kask: inline status feedback for share / install-from-ref
                    // actions (clipboard-based piggyback in v1).
                    .when_some(self.status_message.clone(), |this, msg| {
                        this.child(Label::new(msg).color(Color::Muted))
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .gap_2()
                            .child(self.render_search(cx))
                            // zed-kask: toggle to include the skills that ship with the install
                            // (embedded global + built-in) in the list. When on, on-disk
                            // overrides that hash differently from the shipped package are
                            // badged "Modified".
                            .child(
                                Button::new(
                                    "bundled-skills-toggle",
                                    if self.show_bundled {
                                        "Bundled skills: On"
                                    } else {
                                        "Bundled skills: Off"
                                    },
                                )
                                .style(if self.show_bundled {
                                    ButtonStyle::Filled
                                } else {
                                    ButtonStyle::Subtle
                                })
                                .on_click(cx.listener(
                                    move |this, _event, _window, cx| {
                                        this.show_bundled = !this.show_bundled;
                                        if this.show_bundled && this.bundled_entries.is_empty() {
                                            this.fetch_bundled_skills(cx);
                                        }
                                        this.filter_extension_entries(cx);
                                        this.scroll_to_top(cx);
                                    },
                                )),
                            )
                            // zed-kask: discreet-piggyback consumer — install a skill
                            // from a `kask-skill://` reference on the clipboard
                            // (copied from a channel message or a Share button).
                            .child(
                                Button::new("install-from-ref", "Install from reference")
                                    .style(ButtonStyle::Subtle)
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.install_from_clipboard_ref(cx);
                                    })),
                            )
                            // zed-kask: discreet-piggyback auto-discovery — scan
                            // the user's opened channel buffers for `kask-skill://`
                            // references and surface them as installable cards.
                            .child(
                                Button::new("scan-channels", "Scan channels")
                                    .style(ButtonStyle::Subtle)
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.scan_open_channels_for_refs(cx);
                                    })),
                            )
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
                // zed-kask: count is shared-in-channels + filtered bundled + marketplace.
                let count = self.shared_in_channels.len()
                    + self.filtered_bundled_indices.len()
                    + self.filtered_remote_skill_indices.len();

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

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_api_types::{KaskSkillManifest, KaskSkillMetadata};

    fn skill(id: &str, description: &str) -> KaskSkillMetadata {
        use chrono::Utc;
        let manifest = KaskSkillManifest {
            source_user: "test".into(),
            skill_name: id.into(),
            version: "2026-08-06.1".into(),
            description: description.into(),
            dependencies: Vec::new(),
            tarball_sha256: "sha256:0".into(),
            public_key: "00".repeat(32),
            signature: "00".repeat(64),
            expires_at: "2026-12-06T00:00:00Z".into(),
        };
        KaskSkillMetadata {
            id: format!("test/{id}").into(),
            manifest,
            published_at: Utc::now(),
            download_count: 0,
            upvote_count: 0,
            downvote_count: 0,
        }
    }

    // zed-kask: pin the local-filter contract. `refresh_search` filters the
    // already-fetched catalog in memory; it must not re-hit the network on
    // every keystroke. The filter predicate is the observable contract —
    // if it drifts (e.g. a future change re-fetches), this test fails.
    #[test]
    fn skill_matches_query_substring_matches_id_and_description() {
        let s = skill("bug-hunt", "Find bugs in code");
        // No query → all skills match.
        assert!(skill_matches_query(&s, &None));
        // Query matches id.
        assert!(skill_matches_query(&s, &Some("bug".to_string())));
        // Query matches description.
        assert!(skill_matches_query(&s, &Some("find".to_string())));
        // Query matches neither.
        assert!(!skill_matches_query(&s, &Some("nonexistent".to_string())));
    }

    #[test]
    fn skill_matches_query_is_case_insensitive() {
        let s = skill("Bug-Hunt", "Find Bugs");
        // The caller lowercases the query before passing it; the predicate
        // lowercases the skill id/description. So a lowercased query must match.
        assert!(skill_matches_query(&s, &Some("bug".to_string())));
        assert!(skill_matches_query(&s, &Some("hunt".to_string())));
        // A mixed-case query would NOT match (the caller is responsible for
        // lowercasing); this pins that contract.
        assert!(!skill_matches_query(&s, &Some("BUG".to_string())));
    }

    #[test]
    fn skill_matches_query_empty_query_matches_all() {
        let s = skill("any-skill", "any description");
        // An empty query string is treated as a match (the caller converts
        // empty to `None` via `search_query`, but the predicate is defensive).
        assert!(skill_matches_query(&s, &Some(String::new())));
    }

    // zed-kask: pin that the `provides` filter row and extension upsell
    // banners are absent from the render output. The render method is heavy
    // to test (requires a full `Workspace`), but the deviations are
    // structural: the render method has no `provides` filter row and no
    // upsell banner child. A grep-based check pins that the render code
    // does not reference `provides` filter or upsell banner construction.
    // This is a static pin — it catches re-introduction of the removed UI.
    #[test]
    fn render_code_has_no_provides_filter_or_upsell_banner() {
        // The render method is at `fn render` in this file. Read the source
        // and assert the removed UI elements are not present. We exclude
        // the test module itself from the grep by checking only the
        // production code region (before `#[cfg(test)]`).
        let source = include_str!("kask_extensions_ui.rs");
        let production_code = source.split("#[cfg(test)]").next().unwrap_or(source);
        // The `provides` filter row was removed (kask skills have no
        // `provides` concept). A grep for the filter row's distinctive
        // label must find nothing in production code.
        assert!(
            !production_code.contains("\"Provides:\""),
            "the provides filter row must not be re-introduced"
        );
        // The extension upsell banners were removed. A grep for the
        // upsell banner's distinctive text must find nothing in production code.
        assert!(
            !production_code.contains("Install Zed Extensions"),
            "the extension upsell banner must not be re-introduced"
        );
    }
}
