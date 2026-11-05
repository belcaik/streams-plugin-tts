# tts

Plugin oficial de streams-tools que lee en voz alta interacciones del stream con un motor de síntesis **local y libre** (Piper o eSpeak NG). Pone las solicitudes en una cola única, las modera y las reproduce en un overlay transparente para OBS. No usa servicios en la nube ni descarga motores o voces.

## Disparadores

| Plataforma | Evento | Condición | Texto leído |
|---|---|---|---|
| Twitch | `redemption` | `triggers.twitch.enabled` y el título de la recompensa coincide con `reward_title` (sin espacios externos, ASCII sin distinguir mayúsculas) | `data.input` |
| YouTube | `donation` (Super Chat) | `donations_enabled`, importe finito y `>= minimum_amount` | `data.message` |
| YouTube | `chat` de miembro | `members_enabled` y `user.roles` contiene `subscriber` | `data.message` |
| Kick | `chat` | `triggers.kick.enabled`, el primer token es `command`, hay texto detrás y el usuario tiene uno de `roles` | El resto tras el comando |
| Todas | `chat` | moderador o broadcaster y el mensaje completo es un comando de `skip_commands` o `clear_commands` | Comando administrativo |

**Miembros de YouTube:** el esquema núcleo de `sub` sólo tiene `tier` y `months` y no contiene `data.message`. Por eso el disparador “miembro” significa **un evento `chat` de YouTube cuyo `user.roles` contiene `subscriber`**, y lee `data.message`. Super Chat usa `donation` y también lee `data.message`. Un evento de alta/renovación `sub` no dispara TTS.

Se ignoran el resto de eventos: donaciones y chat común de Twitch, chat de Kick sin el prefijo, `sub` de YouTube y eventos de plugins distintos de `tts_done`.

## Instalación

Añade el repositorio a `config.toml` de streams-tools para instalar el plugin cuando falte:

```toml
[plugins]
install = ["https://github.com/belcaik/streams-plugin-tts"]
```

También puedes instalarlo desde la línea de comandos:

```sh
streams-tools plugin add https://github.com/belcaik/streams-plugin-tts
```

Ambos métodos descargan el ZIP de la release apropiado para la plataforma, verifican su SHA-256 y conservan la carpeta `tts/` administrada por el host. Para fijar la versión usa `https://github.com/belcaik/streams-plugin-tts@v0.1.1`. El plugin no incluye motores ni voces: instálalos aparte (ver más abajo).

### Fuente de navegador OBS

Añade un Browser Source con URL `http://127.0.0.1:7777/p/tts/`. Un tamaño inicial razonable es 700×180 píxeles. Activa **Controlar audio vía OBS** en el Browser Source para capturar el audio y mezclarlo como cualquier otra fuente. El fondo es transparente.

## Configuración

No edites `plugin.toml`: una actualización reemplaza la carpeta completa del plugin. Define sólo tus cambios en `config.toml` de streams-tools bajo `[plugins.config.tts]`. El host hace deep merge sobre los defaults `[config]` del manifiesto; las tablas se mezclan recursivamente y los escalares y arrays se reemplazan.

Esta es la configuración completa expresada como overrides del host; normalmente basta con incluir las claves que quieras cambiar:

```toml
[plugins.config.tts]
max_message_chars = 280
max_queue = 20
cooldown_seconds = 60
playback_timeout_seconds = 90
remove_urls = true
max_repeated_chars = 3
blocked_words = []
blocked_replacement = "***"
skip_commands = ["!skip"]
clear_commands = ["!ttsclear"]
volume = 1.0
show_text = true
show_user = true

[plugins.config.tts.triggers.twitch]
enabled = true
reward_title = "TTS"

[plugins.config.tts.triggers.youtube]
donations_enabled = true
minimum_amount = 1.0
members_enabled = true

[plugins.config.tts.triggers.kick]
enabled = true
command = "!tts"
roles = ["subscriber", "moderator", "broadcaster"]

[plugins.config.tts.engine]
kind = "piper"
speed = 1.0
synthesis_timeout_seconds = 30
max_audio_bytes = 8388608

[plugins.config.tts.engine.piper]
binary = "piper"
model = "models/voice.onnx"
extra_args = []

[plugins.config.tts.engine.espeak]
binary = "espeak-ng"
voice = "es"
base_wpm = 175
extra_args = []
```

