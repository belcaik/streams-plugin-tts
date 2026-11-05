use std::{error::Error, fmt};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub max_message_chars: usize,
    pub max_queue: usize,
    pub cooldown_seconds: u64,
    pub playback_timeout_seconds: u64,
    pub remove_urls: bool,
    pub max_repeated_chars: usize,
    pub blocked_words: Vec<String>,
    pub blocked_replacement: String,
    pub skip_commands: Vec<String>,
    pub clear_commands: Vec<String>,
    pub volume: f64,
    pub show_text: bool,
    pub show_user: bool,
    pub triggers: Triggers,
    pub engine: EngineConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_message_chars: 280,
            max_queue: 20,
            cooldown_seconds: 60,
            playback_timeout_seconds: 90,
            remove_urls: true,
            max_repeated_chars: 3,
            blocked_words: Vec::new(),
            blocked_replacement: "***".into(),
            skip_commands: vec!["!skip".into()],
            clear_commands: vec!["!ttsclear".into()],
            volume: 1.0,
            show_text: true,
            show_user: true,
            triggers: Triggers::default(),
            engine: EngineConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Triggers {
    pub twitch: TwitchTrigger,
    pub youtube: YoutubeTrigger,
    pub kick: KickTrigger,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TwitchTrigger {
    pub enabled: bool,
    pub reward_title: String,
}

impl Default for TwitchTrigger {
    fn default() -> Self {
        Self {
            enabled: true,
            reward_title: "TTS".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct YoutubeTrigger {
    pub donations_enabled: bool,
    pub minimum_amount: f64,
    pub members_enabled: bool,
}

impl Default for YoutubeTrigger {
    fn default() -> Self {
        Self {
            donations_enabled: true,
            minimum_amount: 1.0,
            members_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KickTrigger {
    pub enabled: bool,
    pub command: String,
    pub roles: Vec<String>,
}

impl Default for KickTrigger {
    fn default() -> Self {
        Self {
            enabled: true,
            command: "!tts".into(),
            roles: vec![
                "subscriber".into(),
                "moderator".into(),
                "broadcaster".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub kind: EngineKind,
    pub speed: f64,
    pub synthesis_timeout_seconds: u64,
    pub max_audio_bytes: usize,
    pub piper: PiperConfig,
    pub espeak: EspeakConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            kind: EngineKind::Piper,
            speed: 1.0,
            synthesis_timeout_seconds: 30,
            max_audio_bytes: 8 * 1024 * 1024,
            piper: PiperConfig::default(),
            espeak: EspeakConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Piper,
    Espeak,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PiperConfig {
    pub binary: String,
    pub model: String,
    pub extra_args: Vec<String>,
}

impl Default for PiperConfig {
    fn default() -> Self {
        Self {
            binary: "piper".into(),
            model: "models/voice.onnx".into(),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EspeakConfig {
    pub binary: String,
    pub voice: String,
    pub base_wpm: u32,
    pub extra_args: Vec<String>,
}

impl Default for EspeakConfig {
    fn default() -> Self {
        Self {
            binary: "espeak-ng".into(),
            voice: "es".into(),
            base_wpm: 175,
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "configuración inválida: {}", self.0)
    }
}

impl Error for ConfigError {}

impl Config {
    pub fn normalize(&mut self) -> Result<(), ConfigError> {
        require(
            (1..=1000).contains(&self.max_message_chars),
            "max_message_chars fuera de rango",
        )?;
        require(
            (1..=100).contains(&self.max_queue),
            "max_queue fuera de rango",
        )?;
        require(
            self.cooldown_seconds <= 3600,
            "cooldown_seconds fuera de rango",
        )?;
        require(
            (5..=600).contains(&self.playback_timeout_seconds),
            "playback_timeout_seconds fuera de rango",
        )?;
        require(
            (1..=10).contains(&self.max_repeated_chars),
            "max_repeated_chars fuera de rango",
        )?;
        require(
            self.volume.is_finite() && (0.0..=1.0).contains(&self.volume),
            "volume inválido",
        )?;
        require(
            self.blocked_replacement.chars().count() <= 32
                && !self.blocked_replacement.chars().any(char::is_control),
            "blocked_replacement inválido",
        )?;
        require(
            self.triggers.youtube.minimum_amount.is_finite()
                && self.triggers.youtube.minimum_amount >= 0.0,
            "minimum_amount inválido",
        )?;
        require(
            self.engine.speed.is_finite() && (0.5..=2.0).contains(&self.engine.speed),
            "speed inválido",
        )?;
        require(
            (1..=120).contains(&self.engine.synthesis_timeout_seconds),
            "synthesis_timeout_seconds fuera de rango",
        )?;
        require(
            (64 * 1024..=16 * 1024 * 1024).contains(&self.engine.max_audio_bytes),
            "max_audio_bytes fuera de rango",
        )?;
        normalize_words(&mut self.blocked_words)?;
        normalize_commands(&mut self.skip_commands)?;
        normalize_commands(&mut self.clear_commands)?;
        require(
            !self
                .skip_commands
                .iter()
                .any(|item| self.clear_commands.contains(item)),
            "comandos administrativos solapados",
        )?;
        normalize_command(&mut self.triggers.kick.command)?;
        normalize_roles(&mut self.triggers.kick.roles)?;
        for value in [
            &self.engine.piper.binary,
            &self.engine.piper.model,
            &self.engine.espeak.binary,
            &self.engine.espeak.voice,
        ] {
            require(!value.trim().is_empty(), "binario, modelo o voz vacío")?;
        }
        validate_args(&self.engine.piper.extra_args)?;
        validate_args(&self.engine.espeak.extra_args)
    }
}

fn require(condition: bool, message: &str) -> Result<(), ConfigError> {
    condition
        .then_some(())
        .ok_or_else(|| ConfigError(message.into()))
}

fn normalize_list(
    values: &mut Vec<String>,
    mut normalize: impl FnMut(String) -> Result<String, ConfigError>,
) -> Result<(), ConfigError> {
    let mut normalized = Vec::with_capacity(values.len());
    for value in std::mem::take(values) {
        let value = normalize(value)?;
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    *values = normalized;
    Ok(())
}

fn normalize_words(words: &mut Vec<String>) -> Result<(), ConfigError> {
    normalize_list(words, |word| {
        let word = word.trim().to_lowercase();
        require(!word.is_empty(), "palabra bloqueada vacía")?;
        Ok(word)
    })
}

fn normalize_command(command: &mut String) -> Result<(), ConfigError> {
    *command = command.trim().to_ascii_lowercase();
    require(
        command.starts_with('!') && !command.chars().any(char::is_whitespace),
        "comando inválido",
    )
}

fn normalize_commands(commands: &mut Vec<String>) -> Result<(), ConfigError> {
    normalize_list(commands, |mut command| {
        normalize_command(&mut command)?;
        Ok(command)
    })
}

fn normalize_roles(roles: &mut Vec<String>) -> Result<(), ConfigError> {
    normalize_list(roles, |role| {
        let role = role.trim().to_ascii_lowercase();
        require(
            matches!(role.as_str(), "subscriber" | "moderator" | "broadcaster"),
            "rol Kick inválido",
        )?;
        Ok(role)
    })?;
    require(!roles.is_empty(), "roles Kick vacíos")
}

fn validate_args(args: &[String]) -> Result<(), ConfigError> {
    require(
        args.iter()
            .all(|arg| !arg.is_empty() && !arg.contains('\0')),
        "extra_args inválido",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_manifest() {
        let config = Config::default();
        assert_eq!(config.max_message_chars, 280);
        assert_eq!(config.max_queue, 20);
        assert_eq!(config.cooldown_seconds, 60);
        assert_eq!(config.playback_timeout_seconds, 90);
        assert_eq!(config.triggers.twitch.reward_title, "TTS");
        assert_eq!(config.triggers.kick.command, "!tts");
        assert_eq!(config.engine.kind, EngineKind::Piper);
        assert_eq!(config.engine.max_audio_bytes, 8_388_608);
        assert_eq!(config.engine.espeak.base_wpm, 175);
    }

    #[test]
    fn rejects_representative_boundaries_and_non_finite_values() {
        let invalid: [fn(&mut Config); 7] = [
            |config| config.max_message_chars = 0,
            |config| config.max_queue = 101,
            |config| config.cooldown_seconds = 3601,
            |config| config.playback_timeout_seconds = 4,
            |config| config.volume = f64::NAN,
            |config| config.volume = f64::INFINITY,
            |config| config.engine.speed = 2.1,
        ];
        for invalidate in invalid {
            let mut config = Config::default();
            invalidate(&mut config);
            assert!(config.normalize().is_err());
        }
        assert!(serde_json::from_str::<Config>(r#"{"volume":1e999}"#).is_err());
    }

    #[test]
    fn normalizes_lists_and_rejects_overlap() {
        let mut config = Config {
            blocked_words: vec![" Malo ".into(), "MALO".into()],
            skip_commands: vec![" !SKIP ".into(), "!skip".into()],
            triggers: Triggers {
                kick: KickTrigger {
                    roles: vec![" Subscriber ".into(), "subscriber".into()],
                    ..KickTrigger::default()
                },
                ..Triggers::default()
            },
            ..Config::default()
        };
        config.normalize().expect("valid config");
        assert_eq!(config.blocked_words, ["malo"]);
        assert_eq!(config.skip_commands, ["!skip"]);
        assert_eq!(config.triggers.kick.roles, ["subscriber"]);
        config.clear_commands = vec!["!SKIP".into()];
        assert!(config.normalize().is_err());
    }

    #[test]
    fn rejects_invalid_roles_args_and_unknown_fields() {
        let mut config = Config::default();
        config.triggers.kick.roles = vec!["vip".into()];
        assert!(config.normalize().is_err());
        config.triggers.kick.roles = vec!["subscriber".into()];
        config.engine.piper.extra_args = vec!["bad\0arg".into()];
        assert!(config.normalize().is_err());
        assert!(serde_json::from_str::<Config>(r#"{"unknown":true}"#).is_err());
        assert!(serde_json::from_str::<Config>(r#"{"engine":{"unknown":true}}"#).is_err());
    }
}
