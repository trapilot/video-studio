use serde::Deserialize;

/// Studio-wide technical settings. What to produce (format, audience, language,
/// publish metadata) lives in the per-video-type profiles (`entities/profile.rs`).
#[derive(Debug, Clone)]
pub struct Config {
    pub video: VideoConfig,
    pub voice: VoiceConfig,
    pub tts: TtsConfig,
    pub diffusion: DiffusionParams,
    pub feature: FeatureConfig,
    pub workflow_per_day: i64,
    // pub workflow_gap_minutes: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoConfig {
    /// Output frame size. Every shot is scaled and center-cropped to it.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceConfig {
    pub base_voice: String,
    pub default_gender: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    pub speed: f32,
    pub stability: f32,
    pub similarity_boost: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiffusionParams {
    pub num_steps: u32,
    pub guidance: f32
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeatureConfig {
    pub enable_cache: bool,
    pub enable_subtitle: bool,
    pub enable_translation: bool,
}

pub fn build_config() -> Config {
    Config {
        // Vertical 9:16 for YouTube Shorts and TikTok (the subtitle layout assumes it too).
        video: VideoConfig {
            width: 1080,
            height: 1920,
            fps: 25,
        },
        workflow_per_day: 3,
        // workflow_gap_minutes: 15,
        voice: VoiceConfig {
            base_voice: "pNInz6obpgDQGcFmaJgB".to_string(),
            default_gender: "female".to_string(),
        },
        tts: TtsConfig {
            speed: 1.0,
            stability: 0.8,
            similarity_boost: 0.75,
        },
        diffusion: DiffusionParams {
            num_steps: 4,
            guidance: 3.5, 
        },
        feature: FeatureConfig {
            enable_cache: false,
            enable_subtitle: false,
            enable_translation: false,
        },
    }
}