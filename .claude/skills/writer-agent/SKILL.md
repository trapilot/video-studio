---
name: writer-agent
description: Work on the Writer agent (src/agents/writer.rs) and the Storyboard types: prompt rules, Motion/Transition/Position enums, lenient LLM JSON parsing, image prompts. Use when changing the script prompt, shot schema or enums.
---

# Writer agent

**Code**: `src/agents/writer.rs`; types and parsing in `src/entities/storyboard.rs`. **Pool**: 2.

## Contract
- Input: `Brief`. Output (handoff to Builder): `Storyboard { title, shots: [Shot] }`.
- Shot: `shot_id`, `visual_prompt`, `motion`, `transition`, `actors[]` (character, position, action, emotion), `dialogues[]` (character, text), `environment`, `camera`.

## How it works
- With a cast, the prompt contains `Character::describe()` of each chosen character (every non-empty field plus Appearance from `visual_template`). Without a cast it requires the speaker `"Narrator"` (`NARRATOR`).
- Numbered rules = generic rules (language, `duration_secs`, `shot_count`, one idea per shot, text-free visuals, 9:16 framing) + cast rules + `profile.content_rules`.
- Allowed motions, transitions and positions are printed from `Motion::ALL`, `Transition::ALL`, `Position::ALL`.
- `Storyboard::from_llm_json` parses leniently: case/`_`/`-`/space-insensitive enum matching, fallbacks (`none`, `fade`, `center`), drops empty dialogue and dialogue-less shots, sorts and renumbers shots 1..n.
- `Shot::image_prompt(profile)`: `visual_style`, `visual_prompt`, each actor's `visual_anchor()`, setting, camera, "vertical 9:16 ... no text". Capped at 2000 characters.

## Gotchas
- Adding a `Motion`/`Transition`/`Position` variant needs: enum variant, entry in `ALL`, lowercase `Display`/`FromStr` (strum), and for motion a `zoompan` filter in `ffmpeg_filter`, for transition an `ffmpeg_name` (valid FFmpeg xfade name).
- Do not tell the LLM names the enums cannot parse.
- Keep guidelines generic: no show- or character-specific text in the prompt.
- New `Shot` fields: `#[serde(default)]`.

## Verify
`cargo check`, `cargo test` (`storyboard_parsing_is_lenient_and_safe`, `image_prompt_uses_style_shot_and_character_look`). No real Gemini calls without the user's OK.
