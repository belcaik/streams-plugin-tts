use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InputEvent {
    pub id: String,
    pub platform: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub user: User,
    pub data: Value,
    pub ts: String,
}

impl InputEvent {
    pub fn string(&self, key: &str) -> Option<&str> {
        self.data.get(key)?.as_str()
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.data
            .get(key)?
            .as_f64()
            .filter(|value| value.is_finite())
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.user
            .roles
            .iter()
            .any(|item| item.eq_ignore_ascii_case(role))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Init {
    pub config: Config,
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DoneData {
    pub id: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRequest {
    pub platform: String,
    pub user: User,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub platform: String,
    pub user: User,
    pub text: String,
}

impl Request {
    pub fn new(id: String, raw: RawRequest) -> Self {
        Self {
            id,
            platform: raw.platform,
            user: raw.user,
            text: raw.text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminCommand {
    Skip,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusKind {
    Queued,
    Playing,
    Skipped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusData {
    pub id: String,
    pub status: StatusKind,
    pub queue_len: usize,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioData {
    pub id: String,
    pub mime: String,
    pub wav_base64: String,
    pub text: String,
    pub user_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub data: OutputData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum OutputData {
    Status(StatusData),
    Audio(AudioData),
}

impl OutputEvent {
    pub fn status(data: StatusData) -> Self {
        Self {
            kind: "tts_status".into(),
            data: OutputData::Status(data),
        }
    }

    pub fn audio(data: AudioData) -> Self {
        Self {
            kind: "tts_audio".into(),
            data: OutputData::Audio(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_status_and_audio_exactly() {
        let status = OutputEvent::status(StatusData {
            id: "tts-7".into(),
            status: StatusKind::Skipped,
            queue_len: 0,
            reason: Some("moderator_skip".into()),
            message: None,
        });
        assert_eq!(
            serde_json::to_string(&status).expect("status JSON"),
            concat!(
                r#"{"type":"tts_status","data":{"id":"tts-7","status":"skipped","#,
                r#""queue_len":0,"reason":"moderator_skip","message":null}}"#
            )
        );
        let audio = OutputEvent::audio(AudioData {
            id: "tts-7".into(),
            mime: "audio/wav".into(),
            wav_base64: "UklGR".into(),
            text: "hola".into(),
            user_name: "Ana".into(),
        });
        assert_eq!(
            serde_json::to_string(&audio).expect("audio JSON"),
            concat!(
                r#"{"type":"tts_audio","data":{"id":"tts-7","mime":"audio/wav","#,
                r#""wav_base64":"UklGR","text":"hola","user_name":"Ana"}}"#
            )
        );
    }
}
