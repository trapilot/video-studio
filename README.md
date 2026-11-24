# AI Kids Video Factory

An autonomous multi-agent system that generates educational short-form videos for children and publishes them to social media platforms.

## Features

* Multi-agent workflow architecture
* Educational content generation
* AI-generated images
* AI-generated voice narration
* Automatic video rendering
* YouTube Shorts upload
* TikTok upload
* Topic deduplication
* Character-driven storytelling

---

## Architecture

```text
Planner
  ↓
Writer
  ↓
Builder
  ↓
Renderer
  ↓
Publisher
  ↓
End
```

---

## Supported Providers

Provider clients live in `src/api/`. The active ones are chosen in `src/renderer.rs`.

| Role | Active | Also available |
|---|---|---|
| Script LLM | Gemini (`gemini-2.5-flash`) | — |
| Image | Cloudflare Workers AI (`flux-1-schnell`) | fal, Hugging Face, Pollinations, Google APIs |
| Text-to-Speech | ElevenLabs | Microsoft Edge TTS |
| Video rendering | FFmpeg | — |

---

## Project Structure

```text
src/
├── api/                # one HTTP client per provider
│   ├── cloudflare.rs
│   ├── elevenlabs.rs
│   ├── fal.rs
│   ├── gemini.rs
│   ├── googleapis.rs
│   ├── hugging_face.rs
│   ├── microsoft_edge.rs
│   ├── pollinations.rs
│   └── mod.rs
├── config.rs           # static config (language, voice, TTS, feature flags)
├── db.rs               # SQLite: topic history + resumable workflow state
├── enums.rs            # AgentNode, Motion, Transition, VoiceMode
├── helper.rs           # time, backoff, media detection, error context
├── main.rs             # entrypoint → scheduler
├── models.rs           # VideoState, characters, script/timeline schema
├── renderer.rs         # provider selection, asset generation, FFmpeg render
├── scheduler.rs        # main loop, retry/backoff, session resume
├── uploader.rs         # YouTube + TikTok upload
└── workflow.rs         # Planner → Writer → Builder → Renderer → Publisher state machine
```

---

## Environment Variables

```env
HF_API_KEY=
OPENAI_API_KEY=
GEMINI_API_KEY=
OPENROUTER_API_KEY=
ELEVENLABS_API_KEY=
POLLINATIONS_API_KEY=

CF_ACCOUNT_ID=
CF_API_TOKEN=

YOUTUBE_CLIENT_ID=
YOUTUBE_CLIENT_SECRET=
YOUTUBE_REFRESH_TOKEN=

TIKTOK_ACCESS_TOKEN=
```

Copy `.env.example` to `.env` and fill in the keys you use (`POLLINATIONS_API_KEY` is also supported).

---

## Running

Requires `ffmpeg` and `ffprobe` on `PATH`.

```bash
cargo run                   # start the scheduler loop
APP_DEBUG=True cargo run    # auto-revive sessions that exhausted retries
```

---

## Example Workflow

1. Planner selects a topic.
2. Writer generates video artifact.
3. Builder generates scene assets.
4. Renderer creates the final video.
5. Publisher publishes the video to Youtube|Tiktok.

---

## Example Output

```text
output/
├── 20200101_001/
│   ├── session_id
|           ├── audios/
│           │      ├── scene_1.mp3
│           │      ├── scene_2.mp3
│           │      └── scene_3.mp3
│           ├── images
│           │      ├── scene_1.png
│           │      ├── scene_2.png
│           │      └── scene_3.png
│           ├── videos
│           │      ├── scene_1.mp4
│           │      ├── scene_2.mp4
│           │      └── scene_3.mp4
|           ├── list.txt
|           └── final_video.mp4
│
```

---

## Roadmap

* [ ] Subtitle generation
* [ ] Background music generation
* [ ] Thumbnail generation
* [ ] Multi-language support
* [ ] Content quality review agent
* [ ] SEO optimization agent
* [ ] Automatic scheduling and publishing

---

## License

MIT License
