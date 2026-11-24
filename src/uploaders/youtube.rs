use std::sync::Arc;

use crate::db::DbManager;
use crate::uploader::*;
use crate::oauth;

pub struct YoutubeUploader;

impl YoutubeUploader {
    pub fn new() -> Self {
        Self { }
    }
}

#[async_trait::async_trait]
impl Uploader for YoutubeUploader {
    fn channel(&self) -> Channel {
        Channel::Youtube
    }
    
    async fn upload(
        &self,
        db: &Arc<DbManager>,
        video_path: &str,
        title: &str,
        config: ChannelConfig,
    ) -> Result<(), String> {
        println!("Uploading Youtube");

        let oauth_token = oauth::get_youtube_token(&db)
            .await
            .map_err(|e| e.to_string())?;

        let access_token = oauth_token
            .access_token
            .ok_or_else(|| "Missing youtube access token".to_string())?;

        self.upload_internal(
            video_path,
            title,
            &config,
            &access_token,
        )
        .await
    }
}

impl YoutubeUploader {
    async fn upload_internal(
        &self,
        video_path: &str,
        title: &str,
        config: &ChannelConfig,
        access_token: &str,
    ) -> Result<(), String> {
        let metadata = match config {
            ChannelConfig::Youtube {
                category_id,
                description,
                tags,
                made_for_kids,
            } => serde_json::json!({
                "snippet": {
                    "title": youtube_title(title),
                    "description": description,
                    "tags": tags,
                    "categoryId": category_id,
                },
                "status": {
                    "privacyStatus": "public",
                    "madeForKids": made_for_kids,
                    "selfDeclaredMadeForKids": made_for_kids
                }
            }),
            _ => return Err("Youtube config incorrect".to_string()),
        };

        let video_bytes = tokio::fs::read(video_path)
            .await
            .map_err(|e| format!("read {}: {}", video_path, e))?;
        if video_bytes.is_empty() {
            return Err(format!("{} is empty", video_path));
        }

        let boundary = "foo_bar_baz";

        let mut full_body = Vec::new();

        full_body.extend(format!(
            "--{b}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n",
            b = boundary
        ).as_bytes());

        full_body.extend(metadata.to_string().as_bytes());
        full_body.extend(b"\r\n");

        full_body.extend(format!(
            "--{b}\r\nContent-Type: video/mp4\r\n\r\n",
            b = boundary
        ).as_bytes());

        full_body.extend(&video_bytes);

        full_body.extend(format!("\r\n--{b}--\r\n", b = boundary).as_bytes());

        let res = reqwest::Client::new()
            .post("https://www.googleapis.com/upload/youtube/v3/videos?uploadType=multipart&part=snippet,status")
            .bearer_auth(access_token)
            .header("Content-Type", format!("multipart/related; boundary={}", boundary))
            .body(full_body)
            .send()
            .await
            .map_err(|e| format!("YouTube upload failed: {}", e.without_url()))?;

        let status = res.status();
        let text = res.text().await.map_err(|e| e.to_string())?;

        if status.is_success() {
            println!("Youtube Upload OK!");
            Ok(())
        } else {
            Err(format!("upload failed: {status}, body: {text}"))
        }
    }
}

/// YouTube rejects titles over 100 characters or containing angle brackets.
fn youtube_title(title: &str) -> String {
    let clean: String = title.chars().filter(|c| *c != '<' && *c != '>').collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "Untitled".to_string()
    } else {
        crate::helper::truncate_chars(clean, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_title_is_sanitized() {
        assert_eq!(youtube_title(" <Why> stars twinkle "), "Why stars twinkle");
        assert_eq!(youtube_title(&"é".repeat(150)).chars().count(), 100);
        assert_eq!(youtube_title("<>"), "Untitled");
    }
}
