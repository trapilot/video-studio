
use std::sync::Arc;
use async_trait::async_trait;
use futures::StreamExt;
use futures::TryStreamExt;

use crate::AppState;
use crate::agent::*;
use crate::models::*;
use crate::entities::*;
use crate::provider;
use crate::config;
use crate::helper;


#[derive(Debug, Clone)]
pub struct ShotAssets {
    pub root_path: String,
    pub image_path: String,
    pub audio_path: String,
    pub video_path: String,
}

pub struct ProducedShot {
    pub shot_id: u32,
    pub image_path: String,
    pub audio_path: String,
    pub video_path: String,
    pub subtitle: String,
    pub duration: f64,
    pub motion: Motion,
    pub transition: Transition,
}

impl ShotAssets {
    pub fn new(base: &str, shot_id: u32) -> Self {
        let root = format!("{base}/shot_{shot_id}");

        Self {
            image_path: format!("{root}/image.png"),
            audio_path: format!("{root}/audio.wav"),
            video_path: format!("{root}/video.mp4"),
            root_path: root,
        }
    }

    pub async fn create_dir_all(&self) -> Result<(), String> {
        tokio::fs::create_dir_all(&self.root_path)
            .await
            .map_err(|e| format!("create {}: {}", self.root_path, e))
    }
}

pub struct BuilderAgent;

#[async_trait]
impl Agent for BuilderAgent {
    async fn run(&self, state: &Arc<AppState>, job: &Job) -> Result<(), AgentError> {
        println!("🔧 [Builder] Generating assets...");
        
        let storyboard: Storyboard =
            serde_json::from_str(&job.payload)
            .map_err(|e| AgentError::Decode(e.to_string()))?;

        let profile = job_profile(state, job).await?;
        let output_path = state.services.db
            .production_output_path(&job.production_id)
            .await
            .map_err(|e| AgentError::Execute(e.to_string()))?;

        let timeline: Timeline = self.execute(
                &state,
                profile,
                &storyboard,
                output_path,
            )
            .await
            .map_err(|e| AgentError::Execute(e.to_string()))?;

        let payload = serde_json::to_string(&timeline)
            .map_err(|e| AgentError::Encode(e.to_string()))?;

        state.services.db
            .handoff_job(job, AgentType::Renderer, payload)
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        Ok(())
    }
}

