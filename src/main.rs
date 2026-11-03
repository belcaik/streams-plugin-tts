use std::{
    error::Error,
    io::{self, BufRead, BufReader},
};

use streams_plugin_tts::protocol::{parse_input_line, read_first_init};

fn run() -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let _init = read_first_init(&mut reader)?;
    for line in reader.lines() {
        let line = line?;
        match parse_input_line(&line) {
            Ok(_) => eprintln!("evento válido descartado: worker aún no conectado"),
            Err(error) => eprintln!("entrada descartada: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("tts: {error}");
        std::process::exit(1);
    }
}
