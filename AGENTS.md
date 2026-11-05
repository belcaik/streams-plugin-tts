# Reglas para agentes

- Usa Conventional Commits con descripciones en español, imperativo y minúscula.
- Haz commits atómicos; cada PR admite hasta 10 archivos y 400 líneas sin contar `Cargo.lock`.
- Ejecuta `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` y `node --test` antes de cada commit.
- Reserva stdout para JSONL y escribe diagnósticos en stderr; nunca pases chat por un shell.
- Mantén tests deterministas, sin sleeps, red ni motores TTS instalados.
- Conserva los contratos `tts_audio`, `tts_status`, `tts_done`, las clases BEM y variables CSS públicas.
- Los motores y modelos son externos: el plugin nunca los descarga.
