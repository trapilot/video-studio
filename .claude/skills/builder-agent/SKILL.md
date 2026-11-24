---
name: builder-agent
description: Work on the Builder stage — generating per-scene images and TTS audio and producing VideoTimeline entries. Use when changing image/voice generation, concurrency, voice mode, or asset paths.
---

# Builder Agent

**Code:** `AgentNode::Builder` arm in `src/workflow.rs` calls `build_timelines` in `src/renderer.rs`, which uses `generate_scene_visual` and `generate_scene_audio`.

## Contract
- Input: `state.final_json.scenes`, `state.target_path`, `VoiceMode`.
- Output: writes `{target_path}/images/scene_N.png` and `{target_path}/audios/scene_N.mp3`, sets `state.video_timelines`, then moves to `Renderer`.
- Scene duration comes from the **audio length** (`ffprobe`), not from `scene.duration` in the script.

## Providers
- Image: `cloudflare::generate_image` (change it in `generate_scene_visual`).
- TTS: `elevenlabs::generate_tts` (change it in `generate_scene_audio`).
- To swap or add a provider, see the `add-provider` skill. Don't call `api::*` from `workflow.rs`.

## Voice modes (`enums::VoiceMode`)
- `SingleVoice` (current default): joins all segment texts and voices them with the first segment's `voice_id`.
- `PerSegment`: one TTS call per segment, saved as `scene_N.mp3.tmp_i`, then concatenated with `ffmpeg -f concat`.

## Gotchas
- Scenes run concurrently, but a shared `Semaphore::new(2)` caps concurrency at 2 (ElevenLabs free tier). It covers both image and audio work.
- Always run `detect_media` on returned bytes. Providers sometimes return HTML/JSON errors with a 200 status.
- Assets that already exist on disk are skipped, so retries only generate what's missing. Delete a file to force it to regenerate (for example after changing a prompt).
- The arm saves state after it finishes, so a resume starts at the Renderer.

Verify with `cargo check`.
