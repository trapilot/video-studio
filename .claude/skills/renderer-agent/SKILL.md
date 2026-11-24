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
2. Motion: return a `zoompan` (or other) filter string from `ffmpeg_filter(duration)`. Frames = `duration * 25`.
3. Transition: map it to a valid ffmpeg `xfade` transition name in `ffmpeg_name()`. `Transition::DURATION` is the overlap in seconds.
4. The Writer prompt picks it up automatically from `ALL`.

## Debugging a failed render
- Run the failing ffmpeg command by hand against a real session folder under `output/`. Copy the args from `ffmpeg_render`.
- Check that inputs exist (`visual_path`, `audio_path`) and that `ffprobe` gives a duration for every audio file.
- xfade needs each clip to be longer than `Transition::DURATION`, and all inputs must share resolution, fps, and pixel format.
- For quick experiments: `APP_DEBUG=True` plus `state.revert(AgentNode::Renderer)` (commented out in `scheduler.rs`) re-runs only the render stage.

Verify with `cargo check`. Test renders locally on existing assets, not with a full pipeline run.
