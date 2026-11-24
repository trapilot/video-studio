mod db;
mod api;
mod enums;
mod config;
mod models;
mod characters;
mod profiles;
mod helper;
mod workflow;
mod renderer;
mod uploader;
mod scheduler;

use std::sync::Arc;
use crate::db::DbManager;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let data_dir = std::env::var("STUDIO_DATA_DIR").unwrap_or_else(|_| characters::DEFAULT_DATA_DIR.to_string());
    characters::init(&data_dir)?;
    let profile = profiles::active_profile()?;

    let client = reqwest::Client::new();
    let db = Arc::new(DbManager::new("data_history.db")?);

    println!(
        "🚀 Video Studio v{} | profile: {} ({}) | cast: {}",
        env!("CARGO_PKG_VERSION"),
        profile.id,
        profile.name,
        if profile.has_cast() {
            profile.cast().iter().filter(|c| c.enabled).map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
        } else {
            "narrator only".to_string()
        },
    );
    
    scheduler::run_scheduler(client, db, profile).await;

    Ok(())
}