use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    config::Config,
    engine::{EngineError, SynthesisRequest},
    filter::sanitize,
    model::{
        AdminCommand, AudioData, DoneData, InputEvent, OutputEvent, RawRequest, Request,
        StatusData, StatusKind,
    },
    trigger::{Classification, classify},
};

pub trait Clock {
    fn now(&self) -> Duration;
}

pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

pub enum Message {
    Input(InputEvent),
    Synthesized {
        id: String,
        result: Result<Vec<u8>, EngineError>,
    },
    Tick(Duration),
    Eof,
}

pub enum Effect {
    Emit(OutputEvent),
    Synthesize(SynthesisRequest),
    Log(String),
    Exit,
}

enum Phase {
    Synthesizing,
    Playing { deadline: Duration },
}

struct Current {
    request: Request,
    phase: Phase,
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct UserKey {
    platform: String,
    identity: String,
}

pub struct Coordinator<C: Clock> {
    config: Config,
    data_dir: PathBuf,
    clock: C,
    queue: VecDeque<Request>,
    current: Option<Current>,
    cooldowns: HashMap<UserKey, Duration>,
    cancelled: HashSet<String>,
    next_id: u64,
    engine_unavailable: bool,
}

impl<C: Clock> Coordinator<C> {
    pub fn new(config: Config, data_dir: PathBuf, clock: C) -> Self {
        Self {
            config,
            data_dir,
            clock,
            queue: VecDeque::new(),
            current: None,
            cooldowns: HashMap::new(),
            cancelled: HashSet::new(),
            next_id: 0,
            engine_unavailable: false,
        }
    }

    pub fn playback_deadline(&self) -> Option<Duration> {
        match &self.current.as_ref()?.phase {
            Phase::Playing { deadline } => Some(*deadline),
            Phase::Synthesizing => None,
        }
    }

    pub fn handle(&mut self, message: Message) -> Vec<Effect> {
        match message {
            Message::Input(event) => self.input(event),
            Message::Synthesized { id, result } => self.synthesized(id, result),
            Message::Tick(now) => self.tick(now),
            Message::Eof => vec![Effect::Exit],
        }
    }

    fn input(&mut self, event: InputEvent) -> Vec<Effect> {
        if event.kind == "tts_done" {
            return self.done(event);
        }
        match classify(&event, &self.config) {
            Classification::Ignore => Vec::new(),
            Classification::Admin(AdminCommand::Skip) => self.skip(),
            Classification::Admin(AdminCommand::Clear) => self.clear(),
            Classification::Speak(raw) => self.accept(raw),
        }
    }

    fn accept(&mut self, raw: RawRequest) -> Vec<Effect> {
        self.next_id += 1;
        let id = format!("tts-{}", self.next_id);
        let Some(text) = sanitize(&raw.text, &self.config) else {
            return vec![self.error(id, "empty_message", None)];
        };
        let now = self.clock.now();
        let key = UserKey {
            platform: raw.platform.clone(),
            identity: if raw.user.id.trim().is_empty() {
                raw.user.name.trim().to_lowercase()
            } else {
                raw.user.id.clone()
            },
        };
        let cooldown = Duration::from_secs(self.config.cooldown_seconds);
        if self
            .cooldowns
            .get(&key)
            .is_some_and(|last| now.saturating_sub(*last) < cooldown)
        {
            return vec![self.error(id, "cooldown", None)];
        }
        if self.total() >= self.config.max_queue {
            return vec![self.error(id, "queue_full", None)];
        }
        if self.engine_unavailable {
            return vec![self.error(id, "engine_unavailable", None)];
        }
        self.cooldowns.insert(key, now);
        self.queue
            .push_back(Request::new(id, RawRequest { text, ..raw }));
        let queued = self.status(
            self.queue.back().expect("just pushed").id.clone(),
            StatusKind::Queued,
            None,
            None,
        );
        let mut effects = vec![queued];
        effects.extend(self.start_next());
        effects
    }

    fn start_next(&mut self) -> Option<Effect> {
        if self.current.is_some() {
            return None;
        }
        let request = self.queue.pop_front()?;
        let synthesis = SynthesisRequest {
            id: request.id.clone(),
            text: request.text.clone(),
            data_dir: self.data_dir.clone(),
            max_audio_bytes: self.config.engine.max_audio_bytes,
            deadline: Instant::now()
                + Duration::from_secs(self.config.engine.synthesis_timeout_seconds),
        };
        self.current = Some(Current {
            request,
            phase: Phase::Synthesizing,
        });
        Some(Effect::Synthesize(synthesis))
    }

