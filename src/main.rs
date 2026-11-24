mod db;
mod api;
mod enums;
mod config;
mod models;
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

    let profile = profiles::active_profile()?;

    let client = reqwest::Client::new();
    let db = Arc::new(DbManager::new("data_history.db")?);

    println!("🚀 Video Studio v{} | profile: {} ({})", env!("CARGO_PKG_VERSION"), profile.id, profile.name);
    
    scheduler::run_scheduler(client, db, profile).await;

    Ok(())
}