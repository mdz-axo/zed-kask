//! Voice design — structured voice profile for TTS generation.
//!
//! A VoiceDesign is a machine-readable description of a synthetic voice
//! that can be passed to TTS models for consistent speech generation.
//! Designed by the `voice_design` tool in the media MCP server,
//! stored on the AgentPod, and consumed by the talk service.

use serde::{Deserialize, Serialize};

/// A structured voice profile for TTS generation.
///
/// Designed by an LLM via the `voice_design` tool, stored with the userpod,
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

impl VoiceDesign {
    /// Render this voice design as a compact prose description for TTS model input.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid VoiceDesign with all fields populated
    /// post: returns a prose string describing the voice's gender, age, timbre,
    ///       accent, pace, and emotion range, ending with "."
    pub fn to_tts_description(&self) -> String {
        let mut parts = Vec::new();

        if !self.gender_presentation.is_empty() && self.gender_presentation != "neutral" {
            parts.push(self.gender_presentation.clone());
        }
        if !self.age_range.is_empty() {
            parts.push(self.age_range.clone());
        }
        parts.push("voice".to_string());

        if !self.timbre.is_empty() {
            parts.push(format!("with a {} timbre", self.timbre));
        }
        if !self.accent.is_empty() {
            parts.push(format!("and a {} accent", self.accent));
        }
        parts.push(format!("speaking at a {} pace", self.pace));

        if !self.emotion_range.is_empty() && self.emotion_range != vec!["neutral".to_string()] {
            parts.push(format!(
                "capable of {} tones",
                self.emotion_range.join(", ")
            ));
        }

        parts.join(", ") + "."
    }

    /// Map this voice design to the closest ElevenLabs voice preset.
    ///
    /// ElevenLabs voices available on both DeepInfra and fal.ai:
    /// Rachel (default, warm feminine), Aria (soft feminine), Roger (confident masculine),
    /// Sarah (warm feminine), Laura (calm feminine), Charlie (friendly masculine),
    /// George (authoritative masculine), Callum (deep masculine), River (gentle androgynous),
    /// Liam (young masculine), Charlotte (bright feminine), Alice (clear feminine),
    /// Matilda (young feminine), Will (warm masculine), Jessica (expressive feminine),
    /// Eric (steady masculine), Chris (casual masculine), Brian (deep masculine),
    /// Daniel (measured masculine), Lily (soft feminine), Bill (older masculine).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid VoiceDesign with gender_presentation, age_range,
    ///       timbre, and pitch fields set
    /// post: returns a &'static str naming one of the known ElevenLabs voice
    ///       presets; always returns a valid preset name (never panics)
    pub fn to_elevenlabs_voice(&self) -> &'static str {
        // Match on gender presentation + age + timbre
        let feminine = self.gender_presentation == "feminine";
        let masculine = self.gender_presentation == "masculine";
        let young = self.age_range == "young" || self.age_range == "young-adult";
        let senior = self.age_range == "senior";
        let warm = self.timbre == "warm";
        let bright = self.timbre == "bright";
        let deep = self.timbre == "dark" || self.pitch == "low";

        if feminine && warm && senior {
            "Sarah"
        } else if feminine && warm && young {
            "Matilda"
        } else if feminine && bright {
            "Charlotte"
        } else if feminine && deep {
            "Laura"
        } else if feminine {
            "Rachel" // default feminine
        } else if masculine && deep && senior {
            "Bill"
        } else if masculine && deep {
            "George"
        } else if masculine && warm && young {
            "Liam"
        } else if masculine && warm {
            "Will"
        } else if masculine && bright {
            "Charlie"
        } else if masculine {
            "Roger" // default masculine
        } else if young {
            "River" // androgynous young
        } else {
            "Rachel" // universal default
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

    #[test]
    fn tts_description_renders_prose() {
        let v = VoiceDesign {
            name: "Warm Mentor".to_string(),
            pitch: "medium-low".to_string(),
            timbre: "warm".to_string(),
            pace: "moderate".to_string(),
            accent: "british".to_string(),
            emotion_range: vec!["warm".to_string(), "authoritative".to_string()],
            gender_presentation: "feminine".to_string(),
            age_range: "middle-aged".to_string(),
            description: "A warm, middle-aged feminine voice with a British accent.".to_string(),
        };
        let desc = v.to_tts_description();
        assert!(desc.contains("feminine"));
        assert!(desc.contains("british"));
        assert!(desc.contains("warm"));
    }
}
