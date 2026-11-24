use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use reqwest::Client;

use crate::api::*;
use crate::config::CONFIG;
use crate::enums::*;
use crate::helper::*;
use crate::models::*;

pub async fn build_content(client: &Client, system: &str, user: &str, is_json: bool) -> Result<String, String> {
    gemini::generate_script(client, system, user, is_json).await
}

pub async fn build_timelines(client: &Client, target_path: String, scenes: Vec<Scene>, voice_mode: VoiceMode) -> Result<Vec<VideoTimeline>, String> {
    let audio_root = format!("{}/audios", target_path);
    let visual_root = format!("{}/images", target_path);

    tokio::fs::create_dir_all(&visual_root)
        .await
        .map_err(|e| e.to_string())?;

    tokio::fs::create_dir_all(&audio_root)
        .await
        .map_err(|e| e.to_string())?;

    let mut handles = Vec::new();
    let tts_semaphore = Arc::new(tokio::sync::Semaphore::new(2)); // ElevenLabs free max 2 concurrent

    for scene in scenes {
        let client = client.clone();
        let voice_mode = voice_mode.clone();
        let audio_root = audio_root.clone();
        let visual_root = visual_root.clone();

        let semaphore = tts_semaphore.clone();

        handles.push(tokio::spawn(async move {
            let _permit = semaphore.acquire().await.map_err(|e| e.to_string())?;

            let visual_path = format!("{}/scene_{}.png", visual_root, scene.scene_id);
            let audio_path = format!("{}/scene_{}.mp3", audio_root, scene.scene_id);

            // Skip assets that already exist so a retry doesn't re-spend API credits.
            if tokio::fs::try_exists(&visual_path).await.unwrap_or(false) {
                println!("🔧[Builder] Scene {} image already exists", scene.scene_id);
            } else {
                generate_scene_visual(
                    &client,
                    &visual_path,
                    &scene.visual_prompt,
                ).await?;
            }

            if tokio::fs::try_exists(&audio_path).await.unwrap_or(false) {
                println!("🔧[Builder] Scene {} audio already exists", scene.scene_id);
            } else {
                generate_scene_audio(
                    &client,
                    &audio_path,
                    &voice_mode,
                    &scene.voice_segments,
                ).await?;
            }

            let duration = get_audio_duration(&audio_path).await?;

            Ok::<_, String>(VideoTimeline {
                scene_id: scene.scene_id,
                transition: scene.transition,
                motion: scene.motion,
                audio_path,
                visual_path,
                duration,
                start_time: 0.0,
                end_time: 0.0,
            })
        }));
    }

    let mut assets = Vec::new();

    for handle in handles {
        assets.push(
            handle
                .await
                .map_err(|e| e.to_string())??
        );
    }

    assets.sort_by_key(|v| v.scene_id);

    let mut cursor = 0.0;
    let mut timelines = Vec::new();

    for asset in assets {
        let start_time = cursor;
        let end_time = start_time + asset.duration;

        timelines.push(VideoTimeline {
            start_time,
            end_time,
            ..asset
        });

        cursor = end_time;
    }

    Ok(timelines)
}

/// Length of the cross-fade between clip `a` and clip `b`: `Transition::DURATION`,
/// shortened for very short clips so `xfade` never needs more frames than a clip has.
fn fade_duration(a: f64, b: f64) -> f64 {
    Transition::DURATION.min(a / 2.0).min(b / 2.0)
}

