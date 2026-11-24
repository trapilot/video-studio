//! Studio profiles: one profile = one kind of video the studio can produce.
//!
//! A profile tells every pipeline stage *what* to make: the format, audience,
//! tone, visual style, length, extra content rules, the optional recurring cast,
//! and how to publish the result. The pipeline code itself is format-agnostic.
//!
//! The active profile is chosen with the `STUDIO_PROFILE` env var (matched
//! against `StudioProfile::id`). If it is unset, `DEFAULT_PROFILE_ID` is used.
//! To add a new kind of video, append a `StudioProfile` to `PROFILES`.

use std::env;

use crate::config::CONFIG;
use crate::models::Character;

/// Profile used when `STUDIO_PROFILE` is not set.
pub const DEFAULT_PROFILE_ID: &str = "kids_edu";

/// Speaker name used for voice-over when a profile has no cast.
pub const NARRATOR: &str = "Narrator";

#[derive(Debug)]
pub struct StudioProfile {
    /// Stable slug. Used in `STUDIO_PROFILE`, output folder names, and topic dedup.
    /// Never rename it once videos have been produced, or dedup history is lost.
    pub id: &'static str,
    /// Human-readable name (shown in logs and given to the LLM as the show/channel name).
    pub name: &'static str,

    /// What kind of video this is, in one sentence. Drives the Planner and Writer prompts.
    pub format: &'static str,
    /// Who the video is for.
    pub audience: &'static str,
    /// Voice and mood of the content.
    pub tone: &'static str,
    /// What the Planner should come up with each run (e.g. "a fresh educational topic").
    pub topic_brief: &'static str,
    /// Language for all spoken and written content.
    pub language: &'static str,

    /// Image style used for every scene when there is no main character
    /// (with a cast, the main character's `visual_anchor` is used instead).
    pub visual_style: &'static str,

    /// Target total length in seconds (min, max).
    pub duration_secs: (u16, u16),
    /// Number of scenes (min, max).
    pub scene_count: (u8, u8),

    /// Extra rules appended to the Writer prompt (safety, brand, cultural constraints).
    pub content_rules: &'static [&'static str],

    /// Recurring characters. Empty = narrator-only video voiced with `CONFIG.voice.base_voice`.
    pub characters: &'static [Character],

    pub publish: PublishProfile,
}

#[derive(Debug)]
pub struct PublishProfile {
    pub description: &'static str,
    pub tags: &'static [&'static str],
    /// YouTube category id, e.g. "27" Education, "28" Science & Technology, "22" People & Blogs.
    pub youtube_category: &'static str,
    /// YouTube "made for kids" flag. Must be true for content aimed at children (COPPA).
    pub made_for_kids: bool,
}

impl StudioProfile {
    pub fn has_cast(&self) -> bool {
        self.characters.iter().any(|c| c.enabled)
    }

    pub fn character(&self, name: &str) -> Option<&'static Character> {
        self.characters.iter().find(|c| c.enabled && c.name == name)
    }
}

/// Look up a profile by id.
pub fn profile_by_id(id: &str) -> Option<&'static StudioProfile> {
    PROFILES.iter().find(|p| p.id == id)
}

/// The profile selected by `STUDIO_PROFILE`, or the default.
/// Returns an error for an unknown id so a typo doesn't silently produce the wrong kind of video.
pub fn active_profile() -> Result<&'static StudioProfile, String> {
    match env::var("STUDIO_PROFILE") {
        Ok(id) if !id.trim().is_empty() => {
            let id = id.trim();
            PROFILES.iter().find(|p| p.id == id).ok_or_else(|| {
                let known = PROFILES.iter().map(|p| p.id).collect::<Vec<_>>().join(", ");
                format!("Unknown STUDIO_PROFILE '{}'. Known profiles: {}", id, known)
            })
        }
        _ => Ok(default_profile()),
    }
}

fn default_profile() -> &'static StudioProfile {
    PROFILES
        .iter()
        .find(|p| p.id == DEFAULT_PROFILE_ID)
        .expect("DEFAULT_PROFILE_ID must exist in PROFILES")
}

