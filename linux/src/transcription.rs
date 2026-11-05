use serde::Deserialize;
use std::time::Duration;

const PATH: &str = "/audio/transcriptions";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const BOUNDARY: &str = "sthdictate-boundary-7f3a9c";

#[derive(Deserialize)]
struct Answer {
    text: String,
}

pub fn transcription_url(endpoint: &str) -> Result<String, String> {
    let base = endpoint.trim().trim_end_matches('/');
    let full = if base.ends_with(PATH) { base.to_string() } else { format!("{base}{PATH}") };
    let host = full.strip_prefix("http://").or_else(|| full.strip_prefix("https://")).unwrap_or("");
    if host.is_empty() || host.starts_with('/') {
        return Err("Endpoint must be an http(s) URL, like http://host:8000/v1".into());
    }
    Ok(full)
}

pub fn multipart(audio: &[u8], model: Option<&str>) -> (String, Vec<u8>) {
    let field = |name: &str, value: &str| {
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").into_bytes()
    };
    let file_head = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    );
    let parts = [
        model.map(|name| field("model", name)).unwrap_or_default(),
        field("response_format", "json"),
        file_head.into_bytes(),
        audio.to_vec(),
        b"\r\n".to_vec(),
        format!("--{BOUNDARY}--\r\n").into_bytes(),
    ];
    (format!("multipart/form-data; boundary={BOUNDARY}"), parts.concat())
}

pub fn one_paragraph(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn transcript_text(status: u16, body: &str) -> Result<String, String> {
    if !(200..300).contains(&status) {
        return Err(format!("Server answered {status}: {}", body.chars().take(300).collect::<String>()));
    }
    serde_json::from_str::<Answer>(body)
        .map(|answer| one_paragraph(&answer.text))
        .map_err(|_| "Server answered without a JSON \"text\" field".to_string())
}

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, audio: &[u8]) -> Result<String, String>;
}

pub struct Whisper {
    pub endpoint: String,
    pub model: Option<String>,
}

impl Transcriber for Whisper {
    fn transcribe(&self, audio: &[u8]) -> Result<String, String> {
        let url = transcription_url(&self.endpoint)?;
        let (content_type, body) = multipart(audio, self.model.as_deref());
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .post(&url)
            .header("Content-Type", &content_type)
            .send(&body[..])
            .map_err(|error| format!("Could not reach {url}: {error}"))?;
        let status = response.status().as_u16();
        let text =
            response.body_mut().read_to_string().map_err(|error| format!("{url} dropped the answer: {error}"))?;
        transcript_text(status, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_gets_the_transcription_path_once() {
        assert_eq!(transcription_url("http://h:8000/v1/").unwrap(), "http://h:8000/v1/audio/transcriptions");
        assert_eq!(
            transcription_url(" http://h:8000/v1/audio/transcriptions ").unwrap(),
            "http://h:8000/v1/audio/transcriptions"
        );
    }

    #[test]
    fn garbage_endpoint_is_rejected() {
        assert!(transcription_url("not a url").is_err());
        assert!(transcription_url("http:///v1").is_err());
    }

    #[test]
    fn multipart_carries_audio_and_optional_model() {
        let (content_type, body) = multipart(b"AUDIO", Some("turbo"));
        let text = String::from_utf8_lossy(&body);
        assert!(content_type.ends_with(BOUNDARY));
        assert!(text.contains("name=\"model\"\r\n\r\nturbo"));
        assert!(text.contains("filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nAUDIO\r\n"));
        assert!(text.ends_with(&format!("--{BOUNDARY}--\r\n")));
        assert!(!String::from_utf8_lossy(&multipart(b"A", None).1).contains("name=\"model\""));
    }

    #[test]
    fn transcript_needs_success_and_a_text_field() {
        assert_eq!(transcript_text(200, r#"{"text":"hi"}"#).unwrap(), "hi");
        assert!(transcript_text(500, r#"{"text":"hi"}"#).unwrap_err().starts_with("Server answered 500"));
        assert!(transcript_text(200, "{}").is_err());
    }

    #[test]
    fn whisper_segments_become_one_paragraph() {
        let body = r#"{"text":" Hello, my name is Shane.\n I do not want\n to shout.\n"}"#;
        assert_eq!(transcript_text(200, body).unwrap(), "Hello, my name is Shane. I do not want to shout.");
    }
}
