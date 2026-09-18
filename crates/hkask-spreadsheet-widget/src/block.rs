//! The ```` ```spreadsheet ```` block body model + parser.
//!
//! Parsing is two-stage, matching the viz-widget contract in
//! `hkask-viz-core`: the [`SpreadsheetBlockBody`] type is deliberately
//! tolerant (every field defaults) so foreign-shaped JSON (portfolio,
//! scenarios, media bodies) parses without error and is rejected by the
//! `viz` discriminator check — never logged as malformed. Only after the
//! body claims the `spreadsheet` tag does the strict wire contract
//! (`hkask_types::spreadsheet::SpreadsheetBlock`) parse; a claimed body that
//! fails the strict parse surfaces as a visible widget error state, never a
//! panic.

use hkask_types::spreadsheet::SPREADSHEET_VIZ;
use hkask_types::spreadsheet::SpreadsheetBlock;
use serde::Deserialize;

/// The tolerant discriminator-tagged body of a ```` ```spreadsheet ```` block.
///
/// `viz` selects the renderer; `"spreadsheet"` renders this widget. All other
/// fields are defaulted-and-optional at THIS stage so the body parses under
/// any foreign shape; the strict contract parse happens after the claim.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpreadsheetBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    #[serde(default)]
    pub schema_version: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub active_sheet: Option<String>,
    #[serde(default)]
    pub artifact: Option<hkask_types::spreadsheet::SpreadsheetArtifactRef>,
    #[serde(default)]
    pub viewport: Option<hkask_types::spreadsheet::SpreadsheetViewport>,
    #[serde(default)]
    pub origin: Option<hkask_types::spreadsheet::ArtifactOrigin>,
    #[serde(default)]
    pub mutation: hkask_types::BlockProvenance,
}

/// Parse a block body tolerantly (viz discriminator read). Foreign-shaped
/// JSON parses to defaulted fields rather than an error.
pub fn parse_spreadsheet_body(body: &str) -> anyhow::Result<SpreadsheetBlockBody> {
    Ok(serde_json::from_str::<SpreadsheetBlockBody>(body.trim())?)
}

impl SpreadsheetBlockBody {
    /// Whether this body claims the spreadsheet renderer.
    pub fn claims(&self) -> bool {
        self.viz.as_deref() == Some(SPREADSHEET_VIZ)
    }

    /// The strict wire contract, parsed after the claim. Errors carry the
    /// reason for the widget's visible error state.
    pub fn strict_block(&self) -> Result<SpreadsheetBlock, String> {
        let artifact = self.artifact.clone().ok_or_else(|| {
            "spreadsheet block is missing its opaque artifact identity".to_string()
        })?;
        let viewport = self
            .viewport
            .clone()
            .ok_or_else(|| "spreadsheet block is missing its initial viewport".to_string())?;
        let origin = self
            .origin
            .clone()
            .ok_or_else(|| "spreadsheet block is missing its analytical origin".to_string())?;
        let title = self
            .title
            .clone()
            .ok_or_else(|| "spreadsheet block is missing its title".to_string())?;
        let active_sheet = self
            .active_sheet
            .clone()
            .ok_or_else(|| "spreadsheet block is missing its active sheet".to_string())?;
        let block = SpreadsheetBlock::new(
            title,
            active_sheet,
            artifact,
            viewport,
            origin,
            self.mutation.clone(),
        )
        .map_err(|error| format!("spreadsheet block failed its own contract: {error}"))?;
        Ok(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(viz: Option<&str>) -> String {
        serde_json::json!({
            "viz": viz,
            "schema_version": 1,
            "title": "What-if",
            "active_sheet": "Main",
            "artifact": {
                "artifact_id": "art-1",
                "revision_id": "rev-1",
                "content_digest": "a".repeat(64),
            },
            "viewport": {"sheet": "Main", "start_row": 0, "start_col": 0, "row_count": 10, "col_count": 4},
            "origin": {"server": "hkask-mcp-portfolio", "tool": "portfolio_what_if", "arguments": {}},
            "mutation": {"tool": "spreadsheet_apply", "server": "spreadsheet", "args": {}},
        })
        .to_string()
    }

    #[test]
    fn foreign_shapes_parse_tolerantly_and_do_not_claim() {
        // A portfolio-shaped body parses (all defaults) and does not claim.
        let parsed = parse_spreadsheet_body(
            r#"{"viz": "portfolio", "portfolio": "main", "holdings": {"rows": []}}"#,
        )
        .expect("foreign body parses tolerantly");
        assert_eq!(parsed.viz.as_deref(), Some("portfolio"));
        assert!(!parsed.claims());

        // No viz at all (media-shaped) — parses, no claim.
        let parsed = parse_spreadsheet_body(r#"{"gallery_asset_id": "x"}"#)
            .expect("media-shaped body parses tolerantly");
        assert!(!parsed.claims());
    }

    #[test]
    fn spreadsheet_body_claims_and_parses_strictly() {
        let parsed =
            parse_spreadsheet_body(&body(Some("spreadsheet"))).expect("spreadsheet body parses");
        assert!(parsed.claims());
        let block = parsed.strict_block().expect("strict contract parse");
        assert_eq!(block.active_sheet, "Main");
        assert!(block.mutation.is_dispatchable());
        assert!(block.validate().is_ok());
    }

    #[test]
    fn claimed_body_with_missing_fields_surfaces_a_visible_error() {
        // Claims the tag but carries no artifact identity: the strict parse
        // names what is missing — an error state, never a panic.
        let parsed =
            parse_spreadsheet_body(r#"{"viz": "spreadsheet", "title": "Broken"}"#).expect("parses");
        assert!(parsed.claims());
        let error = parsed
            .strict_block()
            .expect_err("missing identity must fail");
        assert!(error.contains("artifact"), "unexpected error: {error}");
    }

    #[test]
    fn non_dispatchable_mutation_is_rejected_by_the_strict_contract() {
        let json = serde_json::json!({
            "viz": "spreadsheet",
            "schema_version": 1,
            "title": "T",
            "active_sheet": "Main",
            "artifact": {"artifact_id": "a", "revision_id": "r", "content_digest": "a".repeat(64)},
            "viewport": {"sheet": "Main", "start_row": 0, "start_col": 0, "row_count": 2, "col_count": 2},
            "origin": {"server": "s", "tool": "t", "arguments": {}},
        })
        .to_string();
        let parsed = parse_spreadsheet_body(&json).expect("parses");
        let error = parsed
            .strict_block()
            .expect_err("incomplete mutation must fail");
        assert!(error.contains("provenance"), "unexpected error: {error}");
    }
}
