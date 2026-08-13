//! hKask Runtime Services — text classification.

mod classify_impl;

pub use classify_impl::{
    ClassifierConfig, PassageExtraction, classify_batch, extract_passages_batch,
    load_classifier_config, parse_passage_extraction,
};
