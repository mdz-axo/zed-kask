//! hKask Runtime Services — text classification.

mod classify_impl;

pub use classify_impl::{
    ClassifierConfig, TripleExtraction, classify_batch, extract_triples_batch,
    load_classifier_config, parse_triple_extraction,
};
