---
name: renderer-agent
description: Work on the Renderer agent (src/agents/renderer.rs): FFmpeg clips, zoompan motion, xfade joins, ASS subtitles. Use when changing the video look, transitions, subtitles or fixing FFmpeg errors.
---

# Renderer agent

**Code**: `src/agents/renderer.rs`; filters in `Motion::ffmpeg_filter` and `Transition` (`src/entities/storyboard.rs`). **Pool**: 1.

## Contract
- Input: `Timeline`. Output (handoff to Publisher): `FinalCut { title, video_path }` where `video_path` is `final_video.mp4`.
- Needs `ffmpeg` and `ffprobe` on PATH (`ffmpeg_check`). Empty timeline is an error.

## How it works
1. Skip everything if `final_video.mp4` exists.
2. `render_clip` per shot (skips existing `shot_N/video.mp4`): scale + center-crop to `config.video` (1080x1920 @ 25 fps), `zoompan` per `Motion`, speech `adelay` half the transition + `apad`, encode to `.part`, rename.
3. `join_clips`: one clip is re-muxed; otherwise `xfade` (previous clip's transition) + `acrossfade` with `Transition::fade_between`.
4. `build_ass` writes `subtitle.ass`; burn in with `subtitles=` filter into `final_video.mp4.part`, rename.

## Gotchas
- Every clip must share size, fps, pixel format and audio layout or `xfade` fails: keep the scale/crop and `format=yuv420p` / `aformat` in `render_clip`.
- Keep `fade_between` in sync with the Builder's subtitle timing.
- ASS header lines must not be indented (libass).
- Escape paths in filters with `escape_filter_path`.
- Delete `shot_N/video.mp4` or `final_video.mp4` to force a re-render.

## Verify
`cargo check`, `cargo test`, and `cargo test -- --ignored` for a real FFmpeg render on generated assets (no API calls, needs ffmpeg and ffprobe).
