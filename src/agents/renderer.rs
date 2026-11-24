use std::sync::Arc;
use std::path;
use async_trait::async_trait;

use crate::AppState;
use crate::agent::*;
use crate::config::VideoConfig;
use crate::helper;
use crate::models::*;
use crate::entities::*;

pub struct RendererAgent;

#[async_trait]
impl Agent for RendererAgent {
    async fn run(&self, state: &Arc<AppState>, job: &Job) -> Result<(), AgentError> {
        println!("🎥 [Renderer] Rendering video...");

        let timeline: Timeline =
            serde_json::from_str(&job.payload)
            .map_err(|e| AgentError::Decode(e.to_string()))?;

        let video_metadata: VideoMetadata = self
            .execute(&timeline, &state.config.video)
            .await
            .map_err(|e| AgentError::Execute(e.to_string()))?;

        let payload =
            serde_json::to_string(&video_metadata)
            .map_err(|e| AgentError::Encode(e.to_string()))?;

        state.services.db
            .handoff_job(job, AgentType::Publisher, payload)
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        Ok(())
    }
}

impl RendererAgent {
    async fn execute(
        &self,
        timeline: &Timeline,
        video: &VideoConfig,
    ) -> Result<VideoMetadata, String> {
        ffmpeg_check()?;

        if timeline.clips.is_empty() {
            return Err("Nothing to render: timeline has no clips".to_string());
        }

        let metadata = VideoMetadata {
            title: timeline.title.clone(),
            video_path: timeline.final_path.clone(),
        };

        if tokio::fs::try_exists(&timeline.final_path).await.unwrap_or(false) {
            println!("🎬 [FFmpeg] Final video already rendered");
            return Ok(metadata);
        }

        // 1. One clip per shot.
        for clip in timeline.clips.iter() {
            if tokio::fs::try_exists(&clip.video_path).await.unwrap_or(false) {
                println!("🎬 [FFmpeg] Shot number {} already rendered", clip.shot_id);
                continue;
            }
            render_clip(clip, video).await
                .map_err(|e| format!("Shot {}: {}", clip.shot_id, e))?;
        }

        // 2. Join the clips with transitions.
        println!("🎬 [FFmpeg] Starting render video {}", timeline.video_path);
        join_clips(timeline).await?;

        // 3. Subtitles: always written as a sidecar .ass, burned in if configured.
        println!("🎬 [FFmpeg] Starting render subtitle {}", timeline.subtitle_path);
        tokio::fs::write(&timeline.subtitle_path, build_ass(timeline, video))
            .await
            .map_err(|e| format!("write {}: {}", timeline.subtitle_path, e))?;

        let tmp = helper::part_path(&timeline.final_path);
        if !video.burn_subtitles {
            run_ffmpeg(&[
                "-y", "-i", &timeline.video_path,
                "-c", "copy", "-movflags", "+faststart", "-f", "mp4", &tmp,
            ]).await?;
            helper::commit_part(&timeline.final_path).await?;
            println!("🎬 [FFmpeg] Rendered successfully (subtitles not burned in)");
            return Ok(metadata);
        }

        run_ffmpeg(&[
            "-y",
            "-i", &timeline.video_path,
            "-vf", &format!("subtitles=filename='{}'", escape_filter_path(&timeline.subtitle_path)),
            "-movflags", "+faststart",
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-c:a", "copy",
            "-f", "mp4",
            &tmp,
        ]).await?;
        helper::commit_part(&timeline.final_path).await?;

        println!("🎬 [FFmpeg] Rendered successfully");
        Ok(metadata)
    }
}

/// Render one still + its speech into a clip of `clip.duration` seconds.
/// The image is scaled/center-cropped to the output frame, so every clip shares
/// size, fps, pixel format and audio layout (required by `xfade`/`acrossfade`).
/// Speech starts after half the transition and the rest is padded with silence,
/// so cross-fades never cut into a sentence.
async fn render_clip(clip: &Clip, video: &VideoConfig) -> Result<(), String> {
    if !tokio::fs::try_exists(&clip.visual_path).await.unwrap_or(false) {
        return Err(format!("Missing visual file: {}", clip.visual_path));
    }
    if !tokio::fs::try_exists(&clip.audio_path).await.unwrap_or(false) {
        return Err(format!("Missing audio file: {}", clip.audio_path));
    }
    if let Some(parent) = path::Path::new(&clip.video_path).parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }

    let frames = (clip.duration * video.fps as f64).ceil() as u32;
    let delay_ms = ((Transition::DURATION / 2.0) * 1000.0).round() as u64;
    let filter = format!(
        "[0:v]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,{zoom},format=yuv420p[v];\
         [1:a]aformat=sample_rates=44100:channel_layouts=stereo,adelay={d}|{d},apad[a]",
        w = video.width,
        h = video.height,
        zoom = clip.motion.ffmpeg_filter(frames, video.width, video.height, video.fps),
        d = delay_ms,
    );
    let duration = format!("{:.3}", clip.duration);
    let fps = video.fps.to_string();
    let tmp = helper::part_path(&clip.video_path);

    run_ffmpeg(&[
        "-y",
        "-i", &clip.visual_path,
        "-i", &clip.audio_path,
        "-filter_complex", &filter,
        "-map", "[v]",
        "-map", "[a]",
        "-t", &duration,
        "-r", &fps,
        "-c:v", "libx264",
        "-c:a", "aac",
        "-f", "mp4",
        &tmp,
    ]).await?;
    helper::commit_part(&clip.video_path).await
}

