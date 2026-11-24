---
name: planner-agent
description: Work on the Planner stage of the video pipeline — topic selection, character casting, topic dedup, or adding/editing characters in EDU_CHARACTERS. Use when changing how topics or characters are chosen.
---

# Planner Agent

**Code:** `AgentNode::Planner` arm in `src/workflow.rs`. Characters are in `EDU_CHARACTERS` in `src/models.rs`.

## Contract
- Input: `state.target_age`, plus the last 20 topics from `db.get_recent_topics(age, 20)`.
- Output: sets `target_topic`, `main_character`, `spotlight_characters`, `supporting_characters` on `VideoState`, then moves to `AgentNode::Writer` and calls `save_state`.
- The LLM must return JSON: `{topic, main_character, spotlight_characters[], supporting_characters[]}`.

## Casting rules
- Spotlight pool = `enabled && spotlight`. These can be the main character.
- Relation pool = `enabled && !spotlight`. These become supporting characters.
- `target_age` comes from the **first** entry in `EDU_CHARACTERS` (`VideoState::new`). Reordering that array changes the target age and the output folder.

## Adding a character
1. Append a `Character { .. }` to `EDU_CHARACTERS`. Every field is `&'static str`/`i32`/`bool`.
2. `name` must be unique. The Writer matches `segment.speaker == name` to assign `voice_id`.
3. Set `voice_id` to a valid ElevenLabs voice. If a speaker doesn't match any name, it falls back to `CONFIG.voice.base_voice`.
4. `visual_anchor` is the image-style prefix used when this character is the main character. Keep it kid-friendly and in the same 3D Pixar style as the others.

## Guidelines
- Parse LLM JSON with `json_str` / `json_str_array` (bottom of `workflow.rs`), never `unwrap()`.
- Returned names are checked against `EDU_CHARACTERS` (the main character must be a spotlight character). An unknown name returns `Err`, so the scheduler retries.
- Topics only go into `video_history` after a successful End (`scheduler.rs`), so failed runs don't count toward dedup.
- Verify with `cargo check`.
