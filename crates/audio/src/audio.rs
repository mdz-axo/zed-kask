use rodio::{ChannelCount, SampleRate, nz};

pub const SAMPLE_RATE: SampleRate = nz!(48000);
pub const CHANNEL_COUNT: ChannelCount = nz!(2);

mod audio_settings;
pub use audio_settings::AudioSettings;

mod audio_pipeline;
pub use audio_pipeline::Audio;
pub use audio_pipeline::init;

/// The sounds the editor plays. The call-related sounds (joined, mute,
/// screenshare, ...) were removed with the collab/call stack; the
/// agent-completion sound is the surviving kask-side consumer.
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub enum Sound {
    AgentDone,
}

impl Sound {
    fn file(&self) -> &'static str {
        match self {
            Self::AgentDone => "agent_done",
        }
    }
}
