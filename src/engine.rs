use std::{
    error::Error,
    fmt,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use crate::config::{EngineConfig, EngineKind};

pub trait Engine: Send + 'static {
    fn synthesize(&mut self, request: SynthesisRequest) -> Result<Vec<u8>, EngineError>;
}

#[derive(Debug)]
pub struct SynthesisRequest {
    pub id: String,
    pub text: String,
    pub data_dir: PathBuf,
    pub max_audio_bytes: usize,
    pub deadline: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    Unavailable(String),
    Timeout,
    Failed(String),
    InvalidWav,
    TooLarge,
    Io(String),
}

impl EngineError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "engine_unavailable",
            Self::Timeout => "synthesis_timeout",
            Self::Failed(_) | Self::Io(_) => "synthesis_failed",
            Self::InvalidWav => "invalid_wav",
            Self::TooLarge => "audio_too_large",
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => formatter.write_str(message),
            Self::Timeout => formatter.write_str("la síntesis superó su deadline"),
            Self::Failed(message) => write!(formatter, "el motor falló: {message}"),
            Self::InvalidWav => formatter.write_str("el motor no produjo un WAV válido"),
            Self::TooLarge => formatter.write_str("el WAV supera max_audio_bytes"),
            Self::Io(message) => write!(formatter, "error de E/S del motor: {message}"),
        }
    }
}

impl Error for EngineError {}

#[derive(Debug)]
pub struct ProcessEngine {
    kind: EngineKind,
    binary: String,
    extra_args: Vec<String>,
    model: String,
    voice: String,
    base_wpm: u32,
    speed: f64,
}

impl ProcessEngine {
    pub fn from_config(config: &EngineConfig) -> Self {
        Self {
            kind: config.kind,
            binary: match config.kind {
                EngineKind::Piper => config.piper.binary.clone(),
                EngineKind::Espeak => config.espeak.binary.clone(),
            },
            extra_args: match config.kind {
                EngineKind::Piper => config.piper.extra_args.clone(),
                EngineKind::Espeak => config.espeak.extra_args.clone(),
            },
            model: config.piper.model.clone(),
            voice: config.espeak.voice.clone(),
            base_wpm: config.espeak.base_wpm,
            speed: config.speed,
        }
    }

    fn command(&self, output: &Path) -> Command {
        let mut command = Command::new(&self.binary);
        command.args(&self.extra_args);
        match self.kind {
            EngineKind::Piper => {
                command
                    .arg("--model")
                    .arg(&self.model)
                    .arg("--length_scale")
                    .arg((1.0 / self.speed).to_string())
                    .arg("--output_file")
                    .arg(output);
            }
            EngineKind::Espeak => {
                command
                    .arg("-v")
                    .arg(&self.voice)
                    .arg("-s")
                    .arg(((self.base_wpm as f64) * self.speed).round().to_string())
                    .arg("-w")
                    .arg(output);
            }
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, request: &SynthesisRequest, output: &Path) -> Result<(), EngineError> {
        let mut child = self.command(output).spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                EngineError::Unavailable(format!(
                    "no se encontró el ejecutable {} para el motor {:?}",
                    self.binary, self.kind
                ))
            } else {
                EngineError::Io(error.to_string())
            }
        })?;
        let write_result = match child.stdin.take() {
            Some(mut stdin) => stdin.write_all(request.text.as_bytes()),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stdin ausente",
            )),
        };
        if let Err(error) = write_result {
            stop(&mut child);
            return Err(EngineError::Io(error.to_string()));
        }
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err(EngineError::Failed(read_stderr(&mut child))),
                Ok(None) if Instant::now() < request.deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    stop(&mut child);
                    return Err(EngineError::Timeout);
                }
                Err(error) => {
                    stop(&mut child);
                    return Err(EngineError::Io(error.to_string()));
                }
            }
        }
    }
}

impl Engine for ProcessEngine {
    fn synthesize(&mut self, request: SynthesisRequest) -> Result<Vec<u8>, EngineError> {
        let temporary = TempWav::new(&request.data_dir)?;
        self.run(&request, &temporary.path)?;
        let mut bytes = Vec::new();
        File::open(&temporary.path)
            .and_then(|file| {
                file.take(request.max_audio_bytes as u64 + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|error| EngineError::Io(error.to_string()))?;
        if bytes.len() > request.max_audio_bytes {
            return Err(EngineError::TooLarge);
        }
        if !valid_wav(&bytes) {
            return Err(EngineError::InvalidWav);
        }
        Ok(bytes)
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_stderr(child: &mut Child) -> String {
    let mut bytes = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(8192).read_to_end(&mut bytes);
    }
    let message = String::from_utf8_lossy(&bytes);
    let message = message.trim();
    if message.is_empty() {
        "exit no-cero".into()
    } else {
        message.to_owned()
    }
}

struct TempWav {
    path: PathBuf,
}

impl TempWav {
    fn new(data_dir: &Path) -> Result<Self, EngineError> {
        let directory = data_dir.join("tmp");
        std::fs::create_dir_all(&directory).map_err(|error| EngineError::Io(error.to_string()))?;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        loop {
            let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!("tts-{}-{sequence}.wav", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(EngineError::Io(error.to_string())),
            }
        }
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn valid_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piper_and_espeak_put_controlled_arguments_after_extras() {
        let mut config = EngineConfig::default();
        config.piper.extra_args = vec!["--quality".into(), "high".into()];
        let piper = ProcessEngine::from_config(&config);
        let command = piper.command(Path::new("out.wav"));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                "--quality",
                "high",
                "--model",
                "models/voice.onnx",
                "--length_scale",
                "1",
                "--output_file",
                "out.wav"
            ]
        );
        config.kind = EngineKind::Espeak;
        config.speed = 2.0;
        let espeak = ProcessEngine::from_config(&config);
        let command = espeak.command(Path::new("out.wav"));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-v", "es", "-s", "350", "-w", "out.wav"]);
    }

    #[test]
    fn wav_magic_and_error_reasons_are_stable() {
        assert!(valid_wav(b"RIFF0000WAVEdata"));
        assert!(!valid_wav(b"not a wav"));
        assert_eq!(EngineError::Timeout.reason(), "synthesis_timeout");
        assert_eq!(EngineError::TooLarge.reason(), "audio_too_large");
    }
}
