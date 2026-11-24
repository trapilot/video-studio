use std::sync::Arc;
use async_trait::async_trait;

use crate::AppState;
use crate::agent::*;
use crate::models::*;
use crate::entities::*;
use crate::provider;


pub struct PlannerAgent;

#[async_trait]
impl Agent for PlannerAgent {
    async fn run(&self, state: &Arc<AppState>, job: &Job) -> Result<(), AgentError> {
        let profile = job_profile(state, job).await?;
        println!("🧠 [Planner] Creating topic for profile \"{}\"...", profile.id);

        // Topics of this profile from the last 30 days, to avoid repeats.
        let history_list = state.services.db
            .get_recent_topics(profile.id, 30, 30)
            .await
            .unwrap_or_default();

        let cast_rules = if profile.has_cast() {
            r#"
            4. main_character must be one of the SPOTLIGHT CHARACTERS.
            5. spotlight_characters must come from SPOTLIGHT CHARACTERS, depending on what suits the story.
            6. supporting_characters must come from RELATION CHARACTERS, only if the story needs them.
            7. Only the characters provided may be used. Use their exact names.
            8. The topic must fit the main character."#
        } else {
            r#"
            4. This format has no cast: return "main_character": "" and empty arrays for both character lists."#
        };

        let system = format!(r#"
            You are the Content Director for "{}", which publishes {}.
            Audience: {}
            Tone: {}

            Returns JSON:
            {{
                "topic": "...",
                "main_character": "...",
                "spotlight_characters": ["...", "..."],
                "supporting_characters": ["...", "..."]
            }}
            Rules:
            1. Write the topic in {}.
            2. The topic must suit the audience and the format.
            3. Avoid repeating or closely paraphrasing previous topics.{}
            "#,
            profile.name,
            profile.format,
            profile.audience,
            profile.tone,
            profile.language,
            cast_rules,
        );

        let cast_section = if profile.has_cast() {
            let spotlight_chars = profile.enabled_cast()
                .filter(|c| c.spotlight)
                .map(|c| format!(
                    "- {} ({})\n  Age: {}\n  Personality: {}\n  Relations: {}",
                    c.name, c.topic_domain, c.age_label(), c.personality_prompt, c.relationship_prompt
                ))
                .collect::<Vec<_>>()
                .join("\n");

            let relation_chars = profile.enabled_cast()
                .filter(|c| !c.spotlight)
                .map(|c| format!(
                    "- {} ({})\n  Age: {}\n  Role: {}\n  Personality: {}\n  Relations: {}",
                    c.name, c.profession, c.age_label(), c.role, c.personality_prompt, c.relationship_prompt
                ))
                .collect::<Vec<_>>()
                .join("\n");

            format!("SPOTLIGHT CHARACTERS:\n{}\n\nRELATION CHARACTERS:\n{}\n\n", spotlight_chars, relation_chars)
        } else {
            String::new()
        };

        let prompt = format!(r#"
            {}PREVIOUS TOPICS:
            {}

            Tasks:
            1. Create {}.
            2. Choose the cast if this format has one.
            3. Returns JSON in the correct format.
            "#,
            cast_section,
            if history_list.is_empty() { "Not yet".to_string() } else { history_list.join(", ") },
            profile.topic_brief,
        );

        let resp = self
            .execute(&state, &system, &prompt)
            .await
            .map_err(|e| AgentError::Execute(e.to_string()))?;

        let mut brief: Brief =
            serde_json::from_str(&resp)
            .map_err(|e| AgentError::Decode(e.to_string()))?;

        validate_brief(profile, &mut brief)?;
        println!("🧠 [Planner] Topic will be \"{}\"", brief.topic);

        let payload =
            serde_json::to_string(&brief)
                .map_err(|e| AgentError::Encode(e.to_string()))?;

        state.services.db
            .handoff_job(job, AgentType::Writer, payload)
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        state.services.db
            .save_topic(&job.production_id, brief.topic.clone())
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        Ok(())
    }
}

impl PlannerAgent {
    async fn execute(
        &self,
        state: &Arc<AppState>,
        system: &str,
        prompt: &str,
    ) -> Result<String, String> {
        let provider = &provider::Provider::Gemini;
        let guard = match state.services.providers.acquire(&provider).await {
            Some(v) => v,
            None => {
                return Err(format!("{} is acquired", &provider.to_string()));
            }
        };

        let req = provider::ProviderRequest::Script(provider::ScriptRequest {
            system: system.to_string(),
            prompt: prompt.to_string(),
            json_mode: true,
            temperature: None,
            max_tokens: None,
        });

        let rsp = guard
            .call(req)
            .await
            .map_err(|e| e.to_string())?;

        rsp
            .into_text()
            .map_err(|e| e.to_string())
        
    }
}
/// Reject an unusable plan (so the job is retried) and canonicalize character names.
fn validate_brief(profile: &StudioProfile, ctx: &mut Brief) -> Result<(), AgentError> {
    ctx.topic = ctx.topic.trim().to_string();
    if ctx.topic.is_empty() {
        return Err(AgentError::Execute("Planner returned an empty topic".to_string()));
    }

    if !profile.has_cast() {
        // Narrator-only format: ignore whatever names the LLM returned.
        ctx.main_character.clear();
        ctx.spotlight_characters.clear();
        ctx.supporting_characters.clear();
        return Ok(());
    }

    let main = profile
        .character(&ctx.main_character)
        .filter(|c| c.spotlight)
        .ok_or_else(|| AgentError::Execute(format!("Unknown main character: {}", ctx.main_character)))?;
    ctx.main_character = main.name.clone();

    // Accept nicknames, store full names.
    for list in [&mut ctx.spotlight_characters, &mut ctx.supporting_characters] {
        for name in list.iter_mut() {
            let c = profile
                .character(name)
                .ok_or_else(|| AgentError::Execute(format!("Unknown character: {}", name)))?;
            *name = c.name.clone();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(main: &str, spot: &[&str], sup: &[&str]) -> Brief {
        Brief {
            topic: " Why is the sky blue? ".into(),
            main_character: main.into(),
            spotlight_characters: spot.iter().map(|s| s.to_string()).collect(),
            supporting_characters: sup.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn validates_and_canonicalizes_cast() {
        crate::entities::character::init_for_tests();
        let kids = profile_by_id("kids_edu").unwrap();

        let mut ok = ctx("Bin", &["Bin"], &["Nam"]);
        validate_brief(kids, &mut ok).unwrap();
        assert_eq!(ok.topic, "Why is the sky blue?");
        assert_eq!(ok.main_character, "Quốc Khánh");
        assert_eq!(ok.spotlight_characters, vec!["Quốc Khánh"]);

        assert!(validate_brief(kids, &mut ctx("Nam", &[], &[])).is_err(), "not a spotlight character");
        assert!(validate_brief(kids, &mut ctx("Bin", &[], &["Ghost"])).is_err(), "unknown name");

        let explainer = profile_by_id("explainer").unwrap();
        let mut narr = ctx("Someone", &["X"], &["Y"]);
        validate_brief(explainer, &mut narr).unwrap();
        assert!(narr.main_character.is_empty() && narr.spotlight_characters.is_empty());
    }
}
