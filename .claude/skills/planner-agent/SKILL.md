---
name: planner-agent
description: Work on the Planner stage of the video pipeline — topic selection, character casting, topic dedup, or adding/editing characters in a profile's cast. Use when changing how topics or characters are chosen.
---

# Planner Agent

**Code:** `AgentNode::Planner` arm in `src/workflow.rs`. The prompt is built from the active profile (`state.profile()`, `src/profiles.rs`). Casts are `const` arrays of `Character` in `src/profiles.rs` (e.g. `KIDS_EDU_CAST`); the struct is in `src/models.rs`.

## Contract
- Input: the profile (name, format, audience, tone, language, `topic_brief`, cast) plus the last 20 topics from `db.get_recent_topics(&state.profile_id, 20)`.
- Output: sets `target_topic`, `main_character`, `spotlight_characters`, `supporting_characters` on `VideoState`, then moves to `AgentNode::Writer` and calls `save_state`.
- The LLM always returns JSON: `{topic, main_character, spotlight_characters[], supporting_characters[]}`.

## Cast vs narrator branch
- `profile.has_cast()` (any enabled character): the prompt lists SPOTLIGHT CHARACTERS (`enabled && spotlight`, can be main) and RELATION CHARACTERS (`enabled && !spotlight`, become supporting). Names are validated with `profile.character()`; the main character must be a spotlight character.
- No cast: the prompt asks only for a topic and empty character fields. Whatever the LLM returns for them is discarded and the fields are set to empty.

## Adding a character
1. Append a `Character { .. }` to the profile's cast const in `src/profiles.rs` (not to a global list). Every field is `&'static str`/`i32`/`bool`.
2. `name` must be unique within the cast and must not be `"Narrator"` (reserved). The Writer matches `segment.speaker == name` to assign `voice_id`.
3. Set `voice_id` to a valid ElevenLabs voice. A speaker that matches no name falls back to `CONFIG.voice.base_voice`.
4. `visual_anchor` is the image-style prefix used when this character is the main character. Keep it consistent with the profile's `visual_style` and the other characters' anchors.
5. A cast needs at least one enabled spotlight character. `cargo test` checks this.

## Guidelines
- Parse LLM JSON with `json_str` / `json_str_array` (bottom of `workflow.rs`), never `unwrap()`.
- An unknown character name returns `Err`, so the scheduler retries.
- Topics go into `topic_history` (per profile) only after a successful End (`scheduler.rs`), so failed runs don't count toward dedup.
- Don't hardcode audience or format in the prompt; read it from the profile.
- Verify with `cargo check` and `cargo test`.