Las claves desconocidas o fuera de rango invalidan la configuración y el worker termina con error al iniciar.

| Clave | Default | Rango / regla |
|---|---|---|
| `max_message_chars` | `280` | 1–1000 escalares Unicode |
| `max_queue` | `20` | 1–100 solicitudes, incluida la actual |
| `cooldown_seconds` | `60` | 0–3600; `0` lo desactiva |
| `playback_timeout_seconds` | `90` | 5–600 |
| `remove_urls` | `true` | booleano |
| `max_repeated_chars` | `3` | 1–10 |
| `blocked_words` | `[]` | sin vacíos; se normaliza a minúsculas y se deduplica |
| `blocked_replacement` | `"***"` | máximo 32 caracteres, sin controles |
| `skip_commands` | `["!skip"]` | empiezan por `!`, sin espacios, sin solaparse con `clear_commands` |
| `clear_commands` | `["!ttsclear"]` | igual que `skip_commands` |
| `volume` | `1.0` | finito, 0–1 (lo aplica el overlay) |
| `show_text` | `true` | muestra el texto en el overlay |
| `show_user` | `true` | muestra el usuario en el overlay |
| `triggers.twitch.enabled` | `true` | booleano |
| `triggers.twitch.reward_title` | `"TTS"` | título de la recompensa |
| `triggers.youtube.donations_enabled` | `true` | booleano |
| `triggers.youtube.minimum_amount` | `1.0` | finito, `>= 0` |
| `triggers.youtube.members_enabled` | `true` | booleano |
| `triggers.kick.enabled` | `true` | booleano |
| `triggers.kick.command` | `"!tts"` | empieza por `!`, sin espacios |
| `triggers.kick.roles` | los tres | sólo `subscriber`, `moderator`, `broadcaster`; no vacío |
| `engine.kind` | `"piper"` | `piper` o `espeak` |
| `engine.speed` | `1.0` | finito, 0.5–2.0 |
| `engine.synthesis_timeout_seconds` | `30` | 1–120 |
| `engine.max_audio_bytes` | `8388608` | 65536–16777216 (64 KiB–16 MiB) |
| `engine.piper.binary` | `"piper"` | no vacío; ruta absoluta o nombre en `PATH` |
| `engine.piper.model` | `"models/voice.onnx"` | no vacío |
| `engine.piper.extra_args` | `[]` | sin cadenas vacías ni NUL |
| `engine.espeak.binary` | `"espeak-ng"` | no vacío |
| `engine.espeak.voice` | `"es"` | no vacía |
| `engine.espeak.base_wpm` | `175` | palabras por minuto a `speed = 1.0` |
| `engine.espeak.extra_args` | `[]` | sin cadenas vacías ni NUL |

El binario y el modelo no se comprueban al iniciar: si faltan, el worker sigue vivo y lo informa al procesar la primera solicitud.

## Cola, filtro y moderación

- **Cola:** FIFO global para todas las plataformas, con un máximo total de `max_queue`. Sólo hay una síntesis o reproducción activa; la siguiente empieza tras `tts_done`, un skip, un error o el timeout de reproducción.
- **Cooldown:** por `(plataforma, user.id)`, o por nombre normalizado si el id está vacío. Se registra sólo cuando la solicitud saneada entra en la cola.
- **Filtro**, en este orden:
  1. `\r`, `\n`, `\t` y cualquier whitespace pasan a espacio; se eliminan los demás controles, se colapsan espacios y se recorta;
  2. si `remove_urls`, se eliminan tokens que empiezan por `http://`, `https://` o `www.` (no se detectan dominios desnudos);
  3. las palabras de `blocked_words` se sustituyen por `blocked_replacement` sólo si coinciden como palabra completa, conservando la puntuación;
  4. las repeticiones de un mismo carácter se limitan a `max_repeated_chars`;
  5. se trunca a `max_message_chars` y se recorta.
- **Comandos:** `skip_commands` cancela la solicitud actual; `clear_commands` cancela la actual y todas las pendientes y reinicia los cooldowns. Sólo moderadores y broadcasters pueden usarlos, desde cualquier plataforma. Un `subscriber` de Kick puede usar `!tts`, pero no administrar.

