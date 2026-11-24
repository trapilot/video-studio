# Video Studio

Video Studio is an autonomous studio that produces short-form vertical video, any kind you define, and publishes it to YouTube Shorts and TikTok. Kids stories, explainers, product promos, motivational clips and news recaps are all the same pipeline with a different **studio profile**.

It runs as a loop: pick a topic, write a script, generate images and voice-over, render the video with FFmpeg, upload it, wait, repeat. Progress is saved to SQLite after every stage, so a crash or API failure resumes where it stopped.

---

## How it works

```text
Planner -> Writer -> Builder -> Renderer -> Publisher -> End
```

Each stage is one `match` arm of `run_agent_workflow` in `src/workflow.rs`. Stages talk to each other only through the shared `VideoState` (`src/models.rs`), which is saved to the database after every stage. The pipeline does not know what kind of video it makes; that comes from the active profile.

**Planner** (`AgentNode::Planner`, `src/workflow.rs`)
Asks the script LLM for one topic that fits the profile's format, audience and tone, avoiding the last 20 topics saved for this profile (`db.get_recent_topics`). If the profile has a cast, it also picks a main character, spotlight characters and supporting characters, and every name is validated against the cast (a hallucinated name fails the stage and triggers a retry). If the profile has no cast, the character fields are forced empty. Output: `target_topic` and the character fields on `VideoState`.

**Writer** (`AgentNode::Writer`, `src/workflow.rs`)
Builds a system prompt from the profile (format, audience, tone, visual style, cast, rules) and asks the LLM for a strict JSON `VideoArtifact`: a title and a list of scenes, each with `duration`, `motion`, `transition`, a `visual_prompt` (one still image) and `voice_segments` (speaker and text). Each segment then gets a `voice_id`: the cast member's voice, or `CONFIG.voice.base_voice` for the narrator and unknown speakers. Output: `final_json`.

**Builder** (`AgentNode::Builder`, `build_timelines` in `src/renderer.rs`)
For every scene, generates an image (Cloudflare `flux-1-schnell`) and a voice-over (ElevenLabs), at most 2 requests at a time. Existing files are skipped, so a retry only generates what is missing. Scene length is measured from the audio with `ffprobe`. Output: `images/scene_N.png`, `audios/scene_N.mp3` and `video_timelines` on the state.

**Renderer** (`AgentNode::Renderer`, `ffmpeg_render` in `src/renderer.rs`)
Turns each image and audio file into a clip with a camera-motion filter (`zoompan` based, see `Motion` in `src/enums.rs`), joins the clips with `xfade` transitions (see `Transition`) and writes the final file. Existing `videos/scene_N.mp4` and `final_video.mp4` are skipped. Output: `video_path`.

**Publisher** (`AgentNode::Publisher`, `src/uploader.rs`)
Uploads `final_video.mp4` to YouTube and TikTok concurrently. The title is the script's `title` (falling back to the topic), trimmed to YouTube's 100-character limit. YouTube description, tags, category and the made-for-kids flag come from `profile.publish`. Only a YouTube failure fails the stage; a TikTok failure is logged. A platform that already succeeded is skipped on retry, so nothing is posted twice.

**End**
Returns the artifact. The scheduler (`src/scheduler.rs`) marks the session `done` and saves the topic to `topic_history` for this profile, then sleeps 10 minutes.

See [AGENTS.md](AGENTS.md) for the full contract of each stage.

---

## Studio profiles

A `StudioProfile` (`src/profiles.rs`) describes one kind of video: what it is, who it is for, how it looks and sounds, how long it is, which rules apply, who appears in it, and how it is published. Profiles are static Rust data in the `PROFILES` array. The recurring cast is not part of the profile struct: it is loaded from data files (see "Characters are data" below).

### Fields

| Field | Meaning |
|---|---|
| `id` | Stable slug (`kids_edu`). Used in `STUDIO_PROFILE`, output folder names and topic dedup. Lowercase letters, digits and `_` only. Never rename it after producing videos. |
| `name` | Human-readable show or channel name. Shown in the startup log and given to the LLM. |
| `format` | What the video is, as a phrase that completes "producing ...". |
| `audience` | Who watches it. |
| `tone` | Voice and mood. |
| `topic_brief` | What the Planner should come up with each run ("Create {topic_brief}"). |
| `language` | Language for every spoken and written word. |
| `visual_style` | Image style for every scene when there is no cast. With a cast, the main character's `visual_anchor` is used instead. |
| `duration_secs` | `(min, max)` target length in seconds, given to the Writer. |
| `scene_count` | `(min, max)` number of scenes. |
| `content_rules` | Extra rules appended to the Writer prompt (safety, brand, factual constraints). |
| `publish` | `PublishProfile`: `description`, `tags`, `youtube_category`, `made_for_kids`. |

