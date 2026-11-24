//! Characters are data, not code: one TOML file per character under
//! `{data_dir}/characters/{profile_id}/`. They are loaded once at startup.
//!
//! To add a character, drop a new `.toml` file into the profile's folder and
//! restart. No rebuild needed. Files load in file-name order, so a numeric
//! prefix (`01_bin.toml`) controls the order the cast is listed in prompts.
//! See `data/characters/README.md` for the field reference.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::config::CONFIG;
use crate::helper::now_age;
use crate::profiles::{PROFILES, NARRATOR};

/// Default data folder, overridable with `STUDIO_DATA_DIR`.
pub const DEFAULT_DATA_DIR: &str = "./data";

/// A recurring on-screen character. Only `name` is required in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    /// `false` retires the character without deleting the file.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// `true` = can be chosen as the main character.
    #[serde(default)]
    pub spotlight: bool,

    /// Unique within the profile. The Writer uses it as the speaker name.
    pub name: String,
    #[serde(default)]
    pub nickname: String,
    /// ElevenLabs voice id. Empty = `CONFIG.voice.base_voice`.
    #[serde(default)]
    pub voice_id: String,
    /// Used to tell the LLM the character's age. Omit for ageless characters.
    #[serde(default)]
    pub birth_year: Option<i32>,

    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub family_role: String,
    #[serde(default)]
    pub story_function: String,

    #[serde(default)]
    pub profession: String,
    #[serde(default)]
    pub topic_domain: String,

    #[serde(default)]
    pub catchphrase: String,
    #[serde(default)]
    pub speaking_style: String,
    /// Image-style prefix used when this character is the main character.
    #[serde(default)]
    pub visual_anchor: String,

    #[serde(default)]
    pub strengths: String,
    #[serde(default)]
    pub weaknesses: String,
    #[serde(default)]
    pub core_values: String,

    /// How this character relates to the others. Read by the LLM.
    #[serde(default)]
    pub relationship_prompt: String,
    /// Who this character is. Read by the LLM.
    #[serde(default)]
    pub personality_prompt: String,
}

fn yes() -> bool {
    true
}

impl Character {
    pub fn voice(&self) -> &str {
        if self.voice_id.trim().is_empty() {
            CONFIG.voice.base_voice
        } else {
            &self.voice_id
        }
    }

    /// Age for prompts: a number, or "unspecified" when `birth_year` is omitted.
    pub fn age_label(&self) -> String {
        self.birth_year
            .map(|y| now_age(y).to_string())
            .unwrap_or_else(|| "unspecified".to_string())
    }
}

static CASTS: OnceLock<HashMap<String, Vec<Character>>> = OnceLock::new();

/// Load every profile's cast from `{data_dir}/characters`. Call once at startup.
/// A missing `characters` folder is fine (all profiles narrator-only); a bad file is an error.
pub fn init(data_dir: &str) -> Result<(), String> {
    let casts = load_all(Path::new(data_dir))?;
    CASTS.set(casts).map_err(|_| "Characters already loaded".to_string())
}

/// The loaded cast of a profile, in file-name order. Empty if it has none.
pub fn cast_for(profile_id: &str) -> &'static [Character] {
    CASTS
        .get()
        .and_then(|c| c.get(profile_id))
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

fn load_all(data_dir: &Path) -> Result<HashMap<String, Vec<Character>>, String> {
    let root = data_dir.join("characters");
    let mut casts = HashMap::new();

    if !root.is_dir() {
        return Ok(casts);
    }

    for entry in read_sorted(&root)? {
        if !entry.is_dir() {
            continue;
        }
        let profile_id = entry.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        // A folder that matches no profile is almost always a typo; fail loudly.
        if !PROFILES.iter().any(|p| p.id == profile_id) {
            let known = PROFILES.iter().map(|p| p.id).collect::<Vec<_>>().join(", ");
            return Err(format!(
                "{}: folder doesn't match any profile id (known: {})",
                entry.display(), known
            ));
        }
        casts.insert(profile_id, load_cast(&entry)?);
    }

    Ok(casts)
}

fn load_cast(dir: &Path) -> Result<Vec<Character>, String> {
    let mut cast: Vec<Character> = Vec::new();

    for path in read_sorted(dir)? {
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        let character: Character = toml::from_str(&text)
            .map_err(|e| format!("{}: {}", path.display(), e))?;

        let name = character.name.trim();
        if name.is_empty() {
            return Err(format!("{}: `name` is empty", path.display()));
        }
        if name == NARRATOR {
            return Err(format!("{}: \"{}\" is reserved for narrator-only profiles", path.display(), NARRATOR));
        }
        if cast.iter().any(|c| c.name == character.name) {
            return Err(format!("{}: duplicate character name \"{}\"", path.display(), character.name));
        }
        cast.push(character);
    }

    if cast.iter().any(|c| c.enabled) && !cast.iter().any(|c| c.enabled && c.spotlight) {
        return Err(format!(
            "{}: cast has no enabled character with `spotlight = true` (needed for the main character)",
            dir.display()
        ));
    }

    Ok(cast)
}

fn read_sorted(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut paths = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {}", dir.display(), e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
pub(crate) fn init_for_tests() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
    if CASTS.get().is_none() {
        let _ = CASTS.set(load_all(Path::new(dir)).expect("data/characters must load"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_data_loads() {
        init_for_tests();
        let kids = cast_for("kids_edu");
        assert_eq!(kids.len(), 4);
        assert_eq!(kids[0].nickname, "Bin");
        assert!(kids[0].spotlight);
        assert_eq!(kids[0].voice(), CONFIG.voice.base_voice);
        assert!(cast_for("explainer").is_empty());
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn minimal_file_only_needs_a_name() {
        let root = std::env::temp_dir().join(format!("vs_chars_{}", uuid::Uuid::new_v4()));
        let dir = root.join("characters/kids_edu");
        write(&dir, "a.toml", "name = \"Mia\"\nspotlight = true\n");
        let casts = load_all(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let mia = &casts["kids_edu"][0];
        assert!(mia.enabled);
        assert_eq!(mia.age_label(), "unspecified");
    }

    #[test]
    fn rejects_bad_data() {
        let cases: &[(&str, &[(&str, &str)])] = &[
            ("unknown_profile", &[("a.toml", "name = \"A\"\nspotlight = true")]),
            ("kids_edu", &[("a.toml", "name = \"A\"\nspotlight = true"), ("b.toml", "name = \"A\"")]),
            ("kids_edu", &[("a.toml", "name = \"Narrator\"\nspotlight = true")]),
            ("kids_edu", &[("a.toml", "name = \"A\"")]),                       // no spotlight
            ("kids_edu", &[("a.toml", "name = \"A\"\nspotlite = true")]),      // typo'd field
        ];
        for (folder, files) in cases {
            let root = std::env::temp_dir().join(format!("vs_chars_{}", uuid::Uuid::new_v4()));
            for (name, body) in *files {
                write(&root.join("characters").join(folder), name, body);
            }
            let result = load_all(&root);
            let _ = std::fs::remove_dir_all(&root);
            assert!(result.is_err(), "expected error for {:?}", files);
        }
    }
}
