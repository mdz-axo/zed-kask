//! hKask Corpus Services — company-source manifests.
//!
//! The former discovery, style-embedding, and fetch subtrees were removed as
//! unwired dead surface (zero production callers); the company manifest
//! remains live via `corpus_discover_company`.

mod company_manifest;

pub(crate) use company_manifest::CompanySourceManifest;