/// Join all clips into `timeline.video_path` with `xfade` + `acrossfade`.
async fn join_clips(timeline: &Timeline) -> Result<(), String> {
    let tmp = helper::part_path(&timeline.video_path);
    let clips = &timeline.clips;

    if clips.len() == 1 {
        // Nothing to join: re-mux the single clip.
        run_ffmpeg(&[
            "-y", "-i", &clips[0].video_path, "-c", "copy", "-f", "mp4", &tmp,
        ]).await?;
        return helper::commit_part(&timeline.video_path).await;
    }

    let mut filter_parts = Vec::new();
    // Length of the clips merged so far.
    let mut merged = clips[0].duration;

    for i in 1..clips.len() {
        let fade = Transition::fade_between(clips[i - 1].duration, clips[i].duration);
        let offset = merged - fade;
        let prev_v = if i == 1 { "[0:v]".to_string() } else { format!("[v{}]", i - 1) };
        let prev_a = if i == 1 { "[0:a]".to_string() } else { format!("[a{}]", i - 1) };

        filter_parts.push(format!(
            "{}[{}:v]xfade=transition={}:duration={:.3}:offset={:.3}[v{}]",
            prev_v, i, clips[i - 1].transition.ffmpeg_name(), fade, offset, i
        ));
        filter_parts.push(format!("{}[{}:a]acrossfade=d={:.3}[a{}]", prev_a, i, fade, i));

        merged += clips[i].duration - fade;
    }

    let filter = filter_parts.join(";");
    let map_v = format!("[v{}]", clips.len() - 1);
    let map_a = format!("[a{}]", clips.len() - 1);

    let mut args: Vec<&str> = vec!["-y"];
    for clip in clips {
        args.extend(["-i", clip.video_path.as_str()]);
    }
    args.extend([
        "-filter_complex", &filter,
        "-map", &map_v,
        "-map", &map_a,
        "-c:v", "libx264",
        "-pix_fmt", "yuv420p",
        "-c:a", "aac",
        "-f", "mp4",
        &tmp,
    ]);
    run_ffmpeg(&args).await?;
    helper::commit_part(&timeline.video_path).await
}

/// ASS subtitles: one line per clip, shown while its speech plays.
fn build_ass(timeline: &Timeline, video: &VideoConfig) -> String {
    // No indentation: libass expects section headers and keys at line start.
    let mut ass = format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {}\n\
         PlayResY: {}\n\
         WrapStyle: 2\n\
         ScaledBorderAndShadow: yes\n\
         \n\
         [V4+ Styles]\n\
         Format: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\n\
         Style: Default,Arial,{},&H00FFFFFF,&H0000FFFF,&H00000000,&H80000000,1,0,0,0,100,100,0,0,1,5,3,2,60,60,{},1\n\
         \n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        video.width,
        video.height,
        video.height / 19,  // ~100 px at 1920
        video.height / 14,  // bottom margin ~140 px at 1920
    );

    for clip in &timeline.clips {
        if !clip.subtitle.trim().is_empty() {
            ass.push_str(&format!(
                "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
                ass_time(clip.start_time),
                ass_time(clip.end_time),
                escape_ass(&clip.subtitle),
            ));
        }
    }
    ass
}

async fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let output = tokio::process::Command::new("ffmpeg")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to start ffmpeg: {}", e))?;

    if !output.status.success() {
        return Err(format!("FFmpeg failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

/// Both ffmpeg and ffprobe are needed (ffprobe measures shot length).
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

fn ass_time(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 100.0).round() as u64;

    let cs = total % 100;
    let sec = (total / 100) % 60;
    let min = (total / 6000) % 60;
    let hour = total / 360000;

    format!("{hour}:{min:02}:{sec:02}.{cs:02}")
}

fn escape_ass(text: &str) -> String {
    text.replace('\n', "\\N")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

/// Escape a path for use inside a quoted ffmpeg filter argument.
fn escape_filter_path(path: &str) -> String {
    path.replace('\\', "\\\\").replace('\'', "\\'").replace(':', "\\:")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: VideoConfig = VideoConfig { width: 1080, height: 1920, fps: 25, burn_subtitles: true };

    #[test]
    fn ass_header_is_not_indented() {
        let tl = Timeline {
            title: "t".into(), final_path: "f".into(), video_path: "v".into(), subtitle_path: "s".into(),
            clips: vec![],
        };
        let ass = build_ass(&tl, &VIDEO);
        assert!(ass.lines().all(|l| !l.starts_with(' ')));
        assert!(ass.contains("PlayResY: 1920"));
    }

    #[test]
    fn ass_time_formats() {
        assert_eq!(ass_time(0.0), "0:00:00.00");
        assert_eq!(ass_time(61.25), "0:01:01.25");
    }

    /// Renders real clips with ffmpeg from generated assets (no API calls).
    /// Run with `cargo test -- --ignored` (needs ffmpeg and ffprobe on PATH).
    #[tokio::test]
    #[ignore]
    async fn renders_mixed_motions_single_clip_and_subtitles() {
        let root = std::env::temp_dir().join(format!("video_studio_render_{}", uuid::Uuid::new_v4()));
        let root_str = root.to_string_lossy().to_string();
        tokio::fs::create_dir_all(&root).await.unwrap();

        let pad = Transition::DURATION;
        let specs = [(Motion::None, Transition::Fade, 2.0), (Motion::PanLeft, Transition::SlideLeft, 0.6), (Motion::ZoomIn, Transition::Fade, 2.5)];
        let mut clips = vec![];
        let mut clip_start = 0.0;
        let mut prev: Option<f64> = None;
        let mut expected = 0.0;
        for (i, (motion, transition, secs)) in specs.into_iter().enumerate() {
            let id = i as u32 + 1;
            let img = format!("{}/img_{}.png", root_str, id);
            let wav = format!("{}/aud_{}.wav", root_str, id);
            // Square stills (like flux-1-schnell) and mono 22 kHz speech (like Piper).
            run_ffmpeg(&["-y", "-f", "lavfi", "-i", "testsrc=size=1024x1024", "-frames:v", "1", &img]).await.unwrap();
            run_ffmpeg(&["-y", "-f", "lavfi", "-i", &format!("sine=frequency=440:duration={}:sample_rate=22050", secs), &wav]).await.unwrap();
            let duration = secs + pad;
            let fade = prev.map(|p| Transition::fade_between(p, duration)).unwrap_or(0.0);
            clip_start -= fade;
            clips.push(Clip {
                shot_id: id, audio_path: wav, visual_path: img,
                video_path: format!("{}/clip_{}.mp4", root_str, id),
                start_time: clip_start + pad / 2.0, end_time: clip_start + pad / 2.0 + secs,
                duration, subtitle: format!("Line {}", id), acrossfade: fade, transition, motion,
            });
            clip_start += duration;
            expected = clip_start;
            prev = Some(duration);
        }

        let timeline = Timeline {
            title: "Test".into(),
            final_path: format!("{}/final.mp4", root_str),
            video_path: format!("{}/video.mp4", root_str),
            subtitle_path: format!("{}/subtitle.ass", root_str),
            clips,
        };
        let meta = RendererAgent.execute(&timeline, &VIDEO).await.unwrap();

        let probe = |path: String, entries: &'static str, stream: &'static str| async move {
            let out = tokio::process::Command::new("ffprobe")
                .args(["-v", "error", "-select_streams", stream, "-show_entries", entries, "-of", "csv=p=0", &path])
                .output().await.unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(probe(meta.video_path.clone(), "stream=width,height", "v:0").await, "1080,1920");
        let total: f64 = probe(meta.video_path.clone(), "format=duration", "v:0").await.parse().unwrap();
        assert!((total - expected).abs() < 0.3, "final {}s, expected ~{}s", total, expected);

        // A one-clip video must render too.
        let single = Timeline {
            final_path: format!("{}/single_final.mp4", root_str),
            video_path: format!("{}/single_video.mp4", root_str),
            clips: timeline.clips[..1].to_vec(),
            ..timeline.clone()
        };
        RendererAgent.execute(&single, &VIDEO).await.unwrap();

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
