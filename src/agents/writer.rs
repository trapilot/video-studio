use std::sync::Arc;
use async_trait::async_trait;

use crate::AppState;
use crate::agent::*;
use crate::models::*;
use crate::entities::*;
use crate::provider;


pub struct WriterAgent;

#[async_trait]
impl Agent for WriterAgent {
    async fn run(&self, state: &Arc<AppState>, job: &Job) -> Result<(), AgentError> {
        println!("✍️  [Writer] Generating artifact...");
        
        let profile = job_profile(state, job).await?;

        let story_context: StoryContext =
            serde_json::from_str(&job.payload)
            .map_err(|e| AgentError::Decode(e.to_string()))?;

        // Cast block: everything the character files say about the chosen characters.
        let (cast_block, cast_rules) = if profile.has_cast() {
            let mut names = vec![story_context.main_character.clone()];
            names.extend(story_context.spotlight_characters.iter().cloned());
            names.extend(story_context.supporting_characters.iter().cloned());

            let chosen = profile.enabled_cast()
                .filter(|c| names.iter().any(|n| c.answers_to(n)))
                .map(|c| c.describe())
                .collect::<Vec<_>>()
                .join("\n");

            (
                format!("CHARACTERS IN THIS STORY:\n{}", chosen),
                vec![
                    "Only the characters listed above may appear or speak. Use their exact names.".to_string(),
                    "The main character must drive the story.".to_string(),
                    "Keep each character's appearance consistent with their Appearance in every shot.".to_string(),
                ],
            )
        } else {
            (
                format!(
                    "VOICE:\nThere is no cast. Every dialogue must use \"character\": \"{}\", speaking directly to the viewer. \"actors\" may be empty.",
                    NARRATOR
                ),
                vec!["Narration only; no on-screen dialogue between characters.".to_string()],
            )
        };

        let all_motions = Motion::ALL
            .iter()
            .map(|m| format!("- {}", m))
            .collect::<Vec<_>>()
            .join("\n");

        let all_transitions = Transition::ALL
            .iter()
            .map(|m| format!("- {}", m))
            .collect::<Vec<_>>()
            .join("\n");

        let all_positions = Position::ALL
            .iter()
            .map(|m| format!("- {}", m))
            .collect::<Vec<_>>()
            .join("\n");

        // Generic rules, then cast rules, then the profile's own rules.
        let rules = [
            format!("Entire content must be in {}.", profile.language),
            format!("Total video length: {}-{} seconds.", profile.duration_secs.0, profile.duration_secs.1),
            format!("The storyboard must have {}-{} shots.", profile.shot_count.0, profile.shot_count.1),
            "Each shot should represent only ONE visual idea.".to_string(),
            "Each shot should contain approximately 1-2 spoken sentences, spoken in roughly 3-5 seconds.".to_string(),
            "If a narration is too long, split it into multiple shots.".to_string(),
            "Every shot must have at least one dialogue.".to_string(),
            "Every shot must have a detailed visual_prompt describing one still image suitable for image generation.".to_string(),
            "Keep the main subject centered: images are cropped to a vertical 9:16 frame.".to_string(),
            "Never put written text, captions or logos in a visual_prompt.".to_string(),
            "Describe what each actor is doing and feeling.".to_string(),
            "Camera movement should be simple and realistic.".to_string(),
            "Never invent new motion, transition or position names.".to_string(),
        ]
            .into_iter()
            .chain(cast_rules)
            .chain(profile.content_rules.iter().map(|r| r.to_string()))
            .enumerate()
            .map(|(i, r)| format!("{}. {}", i + 1, r))
            .collect::<Vec<_>>()
            .join("\n");

        let system = format!(r#"
            You are an expert film storyboard artist and AI video director for "{}", producing {}.

            AUDIENCE: {}
            TONE: {}
            VISUAL STYLE: {}

            Your task is to turn the topic into a sequence of SHORT VIDEO SHOTS suitable for TikTok, Reels, and YouTube Shorts.

            {}

            MOTION MUST BE ONE OF:
            {}

            TRANSITION MUST BE ONE OF:
            {}

            ACTOR POSITION MUST BE ONE OF:
            {}

            Motion guidelines:
            - Dialogue scene -> zoomin
            - Walking scene -> panleft or panright
            - Landscape scene -> kenburns
            - Emotional moment -> zoomin
            - Ending scene -> zoomout

            Transition guidelines:
            - Same location -> fade
            - New location -> slideleft
            - Time skip -> fadeblack
            - Important reveal -> circleopen

            Rules:
            {}

            Return STRICT JSON ONLY:
            {{
                "title": "...",
                "shots": [
                    {{
                        "shot_id": 1,
                        "transition": "...",
                        "motion": "...",
                        "environment":
                            {{
                                "location": "...",
                                "time_of_day": "...",
                                "weather": "..."
                            }},
                        "camera":
                            {{
                                "shot_type": "close_up | medium | wide",
                                "angle": "front | low | high | side",
                                "movement": "static | zoom_in | zoom_out | pan_left | pan_right"
                            }},
                        "visual_prompt": "...",
                        "actors": [
                            {{
                                "character": "...",
                                "position": "...",
                                "action": "...",
                                "emotion": "..."
                            }}
                        ],
                        "dialogues": [
                            {{
                                "character": "...",
                                "text": "..."
                            }}
                        ]
                    }}
                ]
            }}
            "#,
            profile.name,
            profile.format,
            profile.audience,
            profile.tone,
            profile.visual_style,
            cast_block,
            all_motions,
            all_transitions,
            all_positions,
            rules,
        );

        let prompt = if profile.has_cast() {
            format!(r#"
            TOPIC: {}

            MAIN CHARACTER: {}
            SPOTLIGHT CHARACTERS: {}
            SUPPORTING CHARACTERS: {}

            Create:
                - complete story
                - shot breakdown
                - visual prompts
                - actors MUST NOT be empty
                - dialogues MUST NOT be empty

            Return JSON only.
            "#,
                story_context.topic,
                story_context.main_character,
                story_context.spotlight_characters.join(", "),
                story_context.supporting_characters.join(", "),
            )
        } else {
            format!(r#"
            TOPIC: {}

            Create:
                - complete narration
                - shot breakdown
                - visual prompts
                - dialogues MUST NOT be empty (speaker "{}")

            Return JSON only.
            "#,
                story_context.topic,
                NARRATOR,
            )
        };

        let resp = self.execute(&state, &system, &prompt)
            .await
            .map_err(|e| AgentError::Execute(e.to_string()))?;

        let storyboard: Storyboard =
            serde_json::from_str(&resp)
            .map_err(|e| AgentError::Decode(e.to_string()))?;

        let payload =
            serde_json::to_string(&storyboard)
            .map_err(|e| AgentError::Encode(e.to_string()))?;

        state.services.db
            .handoff_job(job, AgentType::Builder, payload)
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        Ok(())
    }
}

impl WriterAgent {
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
                return Err(format!("{}", &provider.to_string()));
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