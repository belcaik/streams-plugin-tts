use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut text = String::new();
    if io::stdin().read_to_string(&mut text).is_err() {
        return ExitCode::from(2);
    }
    if args.iter().any(|argument| argument == &text) {
        eprintln!("el texto llegó como argumento");
        return ExitCode::from(3);
    }
    if args.iter().any(|argument| argument == "--fake-fail") {
        eprintln!("fallo falso");
        return ExitCode::from(1);
    }
    let mut output = None;
    for (index, argument) in args.iter().enumerate() {
        if matches!(argument.as_str(), "--output_file" | "--output-file" | "-w") {
            output = args.get(index + 1).cloned();
        }
    }
    let Some(output) = output else {
        return ExitCode::from(2);
    };
    let bytes = if args.iter().any(|argument| argument == "--fake-invalid") {
        b"not a wav".to_vec()
    } else if args.iter().any(|argument| argument == "--fake-oversize") {
        let mut bytes = vec![0; 70 * 1024];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[8..12].copy_from_slice(b"WAVE");
        bytes
    } else {
        wav()
    };
    match File::create(output).and_then(|mut file| file.write_all(&bytes)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(2),
    }
}

fn wav() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(46);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&38_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&0_i16.to_le_bytes());
    bytes
}
