use async_trait::async_trait;

use crate::provider::*;

pub struct ElevenLabsClient;

#[async_trait]
impl ProviderClient for ElevenLabsClient {
    async fn call(
        &self,
        req: &ProviderRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {
        match req {
            ProviderRequest::Audio(req) => {
                self.audio(req, credential).await
            },

            _ => Err(ProviderError::NotSupported(format!("{}", Self.provider().clone()))),
        }
    }
    
    fn is_quota_error(
        &self,
        status: u16,
        body: &str,
    ) -> bool {
        status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || body.contains("quota_exceeded")
            || body.contains("exceeds your quota")
            || body.contains("rate limit")
    }
    
    fn is_auth_error(
        &self,
        status: u16,
        body: &str,
    ) -> bool {
        status == reqwest::StatusCode::UNAUTHORIZED
    }

    
    fn provider(&self) -> Provider {
        Provider::ElevenLabs
    }

    fn config(&self) -> ProviderConfig {
        ProviderConfig {
            reset_daily: false,
            reset_monthly: true,
            reset_time: None,
            daily_limit: None,
            retry_after: 5,
        }
    }
}

impl ElevenLabsClient {
    async fn audio(
        &self,
        req: &AudioRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {
        let voice_id = req.voice_id.as_deref().filter(|v| !v.trim().is_empty()).ok_or_else(|| {
            ProviderError::ClientResponse {
                client: "ElevenLabs".to_string(),
                error: "No voice_id given".to_string(),
            }
        })?;

        let payload = serde_json::json!({
            "text": req.text,
            "model_id": "eleven_multilingual_v2",
            "voice_settings": {
                "speed": req.speed,
                "stability": req.stability,
                "similarity_boost": req.similarity_boost,
            },
        });

        // WAV is requested as raw 16-bit mono PCM and wrapped locally.
        let (output_format, accept, pcm_rate) = match req.format {
            Some(AudioFormat::Wav { sample_rate }) => (format!("pcm_{}", sample_rate), "audio/pcm", Some(sample_rate)),
            _ => ("mp3_44100_128".to_string(), "audio/mpeg", None),
        };

        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}?output_format={}",
            voice_id, output_format
        );

        let res = reqwest::Client::new()
            .post(url)
            .header("xi-api-key", &credential.api_key)
            .header("Accept", accept)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                ProviderError::ClientResponse {
                    client: "ElevenLabs".to_string(),
                    error: e.without_url().to_string(),
                }
            })?;

        let bytes = self.read_bytes(res).await?;

        match pcm_rate {
            Some(rate) => pcm16_mono_to_wav(&bytes, rate)
                .map(ProviderResponse::Bytes)
                .map_err(|e| ProviderError::ClientResponse { client: "ElevenLabs".to_string(), error: e }),
            None => Ok(ProviderResponse::Bytes(bytes)),
        }
    }
}

/// Wrap raw little-endian 16-bit mono PCM in a WAV container.
pub fn pcm16_mono_to_wav(pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>, String> {
    if pcm.is_empty() || pcm.len() % 2 != 0 {
        return Err(format!("Invalid PCM data ({} bytes)", pcm.len()));
    }

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::with_capacity(pcm.len() + 44));
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
        for chunk in pcm.chunks_exact(2) {
            writer
                .write_sample(i16::from_le_bytes([chunk[0], chunk[1]]))
                .map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_pcm_as_wav() {
        let pcm: Vec<u8> = [0i16, 1000, -1000, 32767].iter().flat_map(|s| s.to_le_bytes()).collect();
        let wav = pcm16_mono_to_wav(&pcm, 22050).unwrap();
        let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, 22050);
        assert_eq!(reader.spec().channels, 1);
        let samples: Vec<i16> = reader.into_samples().map(|s| s.unwrap()).collect();
        assert_eq!(samples, vec![0, 1000, -1000, 32767]);

        assert!(pcm16_mono_to_wav(&[1, 2, 3], 22050).is_err());
    }
}