pub static PROFILES: &[StudioProfile] = &[
    // Character-driven educational stories for young children (the original show).
    StudioProfile {
        id: "kids_edu",
        name: "Bin's Family Adventures",
        format: "a short animated educational story for young children, told through a recurring family cast",
        audience: "children aged 4-8 and their parents",
        tone: "warm, playful, curious, encouraging",
        topic_brief: "a fresh educational topic a young child would be curious about",
        language: "English",
        visual_style: "a 3D Pixar style illustration, bright colors, soft lighting, child-friendly",
        duration_secs: (45, 60),
        scene_count: (3, 5),
        content_rules: &[
            "Every scene must contain dialogue between characters.",
            "Content and visual prompts must be kid-friendly.",
            "Suitable for the main character's age.",
            "No Chinese elements.",
            "Use English context only.",
        ],
        characters: KIDS_EDU_CAST,
        publish: PublishProfile {
            description: "An AI-generated educational short movie for kids",
            tags: &["ai movie", "english", "vietnamese", "education", "children"],
            youtube_category: "27",
            made_for_kids: true,
        },
    },

    // Narrator-only explainer for a general audience (no cast).
    StudioProfile {
        id: "explainer",
        name: "Quick Explainers",
        format: "a short narrated explainer video that makes one idea clear in under a minute",
        audience: "curious adults and teenagers",
        tone: "clear, confident, friendly, no filler",
        topic_brief: "a surprising 'how does it work' or 'why does it happen' question about science, technology, or everyday life",
        language: "English",
        visual_style: "a clean cinematic digital illustration, high detail, vertical 9:16 composition, no text",
        duration_secs: (40, 60),
        scene_count: (4, 6),
        content_rules: &[
            "Open with a hook in the first scene.",
            "Each scene explains one step of the idea.",
            "Facts must be accurate; avoid speculation.",
            "Visual prompts must not contain any written text or logos.",
        ],
        characters: &[],
        publish: PublishProfile {
            description: "A quick AI-generated explainer",
            tags: &["explainer", "science", "technology", "shorts", "learn"],
            youtube_category: "28",
            made_for_kids: false,
        },
    },
];

