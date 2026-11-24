
use std::sync::Arc;
use async_trait::async_trait;

use crate::AppState;
use crate::agent::*;
use crate::models::*;


pub struct ProducerAgent;

#[async_trait]
impl Agent for ProducerAgent {
    async fn run(&self, _state: &Arc<AppState>, job: &Job) -> Result<(), AgentError> {
        println!("Running production...");
        Ok(())
    }
}