impl BuilderAgent {
    async fn execute(
        &self,
        state: &Arc<AppState>,
        profile: &'static StudioProfile,
        storyboard: &Storyboard,
        target_path: String
    ) -> Result<Timeline, String> {
        // One seed per production: the same prompt then gives the same image on any
        // Cloudflare account/key, so retries and key rotation stay consistent.
        let seed = stable_seed(&target_path);

        let handles = storyboard
            .shots
            .iter()
            .cloned()
            .filter(|shot| !shot.dialogues.is_empty())
            .map(|shot| {
                let providers = state.services.providers.clone();
                let config = state.config.clone();
                let target_path = target_path.clone();

                tokio::spawn(async move {
                    let assets = ShotAssets::new(&target_path, shot.shot_id);
                    assets.create_dir_all().await?;

                    let exists_image = tokio::fs::try_exists(&assets.image_path).await.unwrap_or(false);
                    let exists_audio = tokio::fs::try_exists(&assets.audio_path).await.unwrap_or(false);
                    
                    if !exists_image {
                        text_to_image(
                            shot.image_prompt(profile),
                            seed,
                            assets.clone(),
                            config.clone(),
                            providers.clone()
                        ).await.map_err(|e| format!("Shot {} image: {}", shot.shot_id, e))?;
                    }

                    if !exists_audio {
                        text_to_audio(
                            shot.dialogues.clone(),
                            assets.clone(),
                            profile,
                            config.clone(),
                            providers.clone()
                        ).await.map_err(|e| format!("Shot {} audio: {}", shot.shot_id, e))?;
                    }
                    
                    let duration = get_audio_duration(&assets.audio_path).await?;

                    Ok::<_, String>(ProducedShot {
                        shot_id: shot.shot_id,
                        image_path: assets.image_path,
                        audio_path: assets.audio_path,
                        video_path: assets.video_path,
                        subtitle: shot.dialogues
                            .iter()
                            .map(|d| d.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                        duration,
                        transition: shot.transition,
                        motion: shot.motion,
                    })
                })
            })
            .collect::<Vec<_>>();
        

        let mut produced_shots = Vec::new();

        for handle in handles {
            let shot = handle
                .await
                .map_err(|e| e.to_string())??;

            produced_shots.push(shot);
        }

        produced_shots.sort_by_key(|v| v.shot_id);
        
        if produced_shots.is_empty() {
            return Err("No shots to build".to_string());
        }

        let transition_duration = Transition::DURATION;

        // Each clip is padded by the transition length: speech starts after half of
        // it (adelay) and the rest is silence. Consecutive clips overlap by
        // `Transition::fade_between` (the same value the Renderer uses for xfade),
        // so each clip starts where the previous one ends minus that overlap.
        // start/end_time are when the speech is heard in the final video, for subtitles.
        let mut clip_start = 0.0;
        let mut prev_duration: Option<f64> = None;
        let mut clips = Vec::new();
    
        for shot in produced_shots {
            let duration = shot.duration + transition_duration;
            // Overlap with the previous clip (none for the first one).
            let fade = prev_duration.map(|p| Transition::fade_between(p, duration)).unwrap_or(0.0);
            clip_start -= fade;
            let start_time = clip_start + transition_duration / 2.0;
            let end_time = start_time + shot.duration;
            clip_start += duration;
            prev_duration = Some(duration);

            clips.push(Clip {
                shot_id: shot.shot_id,
                visual_path: shot.image_path,
                audio_path: shot.audio_path,
                video_path: shot.video_path,
                subtitle: shot.subtitle,
                duration,
                start_time,
                end_time,
                acrossfade: fade,
                transition: shot.transition,
                motion: shot.motion,
            });
        }

        Ok(Timeline {
            title: storyboard.title.clone(),
            final_path: format!("{}/final_video.mp4", &target_path),
            video_path: format!("{}/video.mp4", &target_path),
            subtitle_path: format!("{}/subtitle.ass", &target_path),
            clips,
        })
    }
}

async fn text_to_image(
    prompt: String,
    seed: u32,
    assets: ShotAssets,
    config: Arc<config::Config>,
    providers: Arc<provider::ProviderManager>,
) -> Result<(), String> {
    let req =
        provider::ProviderRequest::Image(
            provider::ImageRequest {
                prompt: prompt,
                seed: Some(seed),
                num_steps: Some(config.image.num_steps),
                guidance: Some(config.image.guidance),
                width: None,
                height: None,
            },
        );

    let provider = provider::Provider::CFWorker;

    let guard = providers
        .acquire(&provider)
        .await
        .ok_or_else(|| {
            "Cloudflare unavailable: no usable CF_WORKER_KEY_n/CF_WORKER_ACCOUNT_n (or all keys blocked)".to_string()
        })?;

    let image = guard
        .call(req)
        .await
        .map_err(|e| e.to_string())?
        .into_bytes()
        .map_err(|e| e.to_string())?;

    if !helper::is_image(&image) {
        return Err(format!("Provider returned no image: {}", helper::preview(&image)));
    }

    helper::write_atomic(&assets.image_path, &image).await
}


const PIPER_BIN: &str = "./tools/piper/piper";
const PIPER_MODEL: &str = "./tools/models/en_US-lessac-medium.onnx";
/// Language of `PIPER_MODEL`. Profiles in other languages use ElevenLabs.
const PIPER_LANGUAGE: &str = "English";
const TTS_CONCURRENCY: usize = 4;
/// Piper's lessac-medium model and the ElevenLabs request both produce this
/// format, so segments from either engine can be concatenated.
const TTS_SAMPLE_RATE: u32 = 22050;

/// Which speech engine the Builder uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsEngine {
    /// Local Piper (free, offline). Used when its binary and model are installed.
    Piper,
    /// ElevenLabs API through the provider manager. Fallback when Piper is missing.
    ElevenLabs,
}

