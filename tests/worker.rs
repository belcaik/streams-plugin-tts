use std::{
    cell::{Cell, RefCell},
    io::{self, Cursor, Write},
    rc::Rc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use streams_plugin_tts::{
    coordinator::{Clock, Message},
    engine::{Engine, EngineError, SynthesisRequest},
    protocol::{parse_input_line, read_first_init},
    runtime::{Driver, RuntimeError, run},
};

const WAV: &[u8] = b"RIFF\x04\0\0\0WAVE";

#[derive(Clone, Default)]
struct TestClock(Rc<Cell<Duration>>);

impl Clock for TestClock {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

#[derive(Clone, Default)]
struct SharedLog(Rc<RefCell<Vec<u8>>>);

impl SharedLog {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
}

impl Write for SharedLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Motor en memoria: registra textos y responde con un resultado fijo.
#[derive(Default)]
struct MemoryEngine {
    texts: Vec<String>,
    failure: Option<EngineError>,
    panic: bool,
}

impl Engine for MemoryEngine {
    fn synthesize(&mut self, request: SynthesisRequest) -> Result<Vec<u8>, EngineError> {
        assert!(!self.panic, "fallo simulado del motor");
        self.texts.push(request.text);
        self.failure.clone().map_or(Ok(WAV.to_vec()), Err)
    }
}

struct Harness {
    driver: Driver<TestClock, Vec<u8>, SharedLog>,
    clock: TestClock,
    log: SharedLog,
    engine: MemoryEngine,
    seen: usize,
}

impl Harness {
    fn new(config: Value) -> Self {
        let init = json!({"type":"init", "config":config, "data_dir":"."}).to_string();
        let init = read_first_init(&mut Cursor::new(init)).expect("init");
        let (clock, log) = (TestClock::default(), SharedLog::default());
        Self {
            driver: Driver::new(init, clock.clone(), Vec::new(), log.clone()),
            clock,
            log,
            engine: MemoryEngine::default(),
            seen: 0,
        }
    }

    fn send(&mut self, message: Message) -> Vec<String> {
        let exit = self
            .driver
            .dispatch_inline(&mut self.engine, message)
            .expect("dispatch");
        assert!(!exit, "unexpected exit");
        self.lines().iter().map(summary).collect()
    }

    fn input(&mut self, line: &str) -> Vec<String> {
        let event = parse_input_line(line).expect("valid event");
        self.send(Message::Input(event))
    }

    fn lines(&mut self) -> Vec<Value> {
        let output = String::from_utf8(self.driver.output().clone()).expect("UTF-8");
        let lines: Vec<Value> = output
            .lines()
            .skip(self.seen)
            .map(|line| serde_json::from_str(line).expect("JSON line"))
            .collect();
        self.seen += lines.len();
        lines
    }
}

fn summary(line: &Value) -> String {
    let data = &line["data"];
    match line["type"].as_str() {
        Some("tts_audio") => format!("audio:{}", data["id"].as_str().unwrap_or_default()),
        _ => format!(
            "{}:{}:{}:{}",
            data["status"].as_str().unwrap_or_default(),
            data["id"].as_str().unwrap_or_default(),
            data["queue_len"],
            data["reason"].as_str().unwrap_or("-")
        ),
    }
}

fn event(platform: &str, kind: &str, user: &str, roles: &[&str], data: Value) -> String {
    json!({
        "id": format!("event-{user}"), "platform": platform, "type": kind,
        "user": {"id": user, "name": format!("User {user}"), "roles": roles},
        "data": data, "ts": "2026-11-04T00:00:00Z"
    })
    .to_string()
}

fn twitch(user: &str, text: &str) -> String {
    let data = json!({"reward_id":"r", "reward_title":"TTS", "cost":1, "input":text});
    event("twitch", "redemption", user, &[], data)
}

fn youtube(user: &str, text: &str) -> String {
    let data = json!({"amount":5.0, "currency":"USD", "message":text});
    event("youtube", "donation", user, &[], data)
}

fn kick(user: &str, roles: &[&str], message: &str) -> String {
    event("kick", "chat", user, roles, json!({"message":message}))
}

fn done(id: &str, outcome: &str) -> String {
    let data = json!({"id":id, "outcome":outcome});
    event("plugin:overlay", "tts_done", "overlay", &[], data)
}

#[test]
fn three_platforms_share_one_fifo_and_advance_on_done() {
    let mut harness = Harness::new(json!({}));
    assert_eq!(
        harness.input(&twitch("1", "hola twitch")),
        ["queued:tts-1:1:-", "playing:tts-1:1:-", "audio:tts-1"]
    );
    assert_eq!(
        harness.input(&youtube("2", "hola youtube")),
        ["queued:tts-2:2:-"]
    );
    assert_eq!(
        harness.input(&kick("3", &["subscriber"], "!tts hola kick")),
        ["queued:tts-3:3:-"]
    );
    assert!(harness.input(&done("tts-2", "ended")).is_empty());
    assert_eq!(
        harness.input(&done("tts-1", "ended")),
        ["playing:tts-2:2:-", "audio:tts-2"]
    );
    assert_eq!(
        harness.input(&done("tts-2", "ended")),
        ["playing:tts-3:1:-", "audio:tts-3"]
    );
    assert_eq!(
        harness.engine.texts,
        ["hola twitch", "hola youtube", "hola kick"]
    );
}

#[test]
fn audio_payload_carries_base64_wav_text_and_user() {
    let mut harness = Harness::new(json!({}));
    harness.send(Message::Input(
        parse_input_line(&twitch("1", "hola")).expect("event"),
    ));
    let output = String::from_utf8(harness.driver.output().clone()).expect("UTF-8");
    let audio: Value = serde_json::from_str(output.lines().last().expect("audio")).expect("JSON");
    let data = &audio["data"];
    assert_eq!(data["mime"], "audio/wav");
    assert_eq!(data["text"], "hola");
    assert_eq!(data["user_name"], "User 1");
    let wav = STANDARD
        .decode(data["wav_base64"].as_str().expect("base64"))
        .expect("valid base64");
    assert_eq!(wav, WAV);
}

#[test]
fn unavailable_engine_reports_once_and_keeps_the_worker_alive() {
    let mut harness = Harness::new(json!({"cooldown_seconds":0}));
    harness.engine.failure = Some(EngineError::Unavailable("no se encontró piper".into()));
    assert_eq!(
        harness.input(&twitch("1", "a")),
        ["queued:tts-1:1:-", "error:tts-1:0:engine_unavailable"]
    );
    assert_eq!(
        harness.input(&youtube("2", "b")),
        ["error:tts-2:0:engine_unavailable"]
    );
    assert_eq!(harness.engine.texts, ["a"]);
    assert_eq!(
        harness.log.text().matches("no se encontró piper").count(),
        1
    );
}

#[test]
fn engine_panic_becomes_a_recoverable_synthesis_error() {
    let mut harness = Harness::new(json!({}));
    harness.engine.panic = true;
    assert_eq!(
        harness.input(&twitch("1", "a")),
        ["queued:tts-1:1:-", "error:tts-1:0:synthesis_failed"]
    );
    assert!(harness.log.text().contains("pánico"));
    harness.engine.panic = false;
    assert_eq!(
        harness.input(&youtube("2", "b")),
        ["queued:tts-2:1:-", "playing:tts-2:1:-", "audio:tts-2"]
    );
}

#[test]
fn playback_timeout_uses_the_injected_clock() {
    let mut harness = Harness::new(json!({"playback_timeout_seconds":5, "cooldown_seconds":0}));
    harness.input(&twitch("1", "a"));
    harness.input(&kick("2", &["subscriber"], "!tts b"));
    assert_eq!(harness.driver.next_deadline(), Some(Duration::from_secs(5)));
    harness.clock.0.set(Duration::from_secs(4));
    assert!(harness.send(Message::Tick(harness.driver.now())).is_empty());
    harness.clock.0.set(Duration::from_secs(5));
    assert_eq!(
        harness.send(Message::Tick(harness.driver.now())),
        [
            "skipped:tts-1:1:playback_timeout",
            "playing:tts-2:1:-",
            "audio:tts-2"
        ]
    );
    assert_eq!(
        harness.driver.next_deadline(),
        Some(Duration::from_secs(10))
    );
}

#[test]
fn admin_commands_work_across_platforms_and_need_privileges() {
    let mut harness = Harness::new(json!({"cooldown_seconds":0}));
    harness.input(&twitch("1", "a"));
    harness.input(&youtube("2", "b"));
    harness.input(&twitch("3", "c"));
    assert!(
        harness
            .input(&kick("4", &["subscriber"], "!skip"))
            .is_empty()
    );
    assert_eq!(
        harness.input(&kick("5", &["moderator"], "!SKIP")),
        [
            "skipped:tts-1:2:moderator_skip",
            "playing:tts-2:2:-",
            "audio:tts-2"
        ]
    );
    let clear = event(
        "youtube",
        "chat",
        "6",
        &["broadcaster"],
        json!({"message":"!ttsclear"}),
    );
    assert_eq!(
        harness.input(&clear),
        [
            "skipped:tts-2:0:moderator_clear",
            "skipped:tts-3:0:moderator_clear"
        ]
    );
    assert!(harness.input(&done("tts-2", "ended")).is_empty());
}

#[test]
fn eof_requests_exit_and_invalid_lines_are_rejected_by_the_protocol() {
    let mut harness = Harness::new(json!({}));
    let missing_outcome = event(
        "plugin:overlay",
        "tts_done",
        "overlay",
        &[],
        json!({"id":"tts-1"}),
    );
    for line in ["not-json", "{}", &missing_outcome] {
        assert!(parse_input_line(line).is_err(), "{line}");
    }
    assert!(harness.input(&done("tts-9", "unknown")).is_empty());
    let exit = harness
        .driver
        .dispatch_inline(&mut harness.engine, Message::Eof)
        .expect("eof");
    assert!(exit);
}

#[test]
fn run_discards_invalid_json_and_exits_cleanly_on_eof() {
    let input = concat!(
        r#"{"type":"init","config":{},"data_dir":"."}"#,
        "\nnot-json\n{\"type\":\"chat\"}\n"
    );
    let log = SharedLog::default();
    let mut output = Vec::new();
    run(
        Cursor::new(input),
        &mut output,
        log.clone(),
        |_| MemoryEngine::default(),
        TestClock::default(),
    )
    .expect("clean exit");
    assert!(output.is_empty());
    assert_eq!(log.text().matches("entrada descartada").count(), 2);
    let invalid = run(
        Cursor::new("{}\n"),
        Vec::new(),
        Vec::new(),
        |_| MemoryEngine::default(),
        TestClock::default(),
    );
    assert!(matches!(invalid, Err(RuntimeError::Protocol(_))));
}
