//! Voice design — structured voice profile for TTS generation.
//!
//! A VoiceDesign is a machine-readable description of a synthetic voice
//! that can be passed to TTS models for consistent speech generation.
//! Provider-neutral voice configuration for inference callers,
//! stored on the AgentPod, and consumed by the talk service.

use serde::{Deserialize, Serialize};

/// A structured voice profile for TTS generation.
///
/// Supplied by an inference caller and stored with the agent,
/// and passed to TTS models as a natural-language voice description.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceDesign {
    /// Human-readable name for this voice (e.g., "Warm Mentor", "Crisp Analyst")
    pub name: String,

    /// Pitch register: "low", "medium-low", "medium", "medium-high", "high"
    pub pitch: String,

    /// Timbral quality: "warm", "bright", "dark", "breathy", "clear", "resonant", "nasal"
    pub timbre: String,

    /// Speaking pace: "slow", "deliberate", "moderate", "brisk", "fast"
    pub pace: String,

    /// Accent or dialect: "american", "british", "australian", "indian", etc.
    /// Empty string means neutral/unmarked.
    #[serde(default)]
    pub accent: String,

    /// Emotional range this voice can express.
    /// e.g., ["neutral", "warm", "authoritative", "playful", "concerned"]
    #[serde(default)]
    pub emotion_range: Vec<String>,

    /// Gender presentation: "masculine", "feminine", "androgynous", "neutral"
    #[serde(default)]
    pub gender_presentation: String,

    /// Perceived age range: "young", "young-adult", "middle-aged", "senior"
    #[serde(default)]
    pub age_range: String,

    /// Natural-language description synthesizing all parameters.
    /// This is the primary input to TTS models — a prose description
    /// like "A warm, middle-aged female voice with a gentle British accent,
    /// speaking at a moderate pace with a clear, resonant timbre."
    pub description: String,
}

impl Default for VoiceDesign {
    fn default() -> Self {
        Self {
            name: "Neutral".to_string(),
            pitch: "medium".to_string(),
            timbre: "clear".to_string(),
            pace: "moderate".to_string(),
            accent: String::new(),
            emotion_range: vec!["neutral".to_string()],
            gender_presentation: "neutral".to_string(),
            age_range: "middle-aged".to_string(),
            description: "A clear, neutral voice speaking at a moderate pace.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_voice_is_neutral() {
        let v = VoiceDesign::default();
        assert_eq!(v.name, "Neutral");
        assert_eq!(v.pitch, "medium");
    }
}
