//! The export family: export every stored media layer item over one
//! recording as corpus passages — the semantic handoff for corpus
//! ingestion (a pure layer-store read that works on detached media —
//! the layer-store counterpart of educt_export's corpus_text).

use super::*;

#[tool_router(router = layers_export_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Export every stored media layer item over one recording as corpus passages — the semantic handoff for corpus ingestion. Each item becomes one passage carrying its stable item id (resolvable via layers_get), start-end seconds, kind, label, and its confidence when it has one, preceded by a recording header naming the media path. A pure layer-store read: works on any recording that has stored layers, whether or not the source media file still exists (detached-media layers survive source deletion). Feed the output to corpus_convert → corpus_chunk → corpus_embed; each item's text is a natural chunk boundary."
    )]
    pub async fn layers_export(
        &self,
        Parameters(request): Parameters<LayersExportRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_export", async {
            // No ensure_local_input_exists gate: this is a pure layer-store
            // read. Detached-media recordings (source file deleted, layers
            // surviving via the detach-and-surface contract) must export —
            // the same surfaces layers_transcript reads without a gate.
            let driver = &**self.gallery_store.driver();
            let records = layer_store::recording_layers(
                driver,
                &request.media_path,
                request.kind,
            )
            .map_err(map_layer_store_error)?;
            if records.is_empty() {
                return Err(McpToolError::not_found(format!(
                    "no layers found for {}",
                    request.media_path
                )));
            }
            let mut passages = String::new();
            let mut items_exported = 0;
            for record in &records {
                for item in &record.items {
                    // Written as a number, or left out: the passage is
                    // corpus text, and `confidence=Some(0.9)` / `=None`
                    // was this program's debug notation inside it.
                    let confidence = item
                        .item
                        .payload
                        .get("confidence")
                        .and_then(serde_json::Value::as_f64)
                        .map(|confidence| format!(" confidence={confidence}"))
                        .unwrap_or_default();
                    passages.push_str(&format!(
                        "## recording: {}\n### item: {} kind={} start={:.3}s end={:.3}s label={:?}{}\n{}\n\n",
                        request.media_path,
                        item.id,
                        record.kind.as_str(),
                        item.item.start_sec,
                        item.item.end_sec.unwrap_or(item.item.start_sec),
                        record.label.as_deref().unwrap_or(""),
                        confidence,
                        item.item.searchable_text,
                    ));
                    items_exported += 1;
                }
            }
            Ok(serde_json::json!({
                "status": "exported",
                "media_path": request.media_path,
                "layers": records.len(),
                "items": items_exported,
                "passages": passages,
                "composition": "run corpus_convert → corpus_chunk → corpus_embed on the passages; each item id resolves back via layers_get",
            }))
        })
        .await
    }
}