/// Recurring cast of the `kids_edu` profile.
pub const KIDS_EDU_CAST: &[Character] = &[
    Character {
        enabled: true,
        spotlight: true,

        name: "Quốc Khánh",
        nickname: "Bin",
        voice_id: &CONFIG.voice.base_voice,
        birth_year: 2020,

        role: "Protagonist",
        family_role: "child",
        story_function: "Main protagonist who initiates most adventures and learning experiences.",

        profession: "Student",
        topic_domain: "Curiosity, Exploration, Everyday Learning",

        catchphrase: "I want to try it!",
        speaking_style: "Very energetic, speaks quickly, asks many 'why' questions, expresses excitement openly.",
        visual_anchor: "a 3D Pixar style illustration of a cute Vietnamese boy age 6, short black hair, bright curious eyes, yellow t-shirt, blue shorts, energetic pose, white background",

        strengths: "Curious, brave, friendly, imaginative, willing to help others.",
        weaknesses: "Impulsive, impatient, easily distracted, sometimes acts before thinking.",
        core_values: "Curiosity, friendship, courage, kindness.",

        relationship_prompt: "Bin is the son of Nam and Diem Suong. He is the best friend of Hoang Yen. Hoang Yen often helps and protects him when he gets into trouble.",
        personality_prompt: "A lively and energetic Vietnamese boy who loves exploring everything around him and learning through adventure.",
    },

    Character {
        enabled: true,
        spotlight: false,

        name: "Hoàng Yến",
        nickname: "Yến",
        voice_id: &CONFIG.voice.base_voice, // Bella
        birth_year: 2020,

        role: "Best Friend",
        family_role: "friend",
        story_function: "Voice of reason and emotional support for Bin.",

        profession: "Student",
        topic_domain: "Learning, Friendship, Kindness",

        catchphrase: "Để mình giúp nhé!",
        speaking_style: "Gentle, polite, calm, encouraging and supportive.",
        visual_anchor: "a 3D Pixar style illustration of a cute Vietnamese girl age 6, long black hair with pink ribbons, pink dress, holding books, warm smile, white background",

        strengths: "Intelligent, hardworking, loyal, responsible, brave when protecting friends.",
        weaknesses: "Can worry too much, dislikes conflict, sometimes overthinks.",
        core_values: "Kindness, friendship, honesty, responsibility.",

        relationship_prompt: "Hoang Yen is Bin's classmate and best friend. She often helps him solve problems and stands up for him when needed.",
        personality_prompt: "A sweet and intelligent girl who loves learning and always supports her friends.",
    },

    Character {
        enabled: true,
        spotlight: false,

        name: "Nam",
        nickname: "Ba Nam",
        voice_id: &CONFIG.voice.base_voice,
        birth_year: 1990,

        role: "Father",
        family_role: "father",
        story_function: "Mentor figure who explains concepts and guides Bin's learning.",

        profession: "Software Engineer",
        topic_domain: "Technology, Logic, Problem Solving",

        catchphrase: "What do you think about this?",
        speaking_style: "Calm, logical, patient, explains things step by step.",
        visual_anchor: "a 3D Pixar style illustration of a friendly Vietnamese father in his mid 30s, glasses, polo shirt, holding a laptop, white background",

        strengths: "Wise, patient, analytical, dependable.",
        weaknesses: "Perfectionist, strict with rules, sometimes too serious.",
        core_values: "Responsibility, honesty, critical thinking, discipline.",

        relationship_prompt: "Nam is Bin's father and Diem Suong's husband. Bin often asks him questions about the world.",
        personality_prompt: "A thoughtful software engineer who enjoys teaching children how things work.",
    },

    Character {
        enabled: true,
        spotlight: false,

        name: "Diễm Sương",
        nickname: "Mẹ Sương",
        voice_id: &CONFIG.voice.base_voice,
        birth_year: 1993,

        role: "Mother",
        family_role: "mother",
        story_function: "Emotional anchor of the family and teacher of life lessons.",

        profession: "Elementary School Teacher",
        topic_domain: "Education, Family, Life Skills",

        catchphrase: "Listen to me!",
        speaking_style: "Warm, caring, nurturing, occasionally firm when teaching lessons.",
        visual_anchor: "a 3D Pixar style illustration of a gentle Vietnamese mother in her early 30s, long black hair, pastel blouse, holding a book and cooking spoon, warm smile, white background",

        strengths: "Patient, loving, organized, empathetic.",
        weaknesses: "Can become worried easily, sometimes overprotective.",
        core_values: "Family, respect, kindness, good habits.",

        relationship_prompt: "Diem Suong is Bin's mother and Nam's wife. She helps Bin learn responsibility and good behavior.",
        personality_prompt: "A caring elementary school teacher who loves her family and wants children to grow into kind people.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn profiles_are_well_formed() {
        assert!(PROFILES.iter().any(|p| p.id == DEFAULT_PROFILE_ID), "default profile missing");

        let mut ids = HashSet::new();
        for p in PROFILES {
            assert!(ids.insert(p.id), "duplicate profile id: {}", p.id);
            assert!(!p.id.is_empty() && p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "profile id must be a lowercase slug (used in folder names): {}", p.id);
            assert!(p.duration_secs.0 > 0 && p.duration_secs.0 <= p.duration_secs.1, "bad duration_secs in {}", p.id);
            assert!(p.scene_count.0 > 0 && p.scene_count.0 <= p.scene_count.1, "bad scene_count in {}", p.id);

            if p.has_cast() {
                assert!(p.characters.iter().any(|c| c.enabled && c.spotlight),
                    "profile {} has a cast but no enabled spotlight character", p.id);
                let mut names = HashSet::new();
                for c in p.characters {
                    assert!(names.insert(c.name), "duplicate character name in {}: {}", p.id, c.name);
                    assert_ne!(c.name, NARRATOR, "'{}' is reserved for narrator-only profiles", NARRATOR);
                }
            }
        }
    }
}