    fn synthesized(&mut self, id: String, result: Result<Vec<u8>, EngineError>) -> Vec<Effect> {
        self.cancelled.remove(&id);
        if self.current.as_ref().map(|current| &current.request.id) != Some(&id)
            || !matches!(
                self.current.as_ref().map(|current| &current.phase),
                Some(Phase::Synthesizing)
            )
        {
            return Vec::new();
        }
        let current = self.current.take().expect("matched current");
        match result {
            Ok(wav) => {
                let deadline =
                    self.clock.now() + Duration::from_secs(self.config.playback_timeout_seconds);
                let request = current.request;
                self.current = Some(Current {
                    request: request.clone(),
                    phase: Phase::Playing { deadline },
                });
                vec![
                    self.status(id.clone(), StatusKind::Playing, None, None),
                    Effect::Emit(OutputEvent::audio(AudioData {
                        id,
                        mime: "audio/wav".into(),
                        wav_base64: STANDARD.encode(wav),
                        text: request.text,
                        user_name: request.user.name,
                    })),
                ]
            }
            Err(error) => {
                let unavailable = matches!(error, EngineError::Unavailable(_));
                let message = error.to_string();
                let mut effects = vec![
                    self.error(id, error.reason(), Some(message.clone())),
                    Effect::Log(message.clone()),
                ];
                if unavailable {
                    self.engine_unavailable = true;
                    while let Some(request) = self.queue.pop_front() {
                        effects.push(self.error(
                            request.id,
                            "engine_unavailable",
                            Some(message.clone()),
                        ));
                    }
                }
                effects.extend(self.start_next());
                effects
            }
        }
    }

    fn done(&mut self, event: InputEvent) -> Vec<Effect> {
        let Ok(done) = serde_json::from_value::<DoneData>(event.data) else {
            return Vec::new();
        };
        let Some(current) = &self.current else {
            return Vec::new();
        };
        if current.request.id != done.id || !matches!(current.phase, Phase::Playing { .. }) {
            return Vec::new();
        }
        match done.outcome.as_str() {
            "ended" => {
                self.current = None;
                self.start_next().into_iter().collect()
            }
            "error" => {
                self.current = None;
                let mut effects = vec![self.error(done.id, "playback_error", None)];
                effects.extend(self.start_next());
                effects
            }
            _ => Vec::new(),
        }
    }

    fn tick(&mut self, now: Duration) -> Vec<Effect> {
        let Some(Current {
            phase: Phase::Playing { deadline },
            ..
        }) = &self.current
        else {
            return Vec::new();
        };
        if now < *deadline {
            return Vec::new();
        }
        let current = self.current.take().expect("matched current");
        let mut effects = vec![self.status(
            current.request.id,
            StatusKind::Skipped,
            Some("playback_timeout".into()),
            None,
        )];
        effects.extend(self.start_next());
        effects
    }

    fn skip(&mut self) -> Vec<Effect> {
        let Some(current) = self.current.take() else {
            return Vec::new();
        };
        self.remember_cancelled(&current);
        let mut effects = vec![self.status(
            current.request.id,
            StatusKind::Skipped,
            Some("moderator_skip".into()),
            None,
        )];
        effects.extend(self.start_next());
        effects
    }

    fn clear(&mut self) -> Vec<Effect> {
        let mut requests = Vec::new();
        if let Some(current) = self.current.take() {
            self.remember_cancelled(&current);
            requests.push(current.request);
        }
        requests.extend(self.queue.drain(..));
        self.cooldowns.clear();
        requests
            .into_iter()
            .map(|request| {
                self.status(
                    request.id,
                    StatusKind::Skipped,
                    Some("moderator_clear".into()),
                    None,
                )
            })
            .collect()
    }

    fn remember_cancelled(&mut self, current: &Current) {
        if matches!(current.phase, Phase::Synthesizing) {
            self.cancelled.insert(current.request.id.clone());
        }
    }

    fn total(&self) -> usize {
        self.queue.len() + usize::from(self.current.is_some())
    }

    fn status(
        &self,
        id: String,
        status: StatusKind,
        reason: Option<String>,
        message: Option<String>,
    ) -> Effect {
        Effect::Emit(OutputEvent::status(StatusData {
            id,
            status,
            queue_len: self.total(),
            reason,
            message,
        }))
    }

    fn error(&self, id: String, reason: &str, message: Option<String>) -> Effect {
        self.status(id, StatusKind::Error, Some(reason.into()), message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::User;

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }

    #[test]
    fn initial_state_and_happy_path_follow_the_protocol() {
        let mut coordinator = Coordinator::new(Config::default(), ".".into(), FixedClock);
        assert_eq!(coordinator.playback_deadline(), None);
        let raw = RawRequest {
            platform: "twitch".into(),
            user: User {
                id: "1".into(),
                name: "Ana".into(),
                roles: Vec::new(),
            },
            text: "hola".into(),
        };
        let effects = coordinator.accept(raw);
        assert!(matches!(effects.first(), Some(Effect::Emit(_))));
        assert!(matches!(effects.get(1), Some(Effect::Synthesize(_))));
        coordinator.handle(Message::Synthesized {
            id: "tts-1".into(),
            result: Ok(b"RIFF0000WAVE".to_vec()),
        });
        assert_eq!(
            coordinator.playback_deadline(),
            Some(Duration::from_secs(90))
        );
    }
}