Estados de `tts_status`: `queued`, `playing`, `skipped`, `error`. Códigos de `reason`:

| `reason` | Estado | Significado |
|---|---|---|
| `cooldown` | `error` | El usuario está en cooldown |
| `queue_full` | `error` | La cola alcanzó `max_queue` |
| `empty_message` | `error` | El texto quedó vacío tras el filtro |
| `moderator_skip` | `skipped` | Un moderador saltó la solicitud |
| `moderator_clear` | `skipped` | Un moderador vació la cola |
| `playback_timeout` | `skipped` | El overlay no confirmó a tiempo |
| `playback_error` | `error` | El overlay no pudo reproducir |
| `engine_unavailable` | `error` | No se encontró el ejecutable del motor |
| `synthesis_timeout` | `error` | El motor superó `synthesis_timeout_seconds` |
| `synthesis_failed` | `error` | El motor terminó con error |
| `invalid_wav` | `error` | La salida no es un WAV `RIFF`/`WAVE` |
| `audio_too_large` | `error` | El WAV supera `max_audio_bytes` |

## Contrato JSON

stdout contiene sólo JSONL; los diagnósticos van a stderr.

```json
{"type":"tts_audio","data":{"id":"tts-7","mime":"audio/wav","wav_base64":"UklGR...","text":"hola","user_name":"Ana"}}
{"type":"tts_status","data":{"id":"tts-7","status":"queued","queue_len":1,"reason":null,"message":null}}
{"type":"tts_status","data":{"id":"tts-7","status":"skipped","queue_len":0,"reason":"moderator_skip","message":null}}
```

`id`, `reason` y `message` aparecen siempre; `reason`/`message` son `null` cuando no aplican. `queue_len` cuenta todas las solicitudes aceptadas sin terminar, incluida la actual. El audio viaja como WAV en base64 (≈33 % mayor que el binario), acotado por `max_audio_bytes`; el payload nunca contiene rutas locales.

El overlay confirma con `tts_done` sólo el id que reproduce:

```js
await window.streams.emit("tts_done", { id, outcome: "ended" });
await window.streams.emit("tts_done", { id, outcome: "error" });
```

El worker ignora ids viejos, duplicados y outcomes desconocidos.

## Motores externos

El plugin **no descarga ni redistribuye** motores ni modelos. Instálalos manualmente.

**Piper (Windows/Linux):** descarga el asset de tu plataforma desde `https://github.com/rhasspy/piper/releases`, extráelo y configura `engine.piper.binary` con la ruta absoluta o añádelo a `PATH`. Descarga manualmente un modelo libre `.onnx` junto con su `.onnx.json`, revisa su licencia y configura `engine.piper.model`.

**Piper (macOS):** el repositorio oficial archivado no publica un asset macOS mantenido. Compila el binario C++ standalone desde `https://github.com/rhasspy/piper` con CMake y C++17, o usa un binario propio auditado, y configura su ruta:

```sh
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --config Release
```

Prueba Piper: `echo 'hola' | piper --model /ruta/voz.onnx --output_file prueba.wav`.

**eSpeak NG:** en Windows instala el MSI de `https://github.com/espeak-ng/espeak-ng/releases` y añádelo a `PATH` o usa la ruta absoluta. En Linux: `sudo apt install espeak-ng`, `sudo dnf install espeak-ng` o `sudo pacman -S espeak-ng`. En macOS: `brew install espeak-ng`.

Prueba eSpeak NG: `echo 'hola' | espeak-ng -v es -w prueba.wav`.

`rhasspy/piper` está archivado. eSpeak NG se distribuye bajo GPL-3.0-or-later. Cada voz de Piper tiene su propia licencia: revísala antes de usarla.

### Velocidad y argumentos

- Piper recibe `--length_scale` con `1/speed`.
- eSpeak NG recibe `-s` con `round(base_wpm * speed)`.
- `extra_args` del administrador va antes de los argumentos controlados por el plugin (`--model`, `--output_file`, `-v`, `-w`…). El texto siempre llega por stdin.

## Diagnóstico

