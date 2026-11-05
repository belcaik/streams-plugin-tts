use serde_json::{Value, json};
use streams_plugin_tts::{
    config::Config,
    model::{AdminCommand, InputEvent},
    protocol::parse_input_line,
    trigger::{Classification, classify},
};

fn event(platform: &str, kind: &str, roles: &[&str], data: Value) -> InputEvent {
    let value = json!({
        "id": "event-1", "platform": platform, "type": kind,
        "user": {"id":"user-1", "name":"Ana", "roles":roles},
        "data": data, "ts": "2026-01-01T00:00:00Z"
    });
    parse_input_line(&value.to_string()).expect("valid event")
}

fn spoken(event: &InputEvent, config: &Config) -> String {
    match classify(event, config) {
        Classification::Speak(request) => {
            assert_eq!(request.user.name, "Ana");
            request.text
        }
        other => panic!("expected speech, got {other:?}"),
    }
}

#[test]
fn twitch_redemption_and_mod_chat_follow_the_contract() {
    let redemption = event(
        "twitch",
        "redemption",
        &[],
        json!({
            "reward_id":"r", "reward_title":" tts ", "cost":1, "input":"hola Twitch"
        }),
    );
    assert_eq!(spoken(&redemption, &Config::default()), "hola Twitch");
    let chat = event("twitch", "chat", &["moderator"], json!({"message":"!SKIP"}));
    assert_eq!(
        classify(&chat, &Config::default()),
        Classification::Admin(AdminCommand::Skip)
    );
    let ordinary = event("twitch", "chat", &[], json!({"message":"hola"}));
    assert_eq!(
        classify(&ordinary, &Config::default()),
        Classification::Ignore
    );
}

#[test]
fn kick_accepts_each_configured_role_and_requires_command_text() {
    for role in ["subscriber", "moderator", "broadcaster"] {
        let chat = event("kick", "chat", &[role], json!({"message":"!TTS hola Kick"}));
        assert_eq!(spoken(&chat, &Config::default()), "hola Kick");
    }
    for (roles, message) in [
        (&[][..], "!tts hola"),
        (&["subscriber"][..], "hola"),
        (&["subscriber"][..], "!tts"),
    ] {
        let chat = event("kick", "chat", roles, json!({"message":message}));
        assert_eq!(classify(&chat, &Config::default()), Classification::Ignore);
    }
}

#[test]
fn youtube_donation_threshold_is_inclusive() {
    for (amount, expected) in [(0.5, None), (1.0, Some("igual")), (2.0, Some("sobre"))] {
        let donation = event(
            "youtube",
            "donation",
            &[],
            json!({
                "amount":amount, "currency":"USD", "message":expected.unwrap_or("bajo")
            }),
        );
        let classification = classify(&donation, &Config::default());
        match expected {
            Some(text) => assert_eq!(spoken(&donation, &Config::default()), text),
            None => assert_eq!(classification, Classification::Ignore),
        }
    }
}

#[test]
fn youtube_member_chat_speaks_but_common_chat_and_sub_do_not() {
    let member = event(
        "youtube",
        "chat",
        &["subscriber"],
        json!({"message":"soy miembro"}),
    );
    assert_eq!(spoken(&member, &Config::default()), "soy miembro");
    let common = event("youtube", "chat", &[], json!({"message":"hola"}));
    assert_eq!(
        classify(&common, &Config::default()),
        Classification::Ignore
    );
    let sub = event(
        "youtube",
        "sub",
        &["subscriber"],
        json!({"tier":"1000", "months":3}),
    );
    assert_eq!(classify(&sub, &Config::default()), Classification::Ignore);
}

#[test]
fn disabled_triggers_and_custom_admin_commands_are_honored() {
    let mut config = Config {
        skip_commands: vec!["!salta".into()],
        clear_commands: vec!["!limpia".into()],
        ..Config::default()
    };
    config.normalize().expect("valid config");
    for platform in ["twitch", "youtube", "kick"] {
        let clear = event(
            platform,
            "chat",
            &["broadcaster"],
            json!({"message":"!limpia"}),
        );
        assert_eq!(
            classify(&clear, &config),
            Classification::Admin(AdminCommand::Clear)
        );
        let skip = event(
            platform,
            "chat",
            &["moderator"],
            json!({"message":"!salta"}),
        );
        assert_eq!(
            classify(&skip, &config),
            Classification::Admin(AdminCommand::Skip)
        );
    }
    config.triggers.twitch.enabled = false;
    let redemption = event(
        "twitch",
        "redemption",
        &[],
        json!({"reward_title":"TTS", "input":"no"}),
    );
    assert_eq!(classify(&redemption, &config), Classification::Ignore);
    config.triggers.youtube.members_enabled = false;
    let member = event("youtube", "chat", &["subscriber"], json!({"message":"no"}));
    assert_eq!(classify(&member, &config), Classification::Ignore);
    config.triggers.kick.enabled = false;
    let kick = event(
        "kick",
        "chat",
        &["subscriber"],
        json!({"message":"!tts no"}),
    );
    assert_eq!(classify(&kick, &config), Classification::Ignore);
}
