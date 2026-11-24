use reqwest::{Client, header};
use serde_json::{json, Value};
use std::env;

use crate::api::{read_json, read_bytes, send_err};
use crate::helper::truncate_chars;
use crate::profiles::PublishProfile;

/// YouTube rejects titles over 100 characters or containing angle brackets.
fn youtube_title(title: &str) -> String {
    let clean: String = title.chars().filter(|c| *c != '<' && *c != '>').collect();
    truncate_chars(clean.trim(), 100)
}

async fn read_video(video_path: &str) -> Result<Vec<u8>, String> {
    let bytes = tokio::fs::read(video_path)
        .await
        .map_err(|e| format!("read {}: {}", video_path, e))?;
    if bytes.is_empty() {
        return Err(format!("{} is empty", video_path));
    }
    Ok(bytes)
}

pub async fn upload_to_youtube(client: &Client, video_path: &str, title: &str, publish: &PublishProfile) -> Result<(), String> {
    let client_id = env::var("YOUTUBE_CLIENT_ID").map_err(|_| "Missing YOUTUBE_CLIENT_ID")?;
    let client_secret = env::var("YOUTUBE_CLIENT_SECRET").map_err(|_| "Missing YOUTUBE_CLIENT_SECRET")?;
    let refresh_token = env::var("YOUTUBE_REFRESH_TOKEN").map_err(|_| "Missing YOUTUBE_REFRESH_TOKEN")?;

    // 1. Get access token
    let token_res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| send_err("YouTube OAuth", e))?;

    let token_json = read_json(token_res, "YouTube OAuth").await?;
    let access_token = token_json["access_token"]
        .as_str()
        .ok_or("YouTube OAuth: missing access_token")?;
    
    // 2. Read video
    let video_bytes = read_video(video_path).await?;

    // 3. Metadata
    let metadata = json!({
        "snippet": {
            "title": youtube_title(title),
            "description": publish.description,
            "tags": publish.tags,
            "categoryId": publish.youtube_category,
        },
        "status": {
            "privacyStatus": "public",
            "madeForKids": publish.made_for_kids,
            "selfDeclaredMadeForKids": publish.made_for_kids
        }
    });

    // 4. Build RAW multipart/related body manually
    let boundary = format!("video_studio_{}", uuid::Uuid::new_v4().simple());
    let body = format!(
        "--{b}\r\n\
Content-Type: application/json; charset=UTF-8\r\n\r\n\
{}\r\n\
--{b}\r\n\
Content-Type: video/mp4\r\n\r\n",
        metadata,
        b = boundary
    );

    let mut full_body = body.into_bytes();
    full_body.extend(video_bytes);
    full_body.extend(format!("\r\n--{boundary}--\r\n").into_bytes());

    // 5. Upload
    let res = client
        .post("https://www.googleapis.com/upload/youtube/v3/videos?uploadType=multipart&part=snippet,status")
        .bearer_auth(access_token)
        .header("Content-Type", format!("multipart/related; boundary={}", boundary))
        .body(full_body)
        .send()
        .await
        .map_err(|e| send_err("YouTube upload", e))?;

    let uploaded = read_json(res, "YouTube upload").await?;
    println!("🟢 YouTube Upload OK! id: {}", uploaded["id"].as_str().unwrap_or("?"));
    Ok(())
}

pub async fn upload_to_tiktok(client: &Client, video_path: &str, title: &str) -> Result<(), String> {
    let access_token = env::var("TIKTOK_ACCESS_TOKEN").map_err(|_| "Missing TIKTOK_ACCESS_TOKEN")?;
    let video_bytes = read_video(video_path).await?;
    let size = video_bytes.len();

    let init_payload = json!({
        "post_info": { "title": truncate_chars(title, 2200), "privacy_level": "PUBLIC_TO_EVERYONE", "disable_comment": true },
        "source_info": { "source": "FILE_UPLOAD", "video_size": size, "chunk_size": size, "total_chunk_count": 1 }
    });

    let init_res = client.post("https://open.tiktokapis.com/v2/post/publish/video/init/")
        .bearer_auth(&access_token)
        .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
        .json(&init_payload)
        .send()
        .await
        .map_err(|e| send_err("TikTok init", e))?;

    let init_json: Value = read_json(init_res, "TikTok init").await?;
    if init_json["error"]["code"].as_str() != Some("ok") {
        return Err(format!("TikTok init failed: {}", init_json["error"]));
    }

    let upload_url = init_json["data"]["upload_url"]
        .as_str()
        .ok_or_else(|| format!("TikTok init: missing upload_url: {}", init_json))?;

    let put_res = client.put(upload_url)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CONTENT_LENGTH, size.to_string())
        .header("Content-Range", format!("bytes 0-{}/{}", size - 1, size))
        .body(video_bytes)
        .send()
        .await
        .map_err(|e| send_err("TikTok upload", e))?;

    read_bytes(put_res, "TikTok upload").await?;
    println!("🟢 TikTok Upload OK!");
    Ok(())
}
