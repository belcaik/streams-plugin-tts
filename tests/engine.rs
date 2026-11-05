use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use streams_plugin_tts::{
    config::{EngineConfig, EngineKind},
    engine::{Engine, EngineError, ProcessEngine, SynthesisRequest},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "streams-tts-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(directory: &Path, text: &str) -> SynthesisRequest {
    SynthesisRequest {
        id: "tts-1".into(),
        text: text.into(),
        data_dir: directory.to_owned(),
        max_audio_bytes: 64 * 1024,
        deadline: Instant::now() + Duration::from_secs(5),
    }
}

fn engine(kind: EngineKind, extra: &str) -> ProcessEngine {
    let mut config = EngineConfig {
        kind,
        ..EngineConfig::default()
    };
    config.piper.binary = env!("CARGO_BIN_EXE_fake-tts-engine").into();
    config.espeak.binary = env!("CARGO_BIN_EXE_fake-tts-engine").into();
    match kind {
        EngineKind::Piper => config.piper.extra_args = vec![extra.into()],
        EngineKind::Espeak => config.espeak.extra_args = vec![extra.into()],
    }
    ProcessEngine::from_config(&config)
}

fn assert_clean(directory: &Path) {
    let entries = fs::read_dir(directory.join("tmp"))
        .expect("tmp dir")
        .count();
    assert_eq!(entries, 0);
}

#[test]
fn both_engines_consume_stdin_and_clean_valid_wav() {
    for kind in [EngineKind::Piper, EngineKind::Espeak] {
        let directory = TempDir::new();
        let wav = engine(kind, "--valid")
            .synthesize(request(&directory.0, "hola"))
            .expect("synthesis");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_clean(&directory.0);
    }
}

#[test]
fn nonzero_invalid_and_oversize_outputs_are_rejected_and_cleaned() {
    for (flag, expected) in [
        ("--fake-fail", "synthesis_failed"),
        ("--fake-invalid", "invalid_wav"),
        ("--fake-oversize", "audio_too_large"),
    ] {
        let directory = TempDir::new();
        let error = engine(EngineKind::Piper, flag)
            .synthesize(request(&directory.0, "hola"))
            .expect_err("failure expected");
        assert_eq!(error.reason(), expected);
        assert_clean(&directory.0);
    }
}

#[test]
fn text_in_arguments_fails_and_missing_binary_is_unavailable() {
    let directory = TempDir::new();
    let error = engine(EngineKind::Espeak, "hola")
        .synthesize(request(&directory.0, "hola"))
        .expect_err("text argument must fail");
    assert_eq!(error.reason(), "synthesis_failed");
    assert_clean(&directory.0);
    let mut config = EngineConfig::default();
    config.piper.binary = "/definitely/missing/tts-engine".into();
    let error = ProcessEngine::from_config(&config)
        .synthesize(request(&directory.0, "hola"))
        .expect_err("missing binary");
    assert!(matches!(error, EngineError::Unavailable(_)));
    assert_clean(&directory.0);
}
