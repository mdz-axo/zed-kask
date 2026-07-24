# hkask-bridge-dublincore

Dublin Core + BIBO + CiTO vocabulary bridge — shared bibliographic metadata constants for hKask's state (entity) ontological axis.

Part of the dual-axis ontological framework (P5.4): every MCP server uses this crate alongside `hkask-bridge-pko` (process axis).

## Concepts (50+)

- **Dublin Core Terms:** title, creator, date, description, format, type, subject, identifier, source, language, rights, publisher, contributor
- **Dublin Core Types:** StillImage, MovingImage, Sound, Text, Dataset, Software, Collection, BibliographicResource
- **BIBO:** Article, AcademicArticle, Journal, Book, BookSection, Thesis, Webpage, Document, Preprint, Proceedings, Report, Manuscript
- **CiTO:** cites, isCitedBy, supports, refutes, discusses, reviews, repliesTo, usesDataFrom, citesAsDataSource, citesAsEvidence

## Mapping Helpers

- `mime_to_dc_type(mime)` — MIME type → Dublin Core type
- `kind_to_bibo(kind)` — informal label → BIBO type

## Usage

```rust
use hkask_bridge_dublincore::{TITLE, ARTICLE, kind_to_bibo};

let dc_type = kind_to_bibo("preprint"); // → bibo:Preprint
```
