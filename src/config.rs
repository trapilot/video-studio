use serde::Deserialize;

/// Studio-wide technical settings. What to produce (format, audience, language,
/// publish metadata) lives in the per-video-type profiles (`entities/profile.rs`).
#[derive(Debug, Clone)]
pub struct Config {
    pub video: VideoConfig,
    pub voice: VoiceConfig,
    pub tts: TtsConfig,
    pub image: ImageConfig,
    pub cleanup: CleanupConfig,
    pub productions_per_day: i64,
    // pub production_gap_minutes: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoConfig {
    /// Output frame size. Every shot is scaled and center-cropped to it.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Burn the dialogue into the video as subtitles.
    pub burn_subtitles: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceConfig {
    /// ElevenLabs voice for the narrator and any speaker without a `voice_id`.
    /// Use a premade voice: custom voices exist only on the account that made them.
    pub base_voice: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    pub speed: f32,
    pub stability: f32,
    pub similarity_boost: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageConfig {
    pub num_steps: u32,
    pub guidance: f32
}

#[derive(Debug, Clone, Deserialize)]
pub struct CleanupConfig {
    /// Keep per-shot files and the un-subtitled video after publishing
    /// (useful for debugging). `false` = the Cleaner deletes them.
    pub keep_intermediates: bool,
}

pub fn build_config() -> Config {
    Config {
        // Vertical 9:16 for YouTube Shorts and TikTok (the subtitle layout assumes it too).
        video: VideoConfig {
            width: 1080,
            height: 1920,
            fps: 25,
            burn_subtitles: true,
        },
        productions_per_day: 3,
        // production_gap_minutes: 15,
        voice: VoiceConfig {
            base_voice: "pNInz6obpgDQGcFmaJgB".to_string(),
        },
        tts: TtsConfig {
            speed: 1.0,
            stability: 0.8,
            similarity_boost: 0.75,
        },
        image: ImageConfig {
            num_steps: 4,
            guidance: 3.5, 
        },
        cleanup: CleanupConfig {
            keep_intermediates: false,
        },
    }
}