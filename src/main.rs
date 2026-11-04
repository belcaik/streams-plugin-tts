use std::io::{self, BufReader};

use streams_plugin_tts::{
    coordinator::SystemClock,
    engine::ProcessEngine,
    runtime::{self, RuntimeError},
};

fn run() -> Result<(), RuntimeError> {
    let stdout = io::stdout();
    // stderr queda sin bloquear para que otros hilos puedan diagnosticar.
    runtime::run(
        BufReader::new(io::stdin()),
        stdout.lock(),
        io::stderr(),
        |config| ProcessEngine::from_config(&config.engine),
        SystemClock::default(),
    )
}

fn main() {
    if let Err(error) = run() {
        eprintln!("tts: {error}");
        std::process::exit(1);
    }
}
