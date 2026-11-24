//! Small shared helpers: atomic file writes, media sniffing, text limits.

/// Write a file atomically: write to `<path>.part`, then rename. A crash mid-write
/// never leaves a truncated file that later runs would skip as "already exists".
pub async fn write_atomic(path: &str, bytes: &[u8]) -> Result<(), String> {
    let tmp = part_path(path);
    tokio::fs::write(&tmp, bytes).await.map_err(|e| format!("write {}: {}", tmp, e))?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| format!("rename {}: {}", path, e))
}

/// Temporary path used while `path` is being produced.
pub fn part_path(path: &str) -> String {
    format!("{}.part", path)
}

/// Move a finished `<path>.part` into place.
pub async fn commit_part(path: &str) -> Result<(), String> {
    tokio::fs::rename(part_path(path), path)
        .await
        .map_err(|e| format!("rename {}: {}", path, e))
}

/// True for PNG, JPEG or WebP data. Providers sometimes answer 200 with an
/// HTML or JSON error body; this keeps those from being saved as images.
pub fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

/// Short, printable preview of a response body for error messages.
pub fn preview(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_string()
}

/// Cut `s` to at most `max` characters (not bytes), so multi-byte text never splits.
pub fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_rejects_short_and_error_bodies() {
        assert!(!is_image(&[]));
        assert!(!is_image(b"RIFF"));
        assert!(!is_image(br#"{"errors":[{"code":10000}]}"#));
        assert!(is_image(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A]));
        assert!(is_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
    }

    #[test]
    fn truncate_chars_respects_multibyte() {
        assert_eq!(truncate_chars("Diễm Sương", 4), "Diễm");
    }
}
