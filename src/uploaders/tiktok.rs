use std::sync::Arc;

use crate::db::DbManager;
use crate::uploader::*;

pub struct TiktokUploader {
    access_token: String,
}

impl TiktokUploader {
    pub fn new() -> Self {
        Self {
            access_token: std::env::var("TIKTOK_ACCESS_TOKEN").unwrap_or_default(),
        }
    }
}

#[async_trait::async_trait]
impl Uploader for TiktokUploader {
    fn channel(&self) -> Channel {
        Channel::Tiktok
    }

    async fn upload(
        &self,
        db: &Arc<DbManager>,
        video_path: &str,
        title: &str,
        config: ChannelConfig
    ) -> Result<(), String> {
        println!("Uploading Tiktok");

        if self.access_token.trim().is_empty() {
            return Err("Missing TIKTOK_ACCESS_TOKEN".to_string());
        }

        let video_bytes = tokio::fs::read(video_path)
            .await
            .map_err(|e| format!("read {}: {}", video_path, e))?;
        if video_bytes.is_empty() {
            return Err(format!("{} is empty", video_path));
        }
        
        let size = video_bytes.len();

        let init_payload = match &config {
            ChannelConfig::Tiktok {
                privacy_level,
                disable_comment
            } => {
                serde_json::json!({
                    "post_info": {
                        "title": crate::helper::truncate_chars(title, 2200),
                        "privacy_level": privacy_level,
                        "disable_comment": disable_comment,
                    },
                    "source_info": {
                        "source": "FILE_UPLOAD",
                        "video_size": size,
                        "chunk_size": size,
                        "total_chunk_count": 1,
                    }
                })
            }
            _ => {
                return Err("Tiktok config incorrect".to_string());
            }
        };

        let init_res = reqwest::Client::new()
            .post("https://open.tiktokapis.com/v2/post/publish/video/init/")
            .bearer_auth(&self.access_token)
            .header(reqwest::header::CONTENT_TYPE, "application/json; charset=UTF-8")
            .json(&init_payload)
            .send()
            .await
            .map_err(|e| format!("TikTok init request failed: {}", e.without_url()))?;

        let status = init_res.status();
        let body = init_res.text().await.map_err(|e| e.without_url().to_string())?;
        let init_json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|_| format!("TikTok init HTTP {}: {}", status, body))?;

        if init_json["error"]["code"].as_str() != Some("ok") {
            return Err(format!("TikTok init failed (HTTP {}): {}", status, init_json["error"]));
        }

        let upload_url = init_json["data"]["upload_url"]
            .as_str()
            .ok_or_else(|| format!("TikTok init: missing upload_url: {}", init_json))?;

        let put_res = reqwest::Client::new()
            .put(upload_url)
            .header(reqwest::header::CONTENT_TYPE, "video/mp4")
            .header(reqwest::header::CONTENT_LENGTH, size.to_string())
            .header("Content-Range", format!("bytes 0-{}/{}", size - 1, size))
            .body(video_bytes)
            .send()
            .await
            .map_err(|e| format!("TikTok upload failed: {}", e.without_url()))?;

        if put_res.status().is_success() {
            println!("TikTok Upload OK!");
            Ok(())
        } else {
            let status = put_res.status();
            Err(format!("TikTok upload HTTP {}: {}", status, put_res.text().await.unwrap_or_default()))
        }
    }
 }
