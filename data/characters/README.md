# Characters

Recurring on-screen characters are data, not code. Each character is one TOML file. Adding, editing or retiring a character needs a restart, not a rebuild.

## Folder layout

```text
data/
└── characters/
    ├── README.md            # this file
    └── kids_edu/            # folder name = profile id (src/profiles.rs)
        ├── 01_bin.toml
        ├── 02_yen.toml
        ├── 03_nam.toml
        └── 04_suong.toml
```

- `data/` is the default data folder. Set the env var `STUDIO_DATA_DIR` to use another one; the studio then reads `{STUDIO_DATA_DIR}/characters/{profile_id}/*.toml`.
- The cast is loaded once at startup (`characters::init`, called in `main` before the profile is resolved). Changes to the files take effect on the next start.
- A missing `characters/` folder is fine: every profile is then narrator-only. A profile with no folder, or with no enabled characters, is also narrator-only.
- Files that do not end in `.toml` are ignored (notes, backups).

## Add a character

1. Pick the profile and open its folder, for example `data/characters/kids_edu/`.
2. Copy an existing file to a new name with the next number prefix, for example `05_lan.toml`.
3. Set `name` (required, unique within the profile) and fill in the fields you want. Delete or comment out the rest.
4. Give the character a `visual_anchor` in the same art style as the other characters if it can be the main character (`spotlight = true`).
5. Optional: set `voice_id` to an ElevenLabs voice id. Without it the character speaks with `CONFIG.voice.base_voice`.
6. Restart the studio. A mistake (typo in a field name, duplicate name, ...) stops the program at startup with the file path and the reason.

No code changes, no rebuild. Do not hardcode a cast in Rust.

### File names and order

Files load in file-name order. That order is the cast order in the prompts, so use a numeric prefix (`01_`, `02_`, ...) to control it. The file name has no other meaning: the character is identified by `name` inside the file.

## Fields

Only `name` is required. Strings default to empty, and empty strings are left out of the prompts.

| Field | Required / default | What it does | Used in |
|---|---|---|---|
| `enabled` | default `true` | `false` retires the character without deleting the file. A disabled character is invisible to the pipeline. | Planner, Writer |
| `spotlight` | default `false` | `true` means the character can be chosen as the main character. `false` makes the character a supporting ("relation") character. | Planner, Writer |
| `name` | required, unique per profile | The name the LLM uses in JSON and as the speaker name in `voice_segments`. Must match exactly. `"Narrator"` is reserved. | Planner, Writer, voice |
| `nickname` | empty | Short or alternative name. | Writer |
| `voice_id` | empty = `CONFIG.voice.base_voice` | ElevenLabs voice id used for this character's lines. | Voice (TTS) |
| `birth_year` | omitted = age shown as `unspecified` | Age is computed from it at run time and given to the LLM. Omit for ageless characters. | Planner, Writer |
| `role` | empty | Narrative role (for example "Protagonist"). | Planner (supporting characters), Writer |
| `family_role` | empty | Relationship role inside the family or group. | Writer |
| `story_function` | empty | What this character does in a story. | Writer |
| `profession` | empty | Job or occupation. | Planner (supporting characters), Writer |
| `topic_domain` | empty | Topics this character naturally fits. Helps the Planner pick a main character for a topic. | Planner (spotlight characters) |
| `catchphrase` | empty | A signature line. | Writer |
| `speaking_style` | empty | How the character talks. | Writer |
| `visual_anchor` | empty = `profile.visual_style` | Image-style prefix for every scene when this character is the main character. | Image prompts (via the Writer) |
| `strengths` | empty | Positive traits. | Writer |
| `weaknesses` | empty | Flaws that drive the story. | Writer |
| `core_values` | empty | What the character cares about. | Writer |
| `relationship_prompt` | empty | How this character relates to the others. | Planner, Writer |
| `personality_prompt` | empty | Who this character is. | Planner (spotlight characters), Writer |

Where each field goes, in detail:

- Planner, spotlight characters: `name`, `topic_domain`, age, `personality_prompt`, `relationship_prompt`.
- Planner, other (relation) characters: `name`, `profession`, age, `role`, `relationship_prompt`.
- Writer: every non-empty field of the characters picked by the Planner (main, spotlight and supporting): `nickname`, age, `role`, `family_role`, `story_function`, `profession`, `personality_prompt`, `speaking_style`, `catchphrase`, `strengths`, `weaknesses`, `core_values`, `relationship_prompt`. The Writer also gets the main character's `personality_prompt` on its own, and a list of every enabled character with its `personality_prompt`.
- Writer, visual style: the main character's `visual_anchor` (or `profile.visual_style` if it is empty).
- Voice: `voice_id`, set on each `voice_segment` whose speaker matches `name`. Any other speaker uses `CONFIG.voice.base_voice`.