### Built-in profiles

| id | Name | Format | Cast | YouTube category | Made for kids |
|---|---|---|---|---|---|
| `kids_edu` (default) | Bin's Family Adventures | Animated educational story for children aged 4-8, told by a family cast | `data/characters/kids_edu/` (4 files: Bin, Hoang Yen, Nam, Diem Suong) | 27 Education | yes |
| `explainer` | Quick Explainers | Narrated explainer, one idea in under a minute, for curious adults and teenagers | none (narrator-only) | 28 Science & Technology | no |

### Selecting a profile

Set `STUDIO_PROFILE` in `.env` or the shell:

```bash
STUDIO_PROFILE=explainer cargo run
```

- Unset or empty: `kids_edu` (`DEFAULT_PROFILE_ID`).
- Unknown id: the program exits at startup with an error that lists the known ids.
- An unfinished session always resumes with the profile it started with (`VideoState.profile_id`), even if `STUDIO_PROFILE` has changed. The new value applies to the next new session.

The startup banner shows what is active:

```text
🚀 Video Studio v0.1.0 | profile: kids_edu (Bin's Family Adventures) | cast: Quốc Khánh, Hoàng Yến, Nam, Diễm Sương
🚀 Video Studio v0.1.0 | profile: explainer (Quick Explainers) | cast: narrator only
```

### Cast or narrator-only

| | With a cast | Narrator-only (no characters) |
|---|---|---|
| Planner returns | topic, main character, spotlight and supporting characters (validated with `profile.character()`) | topic only; character fields are forced empty |
| Writer speakers | cast member names only | `"Narrator"` for every segment |
| Voice | each character's `voice_id`; unknown speakers fall back to `CONFIG.voice.base_voice` | `CONFIG.voice.base_voice` |
| Visual style | main character's `visual_anchor` | `profile.visual_style` |
| Extra Writer rules | "Use only the provided characters as speakers", "The main character must drive the story" | "No on-screen dialogue between characters; narration only" |

The Writer rules are numbered in this order: generic rules (language, duration, scene count, at least one voice segment per scene, still-image visual prompts, no invented motion or transition names), then the cast rules above, then the profile's `content_rules`.

---

## Create a new video type

1. Open `src/profiles.rs`.
2. Pick an `id`: a short lowercase slug such as `product_promo`. It becomes a folder name and a database key, so choose it once.
3. Write `format`, `audience`, `tone` and `topic_brief` as prompt-ready phrases. They are pasted into the LLM prompts as they are.
4. Write a `visual_style` that works as a prefix for every image prompt: medium, lighting, composition, and "no text" if you don't want garbled lettering.
5. Set `duration_secs` and `scene_count`. Keep them consistent (a 15 second video does not need 8 scenes).
6. Add `content_rules`: short imperative sentences for the things that must always or never happen.
7. Decide on a cast. For a cast, create `data/characters/<id>/` with one `.toml` file per character after step 9 (see [data/characters/README.md](data/characters/README.md)); for narrator-only, create nothing.
8. Fill in `publish`. Set `made_for_kids: true` for anything aimed at children (COPPA).
9. Append the profile to `PROFILES`, then run `cargo test` and `cargo check`.
10. Try it with `STUDIO_PROFILE=<id> cargo run` (this spends API credits and publishes publicly, see Quick start).

### Example: a narrator-only profile

```rust
StudioProfile {
    id: "product_promo",
    name: "Gadget Spotlight",
    format: "a punchy 30 second vertical promo video for a consumer gadget",
    audience: "young adults who shop online",
    tone: "energetic, upbeat, persuasive but honest",
    topic_brief: "a single everyday gadget or accessory and the one problem it solves",
    language: "English",
    visual_style: "a clean studio product photograph, soft gradient background, dramatic lighting, vertical 9:16 composition, no text, no logos",
    duration_secs: (25, 35),
    scene_count: (4, 5),
    content_rules: &[
        "Open with the problem in the first scene.",
        "Name one concrete benefit per scene.",
        "Do not invent prices, discounts or statistics.",
        "End with a short call to action.",
        "Visual prompts must not contain written text or brand logos.",
    ],
    publish: PublishProfile {
        description: "A quick look at a handy gadget",
        tags: &["gadgets", "product", "review", "shorts"],
        youtube_category: "26",
        made_for_kids: false,
    },
},
```

### Characters are data

The cast is not written in Rust. Each character is one TOML file in `data/characters/<profile_id>/` (for example `data/characters/kids_edu/01_bin.toml`). To add a character, drop in a new file and restart; no rebuild is needed. Files load in file-name order, which is the cast order in prompts. Only `name` is required, and a misspelled field is a startup error.

