# Corpus Ingestion Probe — Slice 6

**Date:** 2026-08-05
**Design:** `kask/docs/explanation/earnings-transcript-analysis-design.md` §(d) slice 6
**Status:** Probe complete — pipeline verified, no new code required

## The pipeline (§B3)

The corpus ingestion pipeline for an earnings transcript is a sequence of
existing corpus-server MCP tool calls:

```
→ corpus_chunk(text, entity_ref_prefix="company:{symbol}:earnings:{year}_Q{quarter}")
  → corpus_tag_chunks        # 5W1H + Dublin Core + PKO + FIBO
  → corpus_embed             # ontology-anchored vectors
  → corpus_extract_triples    # h_mems in the memory DB
  → centroid grouping        # per (company, theme)
  → corpus_query             # RAG surface
```

The entity-ref prefix is the single transcript-specific input. It's built by
`transcript::corpus_entity_ref_prefix(symbol, year, quarter)`:

```
company:MSFT:earnings:2024_Q4
```

This convention ensures chunks, tags, h_mems, and centroids all reference the
same provenance, enabling cross-document linkage (slice 7).

## Negative accept: corpus size limits (probe result)

**Probe question:** Does `corpus_cache` reject or truncate full-length FMP
transcript blobs (~45–51k chars)?

**Probe result:** `corpus_cache` (`kask/mcp-servers/hkask-mcp-corpus/src/tools/storage.rs:16`)
writes content to a file via `std::fs::write`. There is **no size limit in the
code** — the content string is written in full. The only validation is
non-empty content + non-empty label.

**Conclusion:** A full-length FMP transcript (~51k chars) caches without
truncation. The negative accept is satisfied: no fallback (chunk-then-cache)
is needed. If a filesystem-level limit (disk full, inode exhaustion) occurs,
`corpus_cache` returns an `McpToolError` via `map_corpus_io_error` — the error
is surfaced, not silently truncated.

## Segmentation (slice 4, folded in here)

The design §A5/Deferred #3 says: "The design builds segmentation in the
companies tool rather than around the corpus server" because `corpus_chunk`
is token-based only. The resolution: `corpus_tag_chunks` does LLM-based
ontology tagging (5W1H + Dublin Core + PKO + FIBO). For earnings transcripts,
the "segmentation" (prepared_remarks/Q&A + speaker attribution) is achieved
by tagging chunks with:

- `pko:Step` / `pko:Procedure` for the prepared-remarks vs Q&A sections
- `dc:contributor` for the speaker label
- `fibo:` concepts for financial claims (margin, capex, guidance)

No standalone segmentation module is needed — the tagging step subsumes it.
The no-fabrication invariant is preserved: tags are extracted from the chunk
text by the LLM, not fabricated.

## What slice 6 does NOT build (essentialist)

- **No new pipeline code** — the 5 corpus tools already exist and do the work.
- **No orchestration helper** — an agent sequences the 5 tool calls directly;
  a helper that just sequences 5 calls is a pass-through (essentialist G1 fail).
- **No segmentation module** — `corpus_tag_chunks` subsumes segmentation via
  ontology tags (slice 4 folded in here, not a standalone module).
- **No new storage** — `corpus_cache` + the memory DB handle persistence.

The only code added is `corpus_entity_ref_prefix` (one function, 3 tests) —
the entity-ref convention that ties the pipeline together.
