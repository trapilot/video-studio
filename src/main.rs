mod db;
mod agent;
mod oauth;
mod config;
mod provider;
mod uploader;
mod workflow;

mod agents;
mod models;
mod entities;
mod providers;
mod uploaders;

use std::sync::Arc;


#[derive(Clone)]
pub struct AppState {
    pub services: Arc<Services>,
    pub config: Arc<config::Config>,
    /// Profile for new workflows (`STUDIO_PROFILE`). Running workflows use their own.
    pub profile: &'static entities::StudioProfile,
}

pub struct Services {
    pub db: Arc<db::DbManager>,
    pub providers: Arc<provider::ProviderManager>,
    pub uploaders: Arc<uploader::UploaderManager>,
}

impl AppState {
    pub async fn build(profile: &'static entities::StudioProfile) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Arc::new(db::DbManager::new("data_history.db").await?);

        let providers = Arc::new(provider::ProviderManager::new());
        providers.register_with_keys(
            Arc::new(providers::GeminiClient),
            2,
            "GEMINI",
        ).await;
        providers.register_with_keys(
            Arc::new(providers::CFWorkerClient),
            10,
            "CF_WORKER",
        ).await;
        providers.register_with_keys(
            Arc::new(providers::ElevenLabsClient),
            2,
            "ELEVEN_LABS",
        ).await;


        let uploaders = Arc::new(uploader::UploaderManager::new());
        uploaders.register(
            Arc::new(uploaders::YoutubeUploader::new())
        ).await;

        let services = Arc::new(Services {
            db,
            providers,
            uploaders,
        });

        Ok(Self {
            services,
            config: Arc::new(config::build_config()),
            profile,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {   
    dotenvy::dotenv().ok(); 
    let data_dir = std::env::var("STUDIO_DATA_DIR")
        .unwrap_or_else(|_| entities::character::DEFAULT_DATA_DIR.to_string());
    entities::character::init(&data_dir)?;
    let profile = entities::active_profile()?;

    let cast = profile.enabled_cast().map(|c| c.name.as_str()).collect::<Vec<_>>();
    println!(
        "🚀 Video Studio v{} | profile: {} ({}) | cast: {}",
        env!("CARGO_PKG_VERSION"),
        profile.id,
        profile.name,
        if cast.is_empty() { "narrator only".to_string() } else { cast.join(", ") },
    );

    let state = Arc::new(AppState::build(profile).await?);

    if !oauth::has_auth_token(&state.services.db).await {
        oauth::start_oauth_server(&state.services.db).await;
    }

    let revert_job = "";
    if !revert_job.is_empty() {
        println!("\nRevert current job: {}\n\n", revert_job);
        state.services.db.revert_job(revert_job).await?;
    }    

    let workflow = workflow::WorkflowEngine::new(state);

    workflow.start().await;
    
    Ok(())
}