//! Tool surface pins — the exact registered-tool count, the generated
//! TOOL_NAMES set, and the live rmcp schemas, pinned against the combined
//! router: the registration integrity check for the whole server surface.
use super::*;

// One macro call pins the surface two ways: the exact count and
// the generated TOOL_NAMES set against the live rmcp surface. Catches
// silent registration drops — a `#[tool]` impl block without
// `#[tool_router]`, or a sub-router missing from `combined_router()`,
// silently registers nothing (`cargo check` passes on an unwired
// orphan) — and same-count renames, which the count alone cannot. The
// Steer prompt's tool list is RENDERED from TOOL_NAMES
// (hkask_steer::render_grouped_tool_advertisement in media_panel), so
// the name pin is that chain's integrity check: without it, an unrouted
// `#[tool]` fn would flow into the prompt as a dead name.
hkask_mcp_server::tool_surface_pin!(
    MediaServer::combined_router(),
    "combined_router",
    91,
    tool_surface_is_exactly_91_registered_tools,
);

/// The nostr panel names this server's fetch tool from `hkask-types`
/// (`media_wire::MEDIA_FETCH_IMAGE_TOOL`) so it need not link the
/// server. That name must be this server's own.
#[test]
fn wire_tool_name_is_the_servers_own() {
    assert!(TOOL_NAMES.contains(&hkask_types::media_wire::MEDIA_FETCH_IMAGE_TOOL));
}

/// `media_panel` reads `hkask_media::wire::MEDIA_TOOL_NAMES` so it does not
/// link `hkask-mcp-media` (`PRINCIPLES.md:P12.1`). Pin it against this
/// server's build-generated `TOOL_NAMES` so a rename, addition, or removal
/// cannot silently drift.
#[test]
fn wire_media_tool_names_match_server_tool_names() {
    let mut expected: Vec<&str> = TOOL_NAMES.to_vec();
    expected.sort_unstable();
    let mut actual: Vec<&str> = hkask_media::wire::MEDIA_TOOL_NAMES.to_vec();
    actual.sort_unstable();
    assert_eq!(
        actual, expected,
        "hkask_media::wire::MEDIA_TOOL_NAMES drifted from hkask_mcp_media::TOOL_NAMES"
    );
}

/// The one tool that contacts the network per uncached URL says so in
/// its description — the description is the model's safety surface.
#[test]
fn the_fetch_description_states_the_network_contact() {
    let router = MediaServer::combined_router();
    let tool = router
        .list_all()
        .into_iter()
        .find(|tool| tool.name == hkask_types::media_wire::MEDIA_FETCH_IMAGE_TOOL)
        .expect("media_fetch_image is registered");
    assert!(
        tool.description
            .as_deref()
            .unwrap_or_default()
            .contains("network"),
        "the fetch description must state the network contact"
    );
}

// Coverage: every registered tool must map to an OMC concept. Catches
// the silent-drop failure mode where a new tool is added to the router
// without a corresponding arm in omc::tool_to_omc.
#[test]
fn omc_mapping_covers_all_registered_tools() {
    let router = MediaServer::combined_router();
    for tool in router.list_all() {
        assert!(
            omc::tool_to_omc(&tool.name).is_some(),
            "omc::tool_to_omc returned None for registered tool '{}'; \
             add an explicit arm",
            tool.name
        );
    }
}

// Regression: distinct tool families must map to distinct concepts.
#[test]
fn omc_mapping_distinguishes_tool_families() {
    let version = omc::tool_to_omc("educt_locate");
    let scene = omc::tool_to_omc("describe_image");
    let asset = omc::tool_to_omc("gallery_organize");
    let source = omc::tool_to_omc("audio_capture");
    let sequence = omc::tool_to_omc("video_clip");
    let shot = omc::tool_to_omc("video_extract_frames");
    let task = omc::tool_to_omc("job_submit");
    let participant = omc::tool_to_omc("model_list");
    // Eight distinct concepts across retained research tool families.
    let concepts = [
        version,
        scene,
        asset,
        source,
        sequence,
        shot,
        task,
        participant,
    ];
    for (i, a) in concepts.iter().enumerate() {
        for (j, b) in concepts.iter().enumerate() {
            if i != j {
                assert_ne!(
                    a, b,
                    "tool families {i} and {j} must anchor on distinct concepts"
                );
            }
        }
    }
    assert_eq!(version, Some(omc::VERSION_INFO));
    assert_eq!(scene, Some(omc::SCENE));
    assert_eq!(asset, Some(omc::ASSET));
    assert_eq!(source, Some(omc::CAPTURE));
    assert_eq!(sequence, Some(omc::SEQUENCE));
    assert_eq!(shot, Some(omc::SHOT));
    assert_eq!(task, Some(omc::TASK));
    assert_eq!(participant, Some(omc::PARTICIPANT));
}
