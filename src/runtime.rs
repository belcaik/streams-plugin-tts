use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    io::{BufRead, Write},
    panic::{self, AssertUnwindSafe},
    rc::Rc,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    coordinator::{Clock, Coordinator, Effect, Message},
    engine::{Engine, EngineError, SynthesisRequest},
    model::Init,
    protocol::{ProtocolError, parse_input_line, read_first_init, write_output},
};

#[derive(Debug)]
pub enum RuntimeError {
    Protocol(ProtocolError),
    Panic(&'static str),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::Panic(thread) => write!(formatter, "el hilo {thread} terminó con pánico"),
        }
    }
}

impl Error for RuntimeError {}

impl From<ProtocolError> for RuntimeError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

pub enum Action {
    Synthesize(SynthesisRequest),
    Exit,
}

struct SharedClock<C>(Rc<C>);

impl<C: Clock> Clock for SharedClock<C> {
    fn now(&self) -> Duration {
        self.0.now()
    }
}

/// Aplica mensajes al coordinador y es el único escritor de stdout.
pub struct Driver<C: Clock, W: Write, L: Write> {
    coordinator: Coordinator<SharedClock<C>>,
    clock: Rc<C>,
    output: W,
    log: L,
}

impl<C: Clock, W: Write, L: Write> Driver<C, W, L> {
    pub fn new(init: Init, clock: C, output: W, log: L) -> Self {
        let clock = Rc::new(clock);
        Self {
            coordinator: Coordinator::new(
                init.config,
                init.data_dir,
                SharedClock(Rc::clone(&clock)),
            ),
            clock,
            output,
            log,
        }
    }

    pub fn now(&self) -> Duration {
        self.clock.now()
    }

    pub fn next_deadline(&self) -> Option<Duration> {
        self.coordinator.playback_deadline()
    }

    pub fn output(&self) -> &W {
        &self.output
    }

    pub fn log_line(&mut self, message: &str) {
        let _ = writeln!(self.log, "tts: {message}");
    }

    pub fn dispatch(&mut self, message: Message) -> Result<Vec<Action>, ProtocolError> {
        let mut actions = Vec::new();
        for effect in self.coordinator.handle(message) {
            match effect {
                Effect::Emit(output) => write_output(&mut self.output, &output)?,
                Effect::Log(message) => self.log_line(&message),
                Effect::Synthesize(request) => actions.push(Action::Synthesize(request)),
                Effect::Exit => actions.push(Action::Exit),
            }
        }
        Ok(actions)
    }

    /// Driver determinista: sintetiza en línea y devuelve si se pidió salir.
    pub fn dispatch_inline(
        &mut self,
        engine: &mut impl Engine,
        message: Message,
    ) -> Result<bool, ProtocolError> {
        let mut pending = VecDeque::from([message]);
        let mut exit = false;
        while let Some(message) = pending.pop_front() {
            for action in self.dispatch(message)? {
                match action {
                    Action::Synthesize(request) => {
                        let id = request.id.clone();
                        let result = synthesize_guarded(engine, request);
                        pending.push_back(Message::Synthesized { id, result });
                    }
                    Action::Exit => exit = true,
                }
            }
        }
        Ok(exit)
    }
}

enum Inbox {
    Core(Message),
    Discarded(String),
}

pub fn run<R, W, L, E, F, C>(
    mut reader: R,
    output: W,
    log: L,
    engine_factory: F,
    clock: C,
) -> Result<(), RuntimeError>
where
    R: BufRead + Send + 'static,
    W: Write,
    L: Write,
    E: Engine,
    F: FnOnce(&Config) -> E,
    C: Clock,
{
    let init = read_first_init(&mut reader)?;
    let synthesis_timeout = Duration::from_secs(init.config.engine.synthesis_timeout_seconds);
    let engine = engine_factory(&init.config);
    let mut driver = Driver::new(init, clock, output, log);
    let (inbox_tx, inbox) = mpsc::channel();
    let (synth_tx, synth_rx) = mpsc::channel();
    let reader_thread = spawn_reader(reader, inbox_tx.clone());
    let engine_thread = spawn_engine(engine, synth_rx, inbox_tx);
    let mut active = 0_usize;
    'main: loop {
        let message = match receive(&driver, &inbox) {
            Inbox::Discarded(message) => {
                driver.log_line(&message);
                continue;
            }
            Inbox::Core(message) => message,
        };
        if matches!(message, Message::Synthesized { .. }) {
            active = active.saturating_sub(1);
        }
        for action in driver.dispatch(message)? {
            match action {
                Action::Synthesize(request) => {
                    synth_tx
                        .send(request)
                        .map_err(|_| RuntimeError::Panic("de síntesis"))?;
                    active += 1;
                }
                Action::Exit => break 'main,
            }
        }
    }
    drop(synth_tx);
    if active > 0 && !wait_active(&inbox, active, synthesis_timeout + Duration::from_secs(1)) {
        driver.log_line("la síntesis activa no terminó antes de salir");
        return join(reader_thread, "lector");
    }
    join(reader_thread, "lector")?;
    join(engine_thread, "de síntesis")
}

