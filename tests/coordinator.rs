use std::{cell::Cell, rc::Rc, time::Duration};

use serde_json::{Value, json};
use streams_plugin_tts::{
    config::Config,
    coordinator::{Clock, Coordinator, Effect, Message},
    engine::EngineError,
    model::{InputEvent, OutputData, User},
};

#[derive(Clone)]
struct TestClock(Rc<Cell<Duration>>);

impl TestClock {
    fn new() -> Self {
        Self(Rc::new(Cell::new(Duration::ZERO)))
    }

    fn set(&self, now: Duration) {
        self.0.set(now);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

fn event(platform: &str, kind: &str, user: &str, roles: &[&str], data: Value) -> InputEvent {
    InputEvent {
        id: format!("event-{user}"),
        platform: platform.into(),
        kind: kind.into(),
        user: User {
            id: user.into(),
            name: format!("User {user}"),
            roles: roles.iter().map(|role| (*role).into()).collect(),
        },
        data,
        ts: String::new(),
    }
}

fn speak(platform: &str, user: &str, text: &str) -> InputEvent {
    match platform {
        "twitch" => event(
            platform,
            "redemption",
            user,
            &[],
            json!({
                "reward_title":"TTS", "input":text
            }),
        ),
        "youtube" => event(
            platform,
            "chat",
            user,
            &["subscriber"],
            json!({"message":text}),
        ),
        "kick" => event(
            platform,
            "chat",
            user,
            &["subscriber"],
            json!({"message":format!("!tts {text}")}),
        ),
        _ => panic!("unknown platform"),
    }
}

fn done(id: &str, outcome: &str) -> InputEvent {
    event(
        "plugin:overlay",
        "tts_done",
        "overlay",
        &[],
        json!({"id":id, "outcome":outcome}),
    )
}

fn admin(platform: &str, command: &str) -> InputEvent {
    event(
        platform,
        "chat",
        "mod",
        &["moderator"],
        json!({"message":command}),
    )
}

fn trace(effects: Vec<Effect>) -> Vec<String> {
    effects
        .into_iter()
        .map(|effect| match effect {
            Effect::Emit(output) => match output.data {
                OutputData::Status(status) => format!(
                    "{:?}:{}:{}:{}",
                    status.status,
                    status.id,
                    status.queue_len,
                    status.reason.as_deref().unwrap_or("-")
                ),
                OutputData::Audio(audio) => format!("audio:{}", audio.id),
            },
            Effect::Synthesize(request) => format!("synth:{}", request.id),
            Effect::Log(_) => "log".into(),
            Effect::Exit => "exit".into(),
        })
        .collect()
}

fn coordinator(config: Config) -> (Coordinator<TestClock>, TestClock) {
    let clock = TestClock::new();
    (Coordinator::new(config, ".".into(), clock.clone()), clock)
}

fn wav(id: &str) -> Message {
    Message::Synthesized {
        id: id.into(),
        result: Ok(b"RIFF0000WAVE".to_vec()),
    }
}

#[test]
fn fifo_is_global_and_capacity_includes_current() {
    let config = Config {
        max_queue: 2,
        cooldown_seconds: 0,
        ..Config::default()
    };
    let (mut coordinator, _) = coordinator(config);
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("twitch", "1", "a")))),
        ["Queued:tts-1:1:-", "synth:tts-1"]
    );
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("youtube", "2", "b")))),
        ["Queued:tts-2:2:-"]
    );
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("kick", "3", "c")))),
        ["Error:tts-3:2:queue_full"]
    );
    assert_eq!(
        trace(coordinator.handle(wav("tts-1"))),
        ["Playing:tts-1:2:-", "audio:tts-1"]
    );
    assert!(trace(coordinator.handle(Message::Input(done("old", "ended")))).is_empty());
    assert_eq!(
        trace(coordinator.handle(Message::Input(done("tts-1", "ended")))),
        ["synth:tts-2"]
    );
    assert_eq!(
        trace(coordinator.handle(wav("tts-2"))),
        ["Playing:tts-2:1:-", "audio:tts-2"]
    );
}

#[test]
fn cooldown_is_scoped_by_platform_and_user_and_expires() {
    let (mut coordinator, clock) = coordinator(Config {
        cooldown_seconds: 10,
        ..Config::default()
    });
    coordinator.handle(Message::Input(speak("twitch", "1", "a")));
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("twitch", "1", "b")))),
        ["Error:tts-2:1:cooldown"]
    );
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("kick", "1", "c")))),
        ["Queued:tts-3:2:-"]
    );
    clock.set(Duration::from_secs(10));
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("twitch", "1", "d")))),
        ["Queued:tts-4:3:-"]
    );
}