## Examples

### Minimal

```toml
name = "Mia"
spotlight = true
```

This is a valid cast of one. Age is `unspecified`, the voice is the base voice, and the image style is the profile's `visual_style`.

### Full

`data/characters/kids_edu/01_bin.toml`:

```toml
enabled = true
spotlight = true

name = "Quốc Khánh"
nickname = "Bin"
birth_year = 2020

role = "Protagonist"
family_role = "child"
story_function = "Main protagonist who initiates most adventures and learning experiences."

profession = "Student"
topic_domain = "Curiosity, Exploration, Everyday Learning"

catchphrase = "I want to try it!"
speaking_style = "Very energetic, speaks quickly, asks many 'why' questions, expresses excitement openly."
visual_anchor = "a 3D Pixar style illustration of a cute Vietnamese boy age 6, short black hair, bright curious eyes, yellow t-shirt, blue shorts, energetic pose, white background"

strengths = "Curious, brave, friendly, imaginative, willing to help others."
weaknesses = "Impulsive, impatient, easily distracted, sometimes acts before thinking."
core_values = "Curiosity, friendship, courage, kindness."

relationship_prompt = "Bin is the son of Nam and Diem Suong. He is the best friend of Hoang Yen. Hoang Yen often helps and protects him when he gets into trouble."
personality_prompt = "A lively and energetic Vietnamese boy who loves exploring everything around him and learning through adventure."
# voice_id = "..."   # ElevenLabs voice id; omit to use CONFIG.voice.base_voice
```

## Validation

Every file is checked at startup. Any failure prints the file path and exits; nothing is skipped silently.

| Problem | Error |
|---|---|
| The file is not valid TOML | `{path}: ` plus the TOML parse error |
| A field name is misspelled or unknown (the schema rejects unknown fields) | `{path}: ` plus the "unknown field" error |
| `name` is missing | `{path}: ` plus the "missing field `name`" error |
| `name` is empty | `{path}: `name` is empty` |
| `name` is `Narrator` | `{path}: "Narrator" is reserved for narrator-only profiles` |
| Two characters in one profile share a `name` | `{path}: duplicate character name "..."` |
| A folder under `characters/` matches no profile id | `{folder}: folder doesn't match any profile id (known: ...)` |
| A cast has enabled characters but none is enabled with `spotlight = true` | `{folder}: cast has no enabled character with `spotlight = true` (needed for the main character)` |

The startup banner prints the loaded cast, so you can confirm it:

```text
🚀 Video Studio v0.1.0 | profile: kids_edu (Bin's Family Adventures) | cast: Quốc Khánh, Hoàng Yến, Nam, Diễm Sương
```

`cast: narrator only` means the profile has no enabled characters.

## Retire a character

Set `enabled = false` in the file and restart. The character disappears from the prompts and from validation of Planner output (an LLM answer that names it is rejected). Keep the file if you may bring the character back.

If you retire the only spotlight character, enable another one with `spotlight = true`, or the startup check fails. If you disable every character, the profile becomes narrator-only.

## Give a new profile a cast

1. Add the profile in `src/profiles.rs` (see the `add-profile` skill and the README).
2. Create `data/characters/<profile_id>/`. The folder name must equal the profile `id`.
3. Add one `.toml` file per character, with at least one `spotlight = true`.
4. Restart.

For a narrator-only profile, create no folder.

## Tips for writing good prompt fields

- `personality_prompt` and `relationship_prompt` are what the LLM reads most. Write concrete behavior ("asks why before every experiment"), not adjectives alone.
- Mention the other characters by the names the cast uses, so relationships are unambiguous.
- Keep `catchphrase` and `speaking_style` consistent with the personality text. The Writer uses all three.
- `topic_domain` steers the Planner. Be specific enough that topics map to a character.
- Keep every `visual_anchor` in one art style, and in the same style as the profile's `visual_style`. Describe look, clothes and pose; add the background and "no text".
- `name` is the identity. Do not rename a character that existing sessions already use; the saved session would no longer match the cast.
- Leave a field out rather than filling it with filler; empty fields cost nothing in the prompt.