```toml
# data/characters/study_buddies/01_mia.toml
name = "Mia"
spotlight = true          # can be the main character
birth_year = 2010
speaking_style = "Quick, curious, a little sarcastic."
visual_anchor = "a flat vector illustration of a teenage girl with a green hoodie and round glasses, simple pastel background"
personality_prompt = "A curious high-school student who learns by asking questions."
```

The cast must contain at least one enabled `spotlight = true` character, names are unique inside a profile, and `"Narrator"` is reserved. The folder name must match a profile id. A profile with no folder (or no enabled characters) is narrator-only. The full field reference, validation rules and tips are in [data/characters/README.md](data/characters/README.md).

---

## Quick start

Prerequisites:

- Rust 1.91 or newer (the code uses `Duration::from_mins`).
- `ffmpeg` and `ffprobe` on `PATH`.
- API keys: Gemini (script), Cloudflare Workers AI (images), ElevenLabs (voice), YouTube OAuth credentials, optionally a TikTok token.

```bash
cp .env.example .env        # then fill in the keys
cargo build
STUDIO_PROFILE=explainer cargo run
```

Warning: `cargo run` starts a loop that never exits. Every iteration calls paid APIs and uploads the finished video **publicly** to YouTube and TikTok. Do not run it until your keys and the active profile are what you want. Use `cargo check` and `cargo test` to verify changes without side effects.

---

## Configuration

### Environment variables

| Variable | Used by | Purpose |
|---|---|---|
| `STUDIO_PROFILE` | `src/profiles.rs` | Profile id to produce. Empty or unset means `kids_edu`. Unknown id exits at startup. |
| `STUDIO_DATA_DIR` | `src/main.rs`, `src/characters.rs` | Data folder that holds `characters/<profile_id>/*.toml`. Unset means `./data`. |
| `APP_DEBUG` | `src/scheduler.rs` | `True` revives sessions that hit `max_retry` immediately instead of waiting. Any other value is off. |
| `GEMINI_API_KEY` | `src/api/gemini.rs` | Script LLM (active). |
| `CF_ACCOUNT_ID`, `CF_API_TOKEN` | `src/api/cloudflare.rs` | Image generation (active). |
| `ELEVENLABS_API_KEY` | `src/api/elevenlabs.rs` | Text-to-speech (active). |
| `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN` | `src/uploader.rs` | YouTube upload. |
| `TIKTOK_ACCESS_TOKEN` | `src/uploader.rs` | TikTok upload. Required, see Known issues. |
| `HF_API_KEY`, `POLLINATIONS_API_KEY` | `src/api/*.rs` | Optional providers that are present but not wired in. |
| `FAL_API_KEY`, `GOOGLE_ACCESS_TOKEN`, `GOOGLE_PROJECT_ID`, `EDGE_VOICE` | `src/api/*.rs` | Read only by the unused `fal`, `googleapis` and `microsoft_edge` clients. |

### `src/config.rs`

Studio-wide technical settings only. What to produce lives in the profile.

| Setting | Meaning |
|---|---|
| `video.width`, `video.height`, `video.fps` | Output frame size and frame rate (1080x1920, 25). Every scene is scaled and center-cropped to it. |
| `voice.base_voice` | Fallback ElevenLabs voice id: the narrator's voice and the voice of any speaker without a cast entry. |
| `voice.default_gender` | Default gender label. |
| `tts.speed`, `tts.stability`, `tts.similarity_boost` | ElevenLabs voice settings. |
| `feature.enable_cache`, `enable_subtitle`, `enable_translation` | Feature flags (currently all `false`). |

---

## Providers

Provider clients live in `src/api/`. Only `src/renderer.rs` calls them; workflow code never does. Swapping a provider means changing one call there.

| Role | Active | Called from | Also available |
|---|---|---|---|
| Script LLM | Gemini `gemini-2.5-flash` (`gemini::generate_script`) | `build_content` | |
| Image | Cloudflare `flux-1-schnell` (`cloudflare::generate_image`) | `generate_scene_visual` | fal, Hugging Face, Pollinations, Google APIs |
| Text-to-speech | ElevenLabs (`elevenlabs::generate_tts`), max 2 concurrent requests | `generate_scene_audio` | Microsoft Edge TTS |
| Video | FFmpeg | `ffmpeg_render` | |

---

## Output layout

```text
output/
└── 20260101_explainer/              # {YYYYMMDD}_{profile_id}
    └── <session_id>/
        ├── images/
        │   ├── scene_1.png
        │   └── scene_2.png
        ├── audios/
        │   ├── scene_1.mp3
        │   └── scene_2.mp3
        ├── videos/
        │   ├── scene_1.mp4
        │   └── scene_2.mp4
        └── final_video.mp4
```

The date is the day the session was created. SQLite state lives in `./data_history.db`.

---

## State, resume and retries