fn receive<C: Clock, W: Write, L: Write>(
    driver: &Driver<C, W, L>,
    inbox: &Receiver<Inbox>,
) -> Inbox {
    let Some(deadline) = driver.next_deadline() else {
        return inbox.recv().unwrap_or(Inbox::Core(Message::Eof));
    };
    let wait = deadline.saturating_sub(driver.now());
    if wait.is_zero() {
        return Inbox::Core(Message::Tick(driver.now()));
    }
    match inbox.recv_timeout(wait) {
        Ok(message) => message,
        Err(RecvTimeoutError::Timeout) => Inbox::Core(Message::Tick(driver.now())),
        Err(RecvTimeoutError::Disconnected) => Inbox::Core(Message::Eof),
    }
}

fn wait_active(inbox: &Receiver<Inbox>, mut active: usize, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while active > 0 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match inbox.recv_timeout(remaining) {
            Ok(Inbox::Core(Message::Synthesized { .. })) => active -= 1,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    true
}

fn spawn_reader<R: BufRead + Send + 'static>(reader: R, inbox: Sender<Inbox>) -> JoinHandle<bool> {
    thread::spawn(move || {
        let read = panic::catch_unwind(AssertUnwindSafe(|| read_events(reader, &inbox)));
        let _ = inbox.send(Inbox::Core(Message::Eof));
        read.is_ok()
    })
}

fn read_events<R: BufRead>(reader: R, inbox: &Sender<Inbox>) {
    for line in reader.lines() {
        let item = match line {
            Ok(line) => match parse_input_line(&line) {
                Ok(event) => Inbox::Core(Message::Input(event)),
                Err(error) => Inbox::Discarded(format!("entrada descartada: {error}")),
            },
            Err(error) => {
                let _ = inbox.send(Inbox::Discarded(format!("error leyendo stdin: {error}")));
                return;
            }
        };
        if inbox.send(item).is_err() {
            return;
        }
    }
}

fn spawn_engine<E: Engine>(
    mut engine: E,
    requests: Receiver<SynthesisRequest>,
    inbox: Sender<Inbox>,
) -> JoinHandle<bool> {
    thread::spawn(move || {
        for request in requests {
            let id = request.id.clone();
            let result = synthesize_guarded(&mut engine, request);
            if inbox
                .send(Inbox::Core(Message::Synthesized { id, result }))
                .is_err()
            {
                break;
            }
        }
        true
    })
}

fn synthesize_guarded(
    engine: &mut impl Engine,
    request: SynthesisRequest,
) -> Result<Vec<u8>, EngineError> {
    panic::catch_unwind(AssertUnwindSafe(|| engine.synthesize(request)))
        .unwrap_or_else(|_| Err(EngineError::Failed("el motor terminó con pánico".into())))
}

fn join(handle: JoinHandle<bool>, name: &'static str) -> Result<(), RuntimeError> {
    match handle.join() {
        Ok(true) => Ok(()),
        _ => Err(RuntimeError::Panic(name)),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct ZeroClock;

    impl Clock for ZeroClock {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }

    struct StaticEngine;

    impl Engine for StaticEngine {
        fn synthesize(&mut self, _: SynthesisRequest) -> Result<Vec<u8>, EngineError> {
            Ok(b"RIFF\0\0\0\0WAVE".to_vec())
        }
    }

    const INIT: &str = r#"{"type":"init","config":{},"data_dir":"."}"#;
    const REDEMPTION: &str = concat!(
        r#"{"id":"e1","platform":"twitch","type":"redemption","#,
        r#""user":{"id":"1","name":"Ana","roles":[]},"#,
        r#""data":{"reward_title":"TTS","input":"hola"},"ts":""}"#
    );

    fn types(output: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(output)
            .lines()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).expect("JSON line");
                let status = value["data"]["status"].as_str().unwrap_or("audio");
                format!("{}:{status}", value["type"].as_str().expect("type"))
            })
            .collect()
    }

    #[test]
    fn dispatch_writes_emits_and_returns_synthesis_actions() {
        let init = read_first_init(&mut Cursor::new(INIT)).expect("init");
        let mut driver = Driver::new(init, ZeroClock, Vec::new(), Vec::new());
        let event = parse_input_line(REDEMPTION).expect("event");
        let actions = driver.dispatch(Message::Input(event)).expect("dispatch");
        assert!(matches!(actions.as_slice(), [Action::Synthesize(_)]));
        assert_eq!(types(driver.output()), ["tts_status:queued"]);
    }

    #[test]
    fn run_waits_for_the_active_synthesis_and_exits_on_eof() {
        let input = format!("{INIT}\nnot-json\n{REDEMPTION}\n");
        let (mut output, mut log) = (Vec::new(), Vec::new());
        run(
            Cursor::new(input.into_bytes()),
            &mut output,
            &mut log,
            |_| StaticEngine,
            ZeroClock,
        )
        .expect("clean exit");
        let lines = types(&output);
        assert_eq!(lines.first().map(String::as_str), Some("tts_status:queued"));
        assert!(lines.len() == 1 || lines[1..] == ["tts_status:playing", "tts_audio:audio"]);
        assert!(String::from_utf8_lossy(&log).contains("entrada descartada"));
    }
}