impl TtsEngine {
    /// Piper if its binary and model exist and the profile speaks the model's
    /// language (English); otherwise ElevenLabs (multilingual).
    pub fn detect(language: &str) -> Self {
        let installed = std::path::Path::new(PIPER_BIN).is_file()
            && std::path::Path::new(PIPER_MODEL).is_file();
        let engine = Self::choose(installed, language);

        // Log each distinct choice once, so it is visible without flooding the log.
        static LOGGED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let key = format!("{:?}/{}", engine, language);
        if let Ok(mut seen) = LOGGED.lock() {
            if !seen.contains(&key) {
                seen.push(key);
                match (engine, installed) {
                    (TtsEngine::Piper, _) => println!("🔊 [Builder] Speech: Piper ({})", PIPER_MODEL),
                    (TtsEngine::ElevenLabs, false) => println!(
                        "🔊 [Builder] Speech: Piper not found ({} / {}), using ElevenLabs", PIPER_BIN, PIPER_MODEL
                    ),
                    (TtsEngine::ElevenLabs, true) => println!(
                        "🔊 [Builder] Speech: Piper's model is English, profile language is {}, using ElevenLabs", language
                    ),
                }
            }
        }
        engine
    }

    fn choose(piper_installed: bool, language: &str) -> Self {
        if piper_installed && language.trim().eq_ignore_ascii_case(PIPER_LANGUAGE) {
            TtsEngine::Piper
        } else {
            TtsEngine::ElevenLabs
        }
    }
}

async fn text_to_audio(
    dialogues: Vec<Dialogue>,
    assets: ShotAssets,
    profile: &'static StudioProfile,
    config: Arc<config::Config>,
    providers: Arc<provider::ProviderManager>,
) -> Result<(), String> {
    let engine = TtsEngine::detect(profile.language);
    let final_path = assets.audio_path.clone();
    let root_path = assets.root_path.clone();

    let mut audio_paths: Vec<(usize, String)> =
        futures::stream::iter(dialogues.into_iter().enumerate())
            .map(|(i, dialogue)| {
                let segment_path = format!("{}/audio.part_{i}.wav", root_path);
                let config = config.clone();
                let providers = providers.clone();

                async move {
                    match engine {
                        TtsEngine::Piper => piper_tts(&dialogue.text, &segment_path).await?,
                        TtsEngine::ElevenLabs => {
                            // The speaker's own voice if the character file sets one.
                            let voice = profile
                                .character(&dialogue.character)
                                .and_then(|c| c.voice())
                                .unwrap_or(&config.voice.base_voice)
                                .to_string();
                            elevenlabs_tts(&dialogue.text, voice, &config, &providers, &segment_path).await?
                        }
                    }
                    Ok::<_, String>((i, segment_path))
                }
            })
            .buffer_unordered(TTS_CONCURRENCY)
            .try_collect()
            .await?;

    if audio_paths.is_empty() {
        let silent_path = format!("{}/audio.part_0.wav", root_path);

        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f", "lavfi",
                "-i", "anullsrc=r=22050:cl=mono",
                "-t", "3",
                "-acodec", "pcm_s16le",
                &silent_path,
            ])
            .status()
            .await
            .map_err(|e| format!("Create silent audio failed: {e}"))?;

        if !status.success() {
            return Err("Failed to create silent audio".into());
        }

        audio_paths.push((0, silent_path));
    }

    audio_paths.sort_by_key(|(i, _)| *i);
    
    let first = hound::WavReader::open(&audio_paths[0].1)
        .map_err(|e| e.to_string())?;

    let spec = first.spec();

    let tmp_path = helper::part_path(&final_path);
    let mut writer = hound::WavWriter::create(
        &tmp_path,
        spec,
    )
    .map_err(|e| e.to_string())?;

    for (_, path) in &audio_paths {
        let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
        if reader.spec() != spec {
            return Err(format!("{} has a different WAV format than the first segment", path));
        }

        for sample in reader.samples::<i16>() {
            writer
                .write_sample(
                    sample.map_err(|e| e.to_string())?
                )
                .map_err(|e| e.to_string())?;
        }
    }

    writer.finalize()
        .map_err(|e| e.to_string())?;
    helper::commit_part(&final_path).await?;

    for (_, path) in &audio_paths {
        let _ = tokio::fs::remove_file(path).await;
    }

    Ok(())
        
}