pub async fn ffmpeg_render(target_path: String, timelines: &[VideoTimeline]) -> Result<String, String> {
    ffmpeg_check()?;

    if timelines.is_empty() {
        return Err("Nothing to render: no scenes".to_string());
    }

    let final_path = format!("{}/final_video.mp4", &target_path);

    if tokio::fs::try_exists(&final_path).await.unwrap_or(false) {
        println!("🎬 FFmpeg already rendered");
        return Ok(final_path);
    }

    let video = &CONFIG.video;
    let mut video_paths = vec![];

    for timeline in timelines {
        let video_path = format!("{}/videos/scene_{}.mp4", target_path, timeline.scene_id);

        if tokio::fs::try_exists(&video_path).await.unwrap_or(false) {
            println!("🎬 [FFmpeg] Scene {} already rendered", timeline.scene_id);
        } else {
            println!("🎬 [FFmpeg] Starting render scene {}", timeline.scene_id);

            // Check input files
            if !tokio::fs::try_exists(&timeline.visual_path).await.unwrap_or(false) {
                return Err(format!("Missing visual file: {}", timeline.visual_path));
            }

            if !tokio::fs::try_exists(&timeline.audio_path).await.unwrap_or(false) {
                return Err(format!("Missing audio file: {}", timeline.audio_path));
            }

            // Create output directory
            if let Some(parent) = Path::new(&video_path).parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("Failed to create output dir: {}", e))?;
            }

            // Scale/crop the still to the output frame, then animate it. Every clip gets
            // the same size, fps, pixel format and audio layout, which `xfade` requires.
            let frames = (timeline.duration * video.fps as f64).ceil() as u32;
            let motion = Motion::from_str(&timeline.motion).unwrap_or(Motion::None);
            let filter = format!(
                "scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,{zoom},format=yuv420p",
                w = video.width,
                h = video.height,
                zoom = motion.ffmpeg_filter(frames, video.width, video.height, video.fps),
            );
            let duration = format!("{:.3}", timeline.duration);
            let tmp_path = format!("{}.part", video_path);

            run_ffmpeg(&[
                "-y",
                "-i", &timeline.visual_path,
                "-i", &timeline.audio_path,
                "-filter_complex", &format!("[0:v]{}[v]", filter),
                "-map", "[v]",
                "-map", "1:a",
                "-t", &duration,
                "-c:v", "libx264",
                "-r", &video.fps.to_string(),
                "-c:a", "aac",
                "-ar", "44100",
                "-ac", "2",
                "-f", "mp4",
                &tmp_path,
            ]).await.map_err(|e| format!("Scene {}: {}", timeline.scene_id, e))?;

            tokio::fs::rename(&tmp_path, &video_path)
                .await
                .map_err(|e| e.to_string())?;
        }

        video_paths.push(video_path);
    }

    println!("🎬 [FFmpeg] Starting render video {}", final_path);
    let tmp_final = format!("{}.part", final_path);

    if video_paths.len() == 1 {
        // Nothing to join: re-mux the single scene with faststart.
        run_ffmpeg(&[
            "-y",
            "-i", &video_paths[0],
            "-c", "copy",
            "-movflags", "+faststart",
            "-f", "mp4",
            &tmp_final,
        ]).await?;
    } else {
        let mut filter_parts = Vec::new();
        // Length of the clips merged so far.
        let mut merged = timelines[0].duration;

        for i in 1..timelines.len() {
            let fade = fade_duration(timelines[i - 1].duration, timelines[i].duration);
            let offset = merged - fade;
            let transition = Transition::from_str(&timelines[i - 1].transition)
                .unwrap_or(Transition::DEFAULT);

            let prev_v = if i == 1 { "[0:v]".to_string() } else { format!("[v{}]", i - 1) };
            let prev_a = if i == 1 { "[0:a]".to_string() } else { format!("[a{}]", i - 1) };

            filter_parts.push(format!(
                "{}[{}:v]xfade=transition={}:duration={:.3}:offset={:.3}[v{}]",
                prev_v, i, transition.ffmpeg_name(), fade, offset, i
            ));
            filter_parts.push(format!(
                "{}[{}:a]acrossfade=d={:.3}[a{}]",
                prev_a, i, fade, i
            ));

            merged += timelines[i].duration - fade;
        }

        let stream_video = format!("[v{}]", timelines.len() - 1);
        let stream_audio = format!("[a{}]", timelines.len() - 1);
        let filter = filter_parts.join(";");

        let mut args: Vec<&str> = vec!["-y"];
        for video_path in &video_paths {
            args.extend(["-i", video_path.as_str()]);
        }
        args.extend([
            "-filter_complex", &filter,
            "-map", &stream_video,
            "-map", &stream_audio,
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac",
            "-movflags", "+faststart",
            "-f", "mp4",
            &tmp_final,
        ]);
        run_ffmpeg(&args).await?;
    }

    tokio::fs::rename(&tmp_final, &final_path)
        .await
        .map_err(|e| e.to_string())?;

    println!("🎬 [FFmpeg] Rendered successfully");
    Ok(final_path)
}

async fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let output = tokio::process::Command::new("ffmpeg")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to start ffmpeg: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "FFmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

async fn generate_scene_visual(
    client: &Client,
    output_path: &str,
    prompt: &str,
) -> Result<(), String> {
    let image: Vec<u8> = cloudflare::generate_image(client, prompt).await?;

    expect_media(&image, MediaType::Image, "image")?;
    write_atomic(output_path, &image).await
}

