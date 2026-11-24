---
name: renderer-agent
description: Work on the Renderer stage — FFmpeg scene rendering, motion (zoompan) filters, xfade transitions, and final concat. Use when a render fails, when adding a Motion/Transition variant, or when changing video format/resolution.
---

# Renderer Agent

**Code:** `ffmpeg_render` in `src/renderer.rs`. Filters are in `Motion::ffmpeg_filter` and `Transition::ffmpeg_name` in `src/enums.rs`.

## Contract
- Input: `state.video_timelines` (paths, duration, motion, transition per scene).
- Output: `{target_path}/videos/scene_N.mp4`, then `{target_path}/final_video.mp4`. Sets `state.video_path` and moves to `Publisher`.

## Idempotency
- Scenes whose `videos/scene_N.mp4` exists are skipped. If `final_video.mp4` exists, the whole stage is skipped.
- To force a re-render after changing filters, delete those files in the session folder (look at them first).

## Adding a Motion or Transition
1. Add the variant to the enum **and** to its `ALL`, `Display`, and `FromStr` in `src/enums.rs`.
2. Motion: add the `zoompan` parameters in `ffmpeg_filter(frames, w, h, fps)`. Every motion must return a filter with `s=WxH:fps=F` (the function appends them), so all clips share size and fps. Use the `FRAMES` placeholder for expressions that should span the whole clip (pans).
3. Transition: map it to a valid ffmpeg `xfade` transition name in `ffmpeg_name()`. `Transition::DURATION` is the overlap in seconds.
4. The Writer prompt picks it up automatically from `ALL`.

## How a scene is rendered
- The still is scaled and center-cropped to `CONFIG.video` (1080x1920), then animated with `zoompan` for `ceil(duration * fps)` frames. Audio is normalized to AAC 44.1 kHz stereo.
- Clips are joined with `xfade` + `acrossfade`. The fade length is `fade_duration(a, b)`: `Transition::DURATION`, shortened to half of the shorter clip. A single scene is re-muxed instead of joined.
- Outputs go to `<file>.part` and are renamed when ffmpeg succeeds.

## Debugging a failed render
- Run the failing ffmpeg command by hand against a real session folder under `output/`. Copy the args from `ffmpeg_render`.
- Check that inputs exist (`visual_path`, `audio_path`) and that `ffprobe` gives a duration for every audio file.
- xfade needs each clip to be longer than `Transition::DURATION`, and all inputs must share resolution, fps, and pixel format.
- For quick experiments: `APP_DEBUG=True` plus `state.revert(AgentNode::Renderer)` (commented out in `scheduler.rs`) re-runs only the render stage.

Verify with `cargo check` and `cargo test -- --ignored` (renders real clips from generated assets, no API calls). Don't use a full pipeline run to test rendering.
