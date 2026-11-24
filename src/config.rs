use serde::Deserialize;

/// Studio-wide technical settings. What to produce (format, audience, language,
/// publish metadata) lives in the per-video-type profiles in `src/profiles.rs`.
#[derive(Debug)]
pub struct Config {
    pub video: VideoConfig,
    pub voice: VoiceConfig,
    pub tts: TtsConfig,
    pub feature: FeatureConfig,
}

#[derive(Debug, Deserialize)]
pub struct VideoConfig {
    /// Output frame size. Every scene is scaled and center-cropped to it.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(Debug, Deserialize)]
pub struct VoiceConfig {
    pub base_voice: &'static str,
    pub default_gender: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct TtsConfig {
    pub speed: f32,
    pub stability: f32,
    pub similarity_boost: f32,
}

#[derive(Debug, Deserialize)]
pub struct FeatureConfig {
    pub enable_cache: bool,
    pub enable_subtitle: bool,
    pub enable_translation: bool,
}

pub static CONFIG: Config = Config {
    // Vertical 9:16 for YouTube Shorts and TikTok.
    video: VideoConfig {
        width: 1080,
        height: 1920,
        fps: 25,
    },

    voice: VoiceConfig {
        // Fallback ElevenLabs voice: narrator voice and any speaker without its own voice_id.
        base_voice: "pNInz6obpgDQGcFmaJgB",
        default_gender: "female",
    },

    tts: TtsConfig {
        speed: 1.0,
        stability: 0.8,
        similarity_boost: 0.75,

    },

    feature: FeatureConfig {
        enable_cache: false,
        enable_subtitle: false,
        enable_translation: false,
    }
};
