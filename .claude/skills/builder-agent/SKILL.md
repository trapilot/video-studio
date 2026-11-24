---
name: builder-agent
description: Work on the Builder agent (src/agents/builder.rs): per-shot image and speech generation (Cloudflare, Piper or ElevenLabs), asset layout, timeline and subtitle timing. Use when changing assets, TTS or timing.
---

# Builder agent

**Code**: `src/agents/builder.rs`. **Pool**: 1 (shots inside a production run concurrently).

## Contract
- Input: `Storyboard`. Output (handoff to Renderer): `Timeline { title, final_path, video_path, subtitle_path, clips: [Clip] }`.
- Assets: `{db.production_output_path}/shot_N/{image.png, audio.wav, video.mp4}` (`ShotAssets`). `video.mp4` is produced later by the Renderer.

## How it works
- Image: `shot.image_prompt(profile)` -> `Provider::CFWorker` (flux-1-schnell, `config.image`, fixed seed per production via `stable_seed` so any account/key gives the same image) -> `helper::is_image` check -> `helper::write_atomic`.
- Speech: at runtime the Builder checks `./tools/piper/piper` and `./tools/models/en_US-lessac-medium.onnx`. If both exist and the profile's language is English it runs Piper per dialogue line. Otherwise it logs why and uses ElevenLabs via the provider manager (voice = speaker's `voice_id`, else `config.voice.base_voice`; `config.tts` settings). Both give the same WAV format; segments are concatenated with `hound` into `audio.wav.part`, then renamed.
- Duration from `ffprobe`. Each clip lasts `speech + Transition::DURATION` (2 s); speech starts at half of it; clips overlap by `Transition::fade_between(prev, cur)`. `start_time`/`end_time` are subtitle times in the final video.
- Existing `image.png` and `audio.wav` are skipped (idempotent retries).

## Gotchas
- The overlap must match the Renderer's `xfade`/`acrossfade` offsets: both use `Transition::fade_between`. Change it in one place.
- Write files atomically; never leave a truncated asset that a retry would skip.
- Propagate errors; never ignore a failed image or audio.
- Images are square; the Renderer center-crops them to 9:16.
- Piper is gitignored under `tools/`: tell users to install it or set `ELEVEN_LABS_KEY_1`.

## Verify
`cargo check`, `cargo test`. Real image/TTS calls spend quota: ask the user first.
