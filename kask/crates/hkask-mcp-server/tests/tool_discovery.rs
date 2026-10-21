//! Compiler-backed discovery controls: declarations are not registrations until
//! their router is selected. This exercises rmcp metadata, not a stdio client.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};

#[derive(Clone)]
struct DiscoveryFixture;

async fn execute_tool(name: &str) -> String {
    name.to_owned()
}

#[tool_router(router = routed_router)]
impl DiscoveryFixture {
    #[tool(description = "Routed without an execution wrapper")]
    async fn direct(&self) -> String {
        "direct".to_owned()
    }

    #[tool(
        name = "wire_alias",
        description = "The wire name differs from the Rust method"
    )]
    async fn rust_method_name(&self) -> String {
        "alias".to_owned()
    }

    #[tool(description = "Routed through a differently named execution wrapper")]
    async fn wrapped(&self) -> String {
        execute_tool("wrapper_only_decoy").await
    }
}

#[tool_router(router = optional_router)]
impl DiscoveryFixture {
    #[tool(description = "Included only when its router is selected")]
    async fn conditional(&self) -> String {
        "conditional".to_owned()
    }
}

#[tool_router(router = unrouted_router)]
impl DiscoveryFixture {
    #[tool(description = "Declared but absent from the handler router")]
    async fn declared_but_unwired(&self) -> String {
        "unwired".to_owned()
    }
}

impl DiscoveryFixture {
    fn selected_router(include_optional: bool) -> ToolRouter<Self> {
        let router = Self::routed_router();
        if include_optional {
            router + Self::optional_router()
        } else {
            router
        }
    }

    async fn wrapper_without_declaration(&self) -> String {
        execute_tool("not_registered").await
    }
}

#[tool_handler(router = Self::selected_router(true))]
impl ServerHandler for DiscoveryFixture {}

fn names(router: ToolRouter<DiscoveryFixture>) -> Vec<String> {
    let mut names = router
        .list_all()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn discovery_follows_selected_routes_and_wire_names() {
    assert_eq!(
        names(DiscoveryFixture::selected_router(true)),
        ["conditional", "direct", "wire_alias", "wrapped"]
    );
    assert_eq!(
        names(DiscoveryFixture::selected_router(false)),
        ["direct", "wire_alias", "wrapped"]
    );
    assert_eq!(
        names(DiscoveryFixture::unrouted_router()),
        ["declared_but_unwired"],
        "a real declaration can exist in an unselected router"
    );
}

#[tokio::test]
async fn execution_wrappers_and_source_decoys_do_not_register_tools() {
    // #[tool(description = "comment decoy")] async fn comment_decoy() {}
    let string_decoy = r#"#[tool(description = "string decoy")] async fn string_decoy() {}"#;
    assert!(
        string_decoy.contains("#[tool"),
        "positive lexical decoy control"
    );
    assert_eq!(
        DiscoveryFixture.wrapper_without_declaration().await,
        "not_registered"
    );
    let registered = names(DiscoveryFixture::selected_router(true));
    for decoy in [
        "declared_but_unwired",
        "rust_method_name",
        "wrapper_only_decoy",
        "wrapper_without_declaration",
        "not_registered",
        "comment_decoy",
        "string_decoy",
    ] {
        assert!(
            !registered.iter().any(|name| name == decoy),
            "decoy {decoy}"
        );
    }
}