State is stored as JSON in the `workflow_state` table after every stage. Behavior of the loop in `src/scheduler.rs`:

- On each iteration it resumes the most recently updated unfinished session whose `version` equals `APP_VERSION` (`src/db.rs`); if there is none, it starts a new session with the active profile.
- When a stage fails, the retry counter increases, the error is saved, and the loop sleeps for an exponential backoff: the ceiling starts at 2 s and doubles per failure up to 30 minutes (`helper::next_backoff`), with random jitter between half the ceiling and the ceiling. It resets after every successful stage.
- `max_retry` is 3. Once reached:
  - If the session was last updated 30 minutes ago or more, it is deleted and a fresh session starts.
  - Otherwise the scheduler logs it and waits 10 minutes, or with `APP_DEBUG=True` revives it immediately.
  - If the timestamp cannot be parsed, the session is cancelled and the scheduler waits 30 minutes.
- If the database cannot be read, it waits 30 minutes and tries again.
- After a successful session it marks it `done`, saves the topic to `topic_history`, and sleeps 10 minutes.
- Builder and Renderer skip files that already exist, so a retry only does the missing work. Delete files to force regeneration.
- Topic dedup uses the last 20 topics of the session's profile. Topics are saved only after a successful End, so failed runs do not count.
- Bump `APP_VERSION` in `src/db.rs` when `VideoState` or the artifact schema changes incompatibly, so sessions saved by an older build are ignored rather than failing to load.
- A saved session whose profile no longer exists in `PROFILES` is cancelled.
- Assets and renders are written to a `.part` file and renamed when complete, so an interrupted run never leaves a half-written file that would be reused.

---

## Project structure

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
├── characters.rs       # loads and validates data/characters/<profile_id>/*.toml
├── config.rs           # studio-wide settings: base voice, TTS, feature flags
├── db.rs               # SQLite: topic_history + resumable workflow_state
├── enums.rs            # AgentNode, Motion, Transition, VoiceMode
├── helper.rs           # time, backoff, media detection, error context
├── main.rs             # entrypoint: select profile, start scheduler
├── models.rs           # VideoState, VideoArtifact, Scene, VideoTimeline
├── profiles.rs         # StudioProfile, PROFILES, active_profile, cast() lookup
├── renderer.rs         # provider selection, asset generation, FFmpeg render
├── scheduler.rs        # main loop, retry/backoff, session resume
├── uploader.rs         # YouTube + TikTok upload
└── workflow.rs         # Planner -> Writer -> Builder -> Renderer -> Publisher

data/
└── characters/
    ├── README.md       # how to add a character
    └── kids_edu/       # one folder per profile id, one .toml per character
        ├── 01_bin.toml
        ├── 02_yen.toml
        ├── 03_nam.toml
        └── 04_suong.toml
```

---

## Development

```bash
cargo check     # fast type-check, run after every edit
cargo clippy    # lints
cargo test                # unit tests
cargo test -- --ignored   # real FFmpeg render on generated assets (needs ffmpeg, no API calls)
```

`cargo test` runs:

- `profiles::tests::profiles_are_well_formed`: every profile has a unique lowercase-slug id, the default profile exists, and duration and scene ranges are sane. Casts are validated by the loader at startup instead.
- `characters::tests::{repo_data_loads, minimal_file_only_needs_a_name, rejects_bad_data}`: the repo's `data/characters` loads (4 kids_edu characters, explainer has none), a file with only `name` is valid, and bad data (unknown profile folder, duplicate name, `Narrator`, no spotlight, misspelled field) is rejected.
- workflow tests: character file fields reach the Writer prompt, and artifact normalization.
- `db::tests`: topics are scoped per profile; resume picks the latest unfinished session, even when a finished one was saved later.
- `helper::tests`: media sniffing rejects empty, truncated and HTML/JSON error bodies; backoff doubles and caps; titles truncate on character boundaries.
- `renderer::tests::fade_never_exceeds_half_a_clip`, and the ignored `renders_mixed_motions_short_scenes_and_single_scene`, which renders real clips with FFmpeg (mixed motions, a 0.6 s scene, an unknown transition, a one-scene video) and checks the final length and 1080x1920 size.

---

## Known issues and limitations

- Visuals are still images animated with `zoompan` camera motion, not generated video. Cloudflare returns square images, which are center-cropped to 9:16, so the Writer is told to keep subjects centered.
- Only Gemini is wired as the script provider.
- Uploads are always public; there is no dry-run mode.
- TikTok `PUBLIC_TO_EVERYONE` posting requires an audited TikTok app; unaudited apps get an error (logged, not fatal).
- The default `SingleVoice` mode voices a whole scene with the first speaker's voice. `VoiceMode::PerSegment` (one voice per line) exists but is not enabled.

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
