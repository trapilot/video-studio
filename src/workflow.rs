use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::str::FromStr;

use crate::config::CONFIG;
use crate::enums::*;
use crate::models::*;
use crate::profiles::NARRATOR;
use crate::renderer::*;
use crate::helper::*;
use crate::db::DbManager;
use crate::uploader;


pub async fn run_agent_workflow(
    client: &Client,
    db: &DbManager,
    state: &mut VideoState
) -> Result<VideoArtifact, String> {
        println!(
            "▶️ Run workflow | node: {:?} | retry: {} | topic: {}",
            state.current_node,
            state.meta.retry_count,
            state.target_topic,
        );

        let profile = state.profile()?;

        loop {
            match state.current_node {
                AgentNode::Planner => {
                    println!("🧠[Planner] Creating topic for profile \"{}\"...", profile.id);

                    // Extract the history and pass it to the Task.
                    let history_list = db.get_recent_topics(&state.profile_id, 20)
                        .await
                        .unwrap_or_default();

                    let cast_rules = if profile.has_cast() {
                        r#"
                        4. main_character must be one of the SPOTLIGHT CHARACTERS.
                        5. spotlight_characters must come from SPOTLIGHT CHARACTERS, depending on what suits the story.
                        6. supporting_characters must come from RELATION CHARACTERS, only if the story needs them.
                        7. Only the characters provided may be used.
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
                        let spotlight_chars = profile.characters
                            .iter()
                            .filter(|c| c.enabled && c.spotlight)
                            .map(|c| format!(
                                "- {} ({})\n  Age: {}\n  Personality: {}\n  Relations: {}",
                                c.name,
                                c.topic_domain,
                                now_age(c.birth_year),
                                c.personality_prompt,
                                c.relationship_prompt
                            ))
                            .collect::<Vec<_>>()
                            .join("\n");

                        let relation_chars = profile.characters
                            .iter()
                            .filter(|c| c.enabled && !c.spotlight)
                            .map(|c| format!(
                                "- {} ({})\n  Age: {}\n  Role: {}\n  Relations: {}",
                                c.name,
                                c.profession,
                                now_age(c.birth_year),
                                c.role,
                                c.relationship_prompt
                            ))
                            .collect::<Vec<_>>()
                            .join("\n");

                        format!("SPOTLIGHT CHARACTERS:\n{}\n\nRELATION CHARACTERS:\n{}\n\n", spotlight_chars, relation_chars)
                    } else {
                        String::new()
                    };

                    let user = format!(r#"
                        {}PREVIOUS TOPICS:
                        {}

                        Tasks:
                        1. Create {}.
                        2. Choose the cast if this format has one.
                        3. Returns JSON in the correct format.
                        "#,
                        cast_section,
                        history_list.is_empty()
                            .then(|| "Not yet".to_string())
                            .unwrap_or_else(|| history_list.join(", ")),
                        profile.topic_brief,
                    );
                    
                    let resp = build_content(client, &system, &user, true).await?;
                    let parsed: Value = serde_json::from_str(&resp).map_err(|e| ctx("JSON parse", e))?;
                    
                    state.target_topic = json_str(&parsed, "topic")?.trim().to_string();
                    if state.target_topic.is_empty() {
                        return Err(ctx("Planner", "Empty topic"));
                    }

                    if profile.has_cast() {
                        state.main_character = json_str(&parsed, "main_character")?;
                        state.spotlight_characters = json_str_array(&parsed, "spotlight_characters")?;
                        state.supporting_characters = json_str_array(&parsed, "supporting_characters")?;

                        // Validate casting against the profile's cast so a hallucinated name
                        // triggers a retry instead of a failure in the Writer.
                        if !profile.character(&state.main_character).is_some_and(|c| c.spotlight) {
                            return Err(ctx("Planner", format!("Unknown main character: {}", state.main_character)));
                        }
                        if let Some(bad) = state.spotlight_characters
                            .iter()
                            .chain(state.supporting_characters.iter())
                            .find(|n| profile.character(n).is_none())
                        {
                            return Err(ctx("Planner", format!("Unknown character: {}", bad)));
                        }
                    } else {
                        // Narrator-only format: ignore whatever names the LLM returned.
                        state.main_character = String::new();
                        state.spotlight_characters = vec![];
                        state.supporting_characters = vec![];
                    }

                    println!("🧠[Planner] Topic will be \"{}\"", state.target_topic);
                    
                    state.current_node = AgentNode::Writer;
                    let _ = db.save_state(&state.reset()).await;
                }

                AgentNode::Writer => {
                    println!("✍️ [Writer] Generating video artifact...");

                    // With a cast, the main character's anchor sets the look; otherwise the profile's style does.
                    let (visual_style, cast_block, cast_rules, user_cast) = if profile.has_cast() {
                        let main_char = profile
                            .character(&state.main_character)
                            .ok_or_else(|| ctx("Writer", format!("Unknown main character: {}", state.main_character)))?;

                        let spotlight_chars = describe_characters(profile.characters, &state.spotlight_characters);
                        let supporting_chars = describe_characters(profile.characters, &state.supporting_characters);

                        let all_chars = profile.characters
                            .iter()
                            .filter(|c| c.enabled)
                            .map(|c| format!("- {}: {}", c.name, c.personality_prompt))
                            .collect::<Vec<_>>()
                            .join("\n");

                        let cast_block = format!(
                            "MAIN CHARACTER:\n{}\n\nSPOTLIGHT CHARACTERS:\n{}\n\nSUPPORTING CHARACTERS:\n{}",
                            main_char.personality_prompt,
                            spotlight_chars,
                            supporting_chars,
                        );
                        let cast_rules = vec![
                            "Use only the provided characters as speakers.".to_string(),
                            "The main character must drive the story.".to_string(),
                        ];
                        let user_cast = format!(
                            "MAIN CHARACTER:\n{}\n\nSPOTLIGHT CHARACTERS:\n{}\n\nSUPPORTING CHARACTERS:\n{}\n\nALL CHARACTER PERSONALITIES:\n{}\n\n",
                            state.main_character,
                            state.spotlight_characters.join(", "),
                            state.supporting_characters.join(", "),
                            all_chars,
                        );
                        (main_char.visual_anchor, cast_block, cast_rules, user_cast)
                    } else {
                        let cast_block = format!(
                            "VOICE:\nThere is no cast. Every voice_segment must use \"speaker\": \"{}\", speaking directly to the viewer.",
                            NARRATOR,
                        );
                        let cast_rules = vec![
                            "No on-screen dialogue between characters; narration only.".to_string(),
                        ];
                        (profile.visual_style, cast_block, cast_rules, String::new())
                    };
                    
                    let scene_motions = Motion::ALL
                        .iter()
                        .map(|m| format!("- {}", m))
                        .collect::<Vec<_>>()
                        .join("\n");

                    let scene_transitions = Transition::ALL
                        .iter()
                        .map(|m| format!("- {}", m))
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Generic rules first, then cast rules, then the profile's own rules.
                    let rules = [
                        format!("Entire content must be in {}.", profile.language),
                        format!("Video length: {}-{} seconds.", profile.duration_secs.0, profile.duration_secs.1),
                        format!("The video must have {}-{} scenes.", profile.scene_count.0, profile.scene_count.1),
                        "Every scene must have at least one voice_segment.".to_string(),
                        "Each visual_prompt describes one still image (it is animated with camera motion).".to_string(),
                        "Keep the main subject centered: images are cropped to a vertical 9:16 frame.".to_string(),
                        "Never put written text, captions or logos in a visual_prompt.".to_string(),
                        "Never invent new motion names.".to_string(),
                        "Never invent new transition names.".to_string(),
                    ]
                        .into_iter()
                        .chain(cast_rules)
                        .chain(profile.content_rules.iter().map(|r| r.to_string()))
                        .enumerate()
                        .map(|(i, r)| format!("{}. {}", i + 1, r))
                        .collect::<Vec<_>>()
                        .join("\n");

                    let system = format!(r#"
                        You are a senior scriptwriter for "{}", producing {}. Create the FINAL JSON artifact.

                        AUDIENCE: {}
                        TONE: {}

                        VISUAL STYLE (apply it to every visual_prompt so all scenes look consistent):
                        {}

                        {}

                        MOTION MUST BE ONE OF:
                        {}

                        TRANSITION MUST BE ONE OF:
                        {}

                        Motion guidelines:
                        - Dialogue scene -> ZoomIn
                        - Walking scene -> PanLeft or PanRight
                        - Landscape scene -> KenBurns
                        - Emotional moment -> ZoomIn
                        - Ending scene -> ZoomOut

                        Transition guidelines:
                        - Same location -> Fade
                        - New location -> SlideLeft
                        - Time skip -> FadeBlack
                        - Important reveal -> CircleOpen

                        Rules:
                        {}

                        Return STRICT JSON:
                        {{
                        "title": "...",
                        "scenes": [
                            {{
                            "scene_id": 1,
                            "duration": 5,
                            "transition": "...",
                            "motion": "...",
                            "visual_prompt": "...",
                            "voice_segments": [
                                {{
                                    "speaker": "...",
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
                        visual_style,
                        cast_block,
                        scene_motions,
                        scene_transitions,
                        rules,
                    );

                    let user = format!(r#"
                        TOPIC: {}

                        {}Create:
                            - complete script
                            - scene breakdown
                            - visual prompts
                            - voice segments

                        Return JSON only.
                        "#,
                        state.target_topic,
                        user_cast,
                    );

                    let resp = build_content(client, &system, &user, true).await?;

                    let mut artifact: VideoArtifact = serde_json::from_str(&resp)
                        .map_err(|e| ctx("JSON parse", e))?;

                    normalize_artifact(&mut artifact)?;

                    // Speakers without a cast entry (including the narrator) use the base voice.
                    for scene in &mut artifact.scenes {
                        for segment in &mut scene.voice_segments {
                            segment.voice_id = Some(
                                profile
                                    .character(&segment.speaker)
                                    .map(|c| c.voice_id.to_string())
                                    .unwrap_or(CONFIG.voice.base_voice.to_string())
                            );
                        }
                    }

                    state.final_json = Some(artifact);
                    state.current_node = AgentNode::Builder;
                    let _ = db.save_state(&state.reset()).await;
                }

                AgentNode::Builder => {
                    println!("🔧[Builder] Generating assets...");
                    let artifact = state.final_json
                        .as_ref()
                        .ok_or_else(|| ctx("Builder", "Missing script artifact"))?;

                    let timelines = build_timelines(
                        &client,
                        state.target_path.clone(),
                        artifact.scenes.clone(),
                        VoiceMode::SingleVoice,
                    ).await?;

                    state.video_timelines = timelines;
                    state.current_node = AgentNode::Renderer;
                    let _ = db.save_state(&state.reset()).await;
                }

                AgentNode::Renderer => {
                    println!("🎥 [Renderer] Rendering video...");
                    
                    let final_video = ffmpeg_render(
                        state.target_path.clone(),
                        &state.video_timelines,
                    ).await?;

                    state.video_path = final_video;
                    state.current_node = AgentNode::Publisher;
                    let _ = db.save_state(&state.reset()).await;
                }

                AgentNode::Publisher => {
                    println!("📤 [Publisher] Publishing the video...");

                    let title = state.final_json
                        .as_ref()
                        .map(|a| a.title.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| state.target_topic.clone());

                    // Skip platforms that already succeeded, so a retry never posts twice.
                    let youtube = async {
                        if state.youtube_uploaded {
                            println!("📤 [Publisher] YouTube already uploaded, skipping");
                            Ok(())
                        } else {
                            uploader::upload_to_youtube(client, &state.video_path, &title, &profile.publish).await
                        }
                    };
                    let tiktok = async {
                        if state.tiktok_uploaded {
                            println!("📤 [Publisher] TikTok already uploaded, skipping");
                            Ok(())
                        } else {
                            uploader::upload_to_tiktok(client, &state.video_path, &title).await
                        }
                    };
                    let (yt_res, tt_res) = tokio::join!(youtube, tiktok);

                    if let Err(e) = &tt_res {
                        eprintln!("🔴 TikTok error: {}", e);
                    }
                    state.youtube_uploaded = yt_res.is_ok();
                    state.tiktok_uploaded = tt_res.is_ok();

                    // Only YouTube is required. The scheduler saves the flags with the retry,
                    // so the next attempt skips whatever already went out.
                    if let Err(e) = yt_res {
                        return Err(ctx("Publisher", format!("YouTube upload failed: {}", e)));
                    }
                    
                    state.current_node = AgentNode::End;
                    let _ = db.save_state(&state.reset()).await;
                }

                AgentNode::End => {
                    return state.final_json
                        .clone()
                        .ok_or_else(|| ctx("End", "Missing script artifact"));
                }
            };
        }
}

/// Make the Writer's output safe for the Builder and Renderer: at least one scene,
/// unique sequential scene ids (they become file names), non-empty voice-over in every
/// scene, and only known motion/transition names (unknown ones fall back to defaults).
fn normalize_artifact(artifact: &mut VideoArtifact) -> Result<(), String> {
    if artifact.scenes.is_empty() {
        return Err(ctx("Writer", "Script has no scenes"));
    }
    if artifact.scenes.len() > u8::MAX as usize {
        return Err(ctx("Writer", format!("Too many scenes: {}", artifact.scenes.len())));
    }

    artifact.scenes.sort_by_key(|s| s.scene_id);
    for (i, scene) in artifact.scenes.iter_mut().enumerate() {
        scene.scene_id = (i + 1) as u8;

        scene.voice_segments.retain(|v| !v.text.trim().is_empty());
        if scene.voice_segments.is_empty() {
            return Err(ctx("Writer", format!("Scene {} has no voice-over", scene.scene_id)));
        }
        if scene.visual_prompt.trim().is_empty() {
            return Err(ctx("Writer", format!("Scene {} has no visual prompt", scene.scene_id)));
        }

        if Motion::from_str(&scene.motion).is_err() {
            println!("✍️ [Writer] Scene {}: unknown motion '{}', using None", scene.scene_id, scene.motion);
            scene.motion = Motion::None.to_string();
        }
        if Transition::from_str(&scene.transition).is_err() {
            println!("✍️ [Writer] Scene {}: unknown transition '{}', using {}", scene.scene_id, scene.transition, Transition::DEFAULT);
            scene.transition = Transition::DEFAULT.to_string();
        }
    }
    Ok(())
}

/// Prompt block describing the named characters, in cast order.
fn describe_characters(cast: &[Character], names: &[String]) -> String {
    let wanted: HashSet<&str> = names.iter().map(|s| s.as_str()).collect();
    cast.iter()
        .filter(|c| c.enabled && wanted.contains(c.name))
        .map(|c| format!(
            "- {}
  Age: {}
  Personality: {}
  Role: {}
  Relations: {}",
            c.name,
            now_age(c.birth_year),
            c.personality_prompt,
            c.role,
            c.relationship_prompt
        ))
        .collect::<Vec<_>>()
        .join("
")
}

fn json_str(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ctx("JSON", format!("Missing string field '{}'", key)))
}

fn json_str_array(v: &Value, key: &str) -> Result<Vec<String>, String> {
    v[key]
        .as_array()
        .ok_or_else(|| ctx("JSON", format!("Missing array field '{}'", key)))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| ctx("JSON", format!("Non-string item in '{}'", key)))
        })
        .collect()
}