- **Binario ausente:** stderr muestra un error claro con el motor y el binario, se emite `tts_status/error` con `engine_unavailable` y el worker sigue vivo pero queda desactivado internamente hasta reiniciarlo: rechaza nuevas solicitudes sin reintentar.
- **Modelo o voz incorrectos:** el motor termina con error (`synthesis_failed`) y stderr incluye hasta 8 KiB de su salida de error.
- **Exit no-cero o timeout:** `synthesis_failed` o `synthesis_timeout`; el proceso se mata al vencer el plazo y la cola continúa.
- **WAV inválido o demasiado grande:** `invalid_wav` o `audio_too_large`.
- **Autoplay bloqueado:** si `play()` falla, el overlay confirma `error` y el worker emite `playback_error`. En OBS el audio del Browser Source no requiere interacción.
- **Overlay cerrado:** sin `tts_done`, la solicitud se salta con `playback_timeout` tras `playback_timeout_seconds` y la cola no se bloquea.

## Personalización CSS

No edites el plugin. Crea `styles/tts.css` en la raíz de streams-tools (o en `STREAMS_STYLES_DIR`) y recarga el Browser Source. El host lo sirve como `./custom.css`, la última hoja, por lo que tus reglas ganan la cascada.

Selectores: `.st-tts` (raíz), `.st-tts__card`, `.st-tts__user`, `.st-tts__text`, `.st-tts__status`, `.st-tts__audio` (oculto). Estados en la raíz: `.st-tts--idle`, `.st-tts--playing`, `.st-tts--error`. `show_text` y `show_user` usan el atributo `hidden`.

| Variable | Uso |
|---|---|
| `--st-tts-font-family` | Fuente |
| `--st-tts-width` | Ancho del overlay |
| `--st-tts-padding` | Relleno de la tarjeta |
| `--st-tts-gap` | Separación entre líneas |
| `--st-tts-color` | Color del texto |
| `--st-tts-muted-color` | Color del estado |
| `--st-tts-background` | Fondo de la tarjeta |
| `--st-tts-accent` | Usuario y borde |
| `--st-tts-error` | Color en estado de error |
| `--st-tts-radius` | Radio de borde |
| `--st-tts-shadow` | Sombra |
| `--st-tts-user-size` | Tamaño del usuario |
| `--st-tts-text-size` | Tamaño del texto |
| `--st-tts-status-size` | Tamaño del estado |
| `--st-tts-transition-duration` | Duración de transiciones |
| `--st-tts-idle-opacity` | Opacidad en reposo |
| `--st-tts-idle-transform` | Transformación en reposo |

Ejemplo de `styles/tts.css`:

```css
.st-tts {
  --st-tts-accent: #53fc18;
  --st-tts-text-size: 28px;
  --st-tts-idle-opacity: 0.4;
}

.st-tts--playing .st-tts__text {
  text-transform: uppercase;
}
```

## Seguridad

- Nunca se usa shell (`sh -c`, `cmd /C`, PowerShell) ni interpolación: el motor se lanza directamente y el texto saneado viaja por stdin, jamás como argumento.
- Se eliminan controles y se aplican límites de longitud, cola, tiempo y tamaño de audio.
- El WAV temporal se crea en exclusiva en `data_dir/tmp/` con un nombre propio del worker y se borra en éxito y error. La ruta nunca viene del chat.
- El texto sin sanear no se escribe en stderr.
- El overlay inserta datos sólo con `textContent`, nunca como HTML.
- La configuración principal, las rutas de binarios y `extra_args` se consideran confiables: las controla el administrador del stream.

## Desarrollo

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test
cargo build --release --bin tts
stat -c '%s bytes' target/release/tts
```

Las pruebas usan el binario `fake-tts-engine` y motores en memoria: no requieren Piper ni eSpeak NG, red ni sleeps.

Medición base de memoria en Linux:

```sh
printf '%s\n' '{"type":"init","config":{"engine":{"kind":"espeak"}},"data_dir":"."}' | /usr/bin/time -v target/release/tts
```

Mide el RSS base del worker sin sintetizar (`Maximum resident set size`). Una medición end-to-end requiere un motor instalado, y el RSS de Piper/eSpeak NG se mide aparte porque es un proceso externo.

Comprobaciones informativas de dependencias dinámicas (no son gates de CI): `ldd target/release/tts` en Linux, `otool -L target/release/tts` en macOS y `dumpbin /dependents target\release\tts.exe` en Windows.
