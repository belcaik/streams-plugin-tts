use std::{
    error::Error,
    fmt,
    io::{self, BufRead, Write},
    path::PathBuf,
};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::{Config, ConfigError},
    model::{Init, InputEvent, OutputEvent},
};

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    Config(ConfigError),
    InvalidInit(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "error de E/S: {error}"),
            Self::Json(error) => write!(formatter, "JSON inválido: {error}"),
            Self::Config(error) => error.fmt(formatter),
            Self::InvalidInit(message) => formatter.write_str(message),
        }
    }
}

impl Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<ConfigError> for ProtocolError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInit {
    #[serde(rename = "type")]
    kind: String,
    config: Value,
    data_dir: PathBuf,
}

pub fn read_first_init(reader: &mut impl BufRead) -> Result<Init, ProtocolError> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(ProtocolError::InvalidInit("falta la línea init".into()));
    }
    let raw: RawInit = serde_json::from_str(&line)?;
    if raw.kind != "init" || !raw.config.is_object() || raw.data_dir.as_os_str().is_empty() {
        return Err(ProtocolError::InvalidInit(
            "init, config objeto y data_dir son obligatorios".into(),
        ));
    }
    let mut config: Config = serde_json::from_value(raw.config)?;
    config.normalize()?;
    Ok(Init {
        config,
        data_dir: raw.data_dir,
    })
}

pub fn parse_input_line(line: &str) -> Result<InputEvent, ProtocolError> {
    Ok(serde_json::from_str(line)?)
}

pub fn write_output(writer: &mut impl Write, output: &OutputEvent) -> Result<(), ProtocolError> {
    serde_json::to_writer(&mut *writer, output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::model::{AudioData, OutputData};

    #[test]
    fn reads_normalized_init_and_rejects_eof_or_wrong_type() {
        let mut input = Cursor::new(
            b"{\"type\":\"init\",\"config\":{\"volume\":0.5},\"data_dir\":\"/tmp/tts\"}\n",
        );
        let init = read_first_init(&mut input).expect("valid init");
        assert_eq!(init.config.volume, 0.5);
        assert!(read_first_init(&mut Cursor::new(b"")).is_err());
        let wrong = b"{\"type\":\"chat\",\"config\":{},\"data_dir\":\".\"}\n";
        assert!(read_first_init(&mut Cursor::new(wrong)).is_err());
    }

    #[test]
    fn reports_broken_events_and_writes_one_json_line() {
        assert!(parse_input_line("not-json").is_err());
        let output = OutputEvent {
            kind: "tts_audio".into(),
            data: OutputData::Audio(AudioData {
                id: "tts-1".into(),
                mime: "audio/wav".into(),
                wav_base64: "UklGR".into(),
                text: "hola".into(),
                user_name: "Ana".into(),
            }),
        };
        let mut bytes = Vec::new();
        write_output(&mut bytes, &output).expect("write output");
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes[..bytes.len() - 1]).expect("JSON"),
            serde_json::to_value(output).expect("value")
        );
    }
}