#[test]
fn done_requires_current_playing_id_and_timeout_is_exact() {
    let (mut coordinator, _) = coordinator(Config::default());
    coordinator.handle(Message::Input(speak("twitch", "1", "a")));
    assert!(trace(coordinator.handle(Message::Input(done("tts-1", "ended")))).is_empty());
    coordinator.handle(wav("tts-1"));
    assert!(trace(coordinator.handle(Message::Input(done("wrong", "ended")))).is_empty());
    assert!(trace(coordinator.handle(Message::Tick(Duration::from_secs(89)))).is_empty());
    assert_eq!(
        trace(coordinator.handle(Message::Tick(Duration::from_secs(90)))),
        ["Skipped:tts-1:0:playback_timeout"]
    );
    assert!(trace(coordinator.handle(Message::Input(done("tts-1", "ended")))).is_empty());
}

#[test]
fn skip_cancels_synthesis_and_playback_and_discards_late_results() {
    let (mut coordinator, _) = coordinator(Config {
        cooldown_seconds: 0,
        ..Config::default()
    });
    coordinator.handle(Message::Input(speak("twitch", "1", "a")));
    coordinator.handle(Message::Input(speak("youtube", "2", "b")));
    assert_eq!(
        trace(coordinator.handle(Message::Input(admin("kick", "!skip")))),
        ["Skipped:tts-1:1:moderator_skip", "synth:tts-2"]
    );
    assert!(trace(coordinator.handle(wav("tts-1"))).is_empty());
    coordinator.handle(wav("tts-2"));
    assert_eq!(
        trace(coordinator.handle(Message::Input(admin("twitch", "!skip")))),
        ["Skipped:tts-2:0:moderator_skip"]
    );
}

#[test]
fn clear_cancels_current_and_pending_and_resets_cooldowns() {
    let (mut coordinator, _) = coordinator(Config::default());
    for platform in ["twitch", "youtube", "kick"] {
        coordinator.handle(Message::Input(speak(platform, "same", "a")));
    }
    assert_eq!(
        trace(coordinator.handle(Message::Input(admin("youtube", "!ttsclear")))),
        [
            "Skipped:tts-1:0:moderator_clear",
            "Skipped:tts-2:0:moderator_clear",
            "Skipped:tts-3:0:moderator_clear"
        ]
    );
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("twitch", "same", "b")))),
        ["Queued:tts-4:1:-", "synth:tts-4"]
    );
    assert!(trace(coordinator.handle(wav("tts-1"))).is_empty());
}

#[test]
fn unavailable_engine_drains_and_rejects_without_retrying() {
    let (mut coordinator, _) = coordinator(Config {
        cooldown_seconds: 0,
        ..Config::default()
    });
    coordinator.handle(Message::Input(speak("twitch", "1", "a")));
    coordinator.handle(Message::Input(speak("youtube", "2", "b")));
    let result = Err(EngineError::Unavailable("no piper".into()));
    assert_eq!(
        trace(coordinator.handle(Message::Synthesized {
            id: "tts-1".into(),
            result
        })),
        [
            "Error:tts-1:1:engine_unavailable",
            "log",
            "Error:tts-2:0:engine_unavailable"
        ]
    );
    assert_eq!(
        trace(coordinator.handle(Message::Input(speak("kick", "3", "c")))),
        ["Error:tts-3:0:engine_unavailable"]
    );
}

#[test]
fn recoverable_errors_and_playback_errors_advance() {
    let (mut coordinator, _) = coordinator(Config {
        cooldown_seconds: 0,
        ..Config::default()
    });
    coordinator.handle(Message::Input(speak("twitch", "1", "a")));
    coordinator.handle(Message::Input(speak("youtube", "2", "b")));
    let result = Err(EngineError::Failed("exit".into()));
    assert_eq!(
        trace(coordinator.handle(Message::Synthesized {
            id: "tts-1".into(),
            result
        })),
        ["Error:tts-1:1:synthesis_failed", "log", "synth:tts-2"]
    );
    coordinator.handle(wav("tts-2"));
    assert_eq!(
        trace(coordinator.handle(Message::Input(done("tts-2", "error")))),
        ["Error:tts-2:0:playback_error"]
    );
}