async fn get_audio_duration(path: &str) -> Result<f64, String> {
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            path,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(format!("ffprobe failed on {}: {}", path, String::from_utf8_lossy(&output.stderr)));
    }

    let s = String::from_utf8_lossy(&output.stdout);
    let duration = s.trim().parse::<f64>()
        .map_err(|e| format!("Parse duration {} --> {}", path, e))?;
    if !(duration.is_finite() && duration > 0.0) {
        return Err(format!("Audio {} has no length", path));
    }
    Ok(duration)
}
async fn piper_tts(text: &str, segment_path: &str) -> Result<(), String> {
    let mut child = tokio::process::Command::new(PIPER_BIN)
        .args(["--model", PIPER_MODEL, "--output_file", segment_path])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Piper spawn failed: {e}"))?;

    // Dropping stdin at the end of this block closes it, so Piper sees EOF.
    if let Some(mut stdin) = child.stdin.take() {
        tokio::io::AsyncWriteExt::write_all(&mut stdin, text.as_bytes())
            .await
            .map_err(|e| format!("Piper stdin failed: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Piper wait failed: {e}"))?;

    if !output.status.success() {
        return Err(format!("Piper synthesis failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

async fn elevenlabs_tts(
    text: &str,
    voice_id: String,
    config: &config::Config,
    providers: &provider::ProviderManager,
    segment_path: &str,
) -> Result<(), String> {
    let req = provider::ProviderRequest::Audio(provider::AudioRequest {
        text: text.to_string(),
        voice_id: Some(voice_id),
        language: None,
        speed: Some(config.tts.speed),
        stability: Some(config.tts.stability),
        similarity_boost: Some(config.tts.similarity_boost),
        format: Some(provider::AudioFormat::Wav { sample_rate: TTS_SAMPLE_RATE }),
    });

    let guard = providers
        .acquire(&provider::Provider::ElevenLabs)
        .await
        .ok_or_else(|| "ElevenLabs unavailable: Piper is not installed and no ELEVEN_LABS_KEY_n is set (or all keys are blocked)".to_string())?;

    let wav = guard
        .call(req)
        .await
        .map_err(|e| e.to_string())?
        .into_bytes()
        .map_err(|e| e.to_string())?;

    if !wav.starts_with(b"RIFF") {
        return Err(format!("ElevenLabs returned no WAV audio: {}", helper::preview(&wav)));
    }
    helper::write_atomic(segment_path, &wav).await
}

/// Deterministic 32-bit seed from a string (FNV-1a), stable across runs and Rust versions.
pub fn stable_seed(s: &str) -> u32 {
    s.bytes().fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_engine_follows_install_and_language() {
        assert_eq!(TtsEngine::choose(true, "English"), TtsEngine::Piper);
        assert_eq!(TtsEngine::choose(true, "english "), TtsEngine::Piper);
        assert_eq!(TtsEngine::choose(true, "Vietnamese"), TtsEngine::ElevenLabs);
        assert_eq!(TtsEngine::choose(false, "English"), TtsEngine::ElevenLabs);
    }

    #[test]
    fn seed_is_stable() {
        assert_eq!(stable_seed("abc"), stable_seed("abc"));
        assert_ne!(stable_seed("abc"), stable_seed("abd"));
        assert_eq!(stable_seed(""), 0x811c9dc5);
    }
}
