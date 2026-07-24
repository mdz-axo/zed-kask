//! Transcript bundle — synchronized audio + word-level timed transcript.
//!
//! A TranscriptBundle links an audio recording with a word-level timed
//! transcript, enabling frontend interactions like:
//! - Highlighting words as audio plays
//! - Clicking a word to seek audio to that position
//! - Searching transcript text
//!
//! Format: `hkask-transcript-v1` JSON bundle.

use serde::{Deserialize, Serialize};

/// A single timed word in a transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedWord {
    /// The word text.
    pub word: String,
    /// Start time in milliseconds from audio beginning.
    pub start_ms: u64,
    /// End time in milliseconds from audio beginning.
    pub end_ms: u64,
    /// Confidence score (0.0–1.0) from the STT model.
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// A transcript segment (sentence or phrase) with timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    /// The segment text.
    pub text: String,
    /// Start time in milliseconds.
    pub start_ms: u64,
    /// End time in milliseconds.
    pub end_ms: u64,
}

/// A synchronized audio + transcript bundle.
///
/// Produced by `record_and_transcribe` or `transcribe_with_words` tools.
/// The frontend uses `words` for word-level highlighting and click-to-seek.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptBundle {
    /// Format identifier: "hkask-transcript-v1"
    pub format: String,

    /// Path to the audio file (WAV, 16kHz mono).
    pub audio_path: String,

    /// Total audio duration in seconds.
    pub audio_duration_secs: f32,

    /// Full transcript text (plain text, no timings).
    pub full_text: String,

    /// Word-level timing for interactive highlighting.
    /// Empty if word-level timestamps not available from STT model.
    #[serde(default)]
    pub words: Vec<TimedWord>,

    /// Segment-level timing (sentences/phrases).
    #[serde(default)]
    pub segments: Vec<TranscriptSegment>,

    /// Language code (e.g., "en") if known.
    #[serde(default)]
    pub language: Option<String>,

    /// STT model used for transcription.
    #[serde(default)]
    pub model: Option<String>,
}

impl TranscriptBundle {
    /// Create a new bundle with format marker.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  audio_path is a non-empty file path string; audio_duration_secs >= 0.0;
    ///       full_text is a valid transcript string (may be empty)
    /// post: returns a TranscriptBundle with format "hkask-transcript-v1",
    ///       empty words/segments vectors, and None for language/model
    pub fn new(audio_path: String, audio_duration_secs: f32, full_text: String) -> Self {
        Self {
            format: "hkask-transcript-v1".to_string(),
            audio_path,
            audio_duration_secs,
            full_text,
            words: Vec::new(),
            segments: Vec::new(),
            language: None,
            model: None,
        }
    }

    /// Total word count.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid TranscriptBundle
    /// post: returns the number of TimedWord entries in self.words (usize)
    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// Find the word at a given millisecond position.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  ms is any u64 millisecond offset
    /// post: returns Some(&TimedWord) if a word spans ms (start_ms <= ms < end_ms);
    ///       returns None if no word covers that position
    pub fn word_at_ms(&self, ms: u64) -> Option<&TimedWord> {
        self.words
            .iter()
            .find(|w| w.start_ms <= ms && ms < w.end_ms)
    }

    /// Get the segment containing a given millisecond position.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  ms is any u64 millisecond offset
    /// post: returns Some(&TranscriptSegment) if a segment spans ms
    ///       (start_ms <= ms < end_ms); returns None otherwise
    pub fn segment_at_ms(&self, ms: u64) -> Option<&TranscriptSegment> {
        self.segments
            .iter()
            .find(|s| s.start_ms <= ms && ms < s.end_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_has_format_marker() {
        let bundle = TranscriptBundle::new(
            "/tmp/audio.wav".to_string(),
            10.0,
            "Hello world.".to_string(),
        );
        assert_eq!(bundle.format, "hkask-transcript-v1");
    }

    #[test]
    fn word_at_ms_finds_correct_word() {
        let bundle = TranscriptBundle {
            words: vec![
                TimedWord {
                    word: "Hello".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    confidence: Some(0.99),
                },
                TimedWord {
                    word: "world".to_string(),
                    start_ms: 500,
                    end_ms: 900,
                    confidence: Some(0.98),
                },
            ],
            ..TranscriptBundle::new(
                "/tmp/audio.wav".to_string(),
                2.0,
                "Hello world.".to_string(),
            )
        };

        assert_eq!(bundle.word_at_ms(200).unwrap().word, "Hello");
        assert_eq!(bundle.word_at_ms(700).unwrap().word, "world");
        assert!(bundle.word_at_ms(1000).is_none());
    }
}