async fn generate_scene_audio(
    client: &Client,
    output_path: &str,
    voice_mode: &VoiceMode,
    voice_segments: &[VoiceSegment],
) -> Result<(), String> {
    let first = voice_segments
        .first()
        .ok_or_else(|| format!("No voice segments for {}", output_path))?;

    match voice_mode {
        VoiceMode::PerSegment => {
            let dir = Path::new(output_path).parent().unwrap_or(Path::new("."));
            let stem = Path::new(output_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("audio");
            let mut segment_names: Vec<String> = Vec::new();

            for (i, segment) in voice_segments.iter().enumerate() {
                let audio = elevenlabs::generate_tts(
                    client,
                    &segment.text,
                    segment.voice_id.as_deref().unwrap_or(CONFIG.voice.base_voice),
                )
                .await?;

                expect_media(&audio, MediaType::Audio, "audio")?;

                let name = format!("{}.seg_{}.mp3", stem, i);
                write_atomic(&dir.join(&name).to_string_lossy(), &audio).await?;
                segment_names.push(name);
            }

            // ffmpeg resolves concat entries relative to the list file, so list bare
            // file names and keep the list in the same folder.
            let list_path = format!("{}.list", output_path);
            let list_content = segment_names
                .iter()
                .map(|n| format!("file '{}'\n", n.replace('\'', "'\\''")))
                .collect::<String>();

            tokio::fs::write(&list_path, list_content)
                .await
                .map_err(|e| e.to_string())?;

            let tmp_path = format!("{}.part", output_path);
            run_ffmpeg(&[
                "-y",
                "-f", "concat",
                "-safe", "0",
                "-i", &list_path,
                "-c", "copy",
                "-f", "mp3",
                &tmp_path,
            ]).await.map_err(|e| format!("Failed to merge audio for {}: {}", output_path, e))?;

            tokio::fs::rename(&tmp_path, output_path)
                .await
                .map_err(|e| e.to_string())?;

            let _ = tokio::fs::remove_file(&list_path).await;
            for name in &segment_names {
                let _ = tokio::fs::remove_file(dir.join(name)).await;
            }
        }

        VoiceMode::SingleVoice => {
            let text = voice_segments
                .iter()
                .map(|v| v.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let audio = elevenlabs::generate_tts(
                client,
                &text,
                first.voice_id.as_deref().unwrap_or(CONFIG.voice.base_voice),
            )
            .await?;

            expect_media(&audio, MediaType::Audio, "audio")?;
            write_atomic(output_path, &audio).await?;
        }
    }

    Ok(())
}

/// Both ffmpeg and ffprobe are needed (ffprobe measures scene length).
fn ffmpeg_check() -> Result<(), String> {
    for tool in ["ffmpeg", "ffprobe"] {
        let ok = std::process::Command::new(tool)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return Err(format!("{} not found in PATH. Please install ffmpeg or set PATH correctly.", tool));
        }
    }
    Ok(())
}

async fn get_audio_duration(path: &str) -> Result<f64, String> {
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to start ffprobe: {}", e))?;

    if !output.status.success() {
        return Err(format!("ffprobe failed on {}: {}", path, String::from_utf8_lossy(&output.stderr)));
    }

    let duration = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("ffprobe gave no duration for {}: {}", path, e))?;

    if !(duration.is_finite() && duration > 0.0) {
        return Err(format!("Audio {} has no length ({})", path, duration));
    }

    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders real clips with ffmpeg from generated assets (no API calls).
    /// Run with `cargo test -- --ignored` (needs ffmpeg and ffprobe on PATH).
    #[tokio::test]
    #[ignore]
    async fn renders_mixed_motions_short_scenes_and_single_scene() {
        let root = std::env::temp_dir().join(format!("video_studio_render_{}", uuid::Uuid::new_v4()));
        let root_str = root.to_string_lossy().to_string();
        tokio::fs::create_dir_all(root.join("images")).await.unwrap();
        tokio::fs::create_dir_all(root.join("audios")).await.unwrap();

        // Square 1024x1024 stills (like flux-1-schnell) and tones of different lengths.
        let scenes = [(1, "None", "Fade", 2.0), (2, "PanLeft", "SlideLeft", 0.6), (3, "ZoomIn", "Bogus", 2.5)];
        let mut timelines = vec![];
        for (id, motion, transition, secs) in scenes {
            let img = format!("{}/images/scene_{}.png", root_str, id);
            let mp3 = format!("{}/audios/scene_{}.mp3", root_str, id);
            run_ffmpeg(&["-y", "-f", "lavfi", "-i", "testsrc=size=1024x1024", "-frames:v", "1", &img]).await.unwrap();
            run_ffmpeg(&["-y", "-f", "lavfi", "-i", &format!("sine=frequency=440:duration={}", secs), &mp3]).await.unwrap();
            let duration = get_audio_duration(&mp3).await.unwrap();
            timelines.push(VideoTimeline {
                scene_id: id, audio_path: mp3, visual_path: img,
                start_time: 0.0, end_time: 0.0, duration,
                transition: transition.into(), motion: motion.into(),
            });
        }

        let final_path = ffmpeg_render(root_str.clone(), &timelines).await.unwrap();
        let total = get_audio_duration(&final_path).await.unwrap();
        let expected: f64 = timelines.iter().map(|t| t.duration).sum::<f64>()
            - fade_duration(timelines[0].duration, timelines[1].duration)
            - fade_duration(timelines[1].duration, timelines[2].duration);
        assert!((total - expected).abs() < 0.3, "final {}s, expected ~{}s", total, expected);

        let probe = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", &final_path])
            .output().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), format!("{},{}", CONFIG.video.width, CONFIG.video.height));

        // A one-scene video must render too.
        let single_root = format!("{}/single", root_str);
        let single = ffmpeg_render(single_root, &timelines[..1]).await.unwrap();
        assert!(get_audio_duration(&single).await.unwrap() > 1.5);

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[test]
    fn fade_never_exceeds_half_a_clip() {
        assert_eq!(fade_duration(5.0, 6.0), Transition::DURATION);
        assert_eq!(fade_duration(0.8, 6.0), 0.4);
        assert_eq!(fade_duration(6.0, 1.0), 0.5);
    }
}
