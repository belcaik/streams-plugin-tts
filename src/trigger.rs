use crate::{
    config::Config,
    model::{AdminCommand, InputEvent, RawRequest},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    Ignore,
    Admin(AdminCommand),
    Speak(RawRequest),
}

pub fn classify(event: &InputEvent, config: &Config) -> Classification {
    if event.kind == "chat"
        && let Some(admin) = admin_command(event, config)
    {
        return Classification::Admin(admin);
    }
    match event.platform.as_str() {
        "twitch" if event.kind == "redemption" => twitch(event, config),
        "youtube" if event.kind == "donation" => youtube_donation(event, config),
        "youtube" if event.kind == "chat" => youtube_member(event, config),
        "kick" if event.kind == "chat" => kick(event, config),
        _ => Classification::Ignore,
    }
}

fn admin_command(event: &InputEvent, config: &Config) -> Option<AdminCommand> {
    if !event.has_role("moderator") && !event.has_role("broadcaster") {
        return None;
    }
    let message = event.string("message")?.trim();
    if config
        .skip_commands
        .iter()
        .any(|cmd| message.eq_ignore_ascii_case(cmd))
    {
        Some(AdminCommand::Skip)
    } else if config
        .clear_commands
        .iter()
        .any(|cmd| message.eq_ignore_ascii_case(cmd))
    {
        Some(AdminCommand::Clear)
    } else {
        None
    }
}

fn speak(event: &InputEvent, text: &str) -> Classification {
    Classification::Speak(RawRequest {
        platform: event.platform.clone(),
        user: event.user.clone(),
        text: text.to_owned(),
    })
}

fn twitch(event: &InputEvent, config: &Config) -> Classification {
    let trigger = &config.triggers.twitch;
    let Some(title) = event.string("reward_title") else {
        return Classification::Ignore;
    };
    if !trigger.enabled
        || !title
            .trim()
            .eq_ignore_ascii_case(trigger.reward_title.trim())
    {
        return Classification::Ignore;
    }
    event
        .string("input")
        .map_or(Classification::Ignore, |text| speak(event, text))
}

fn youtube_donation(event: &InputEvent, config: &Config) -> Classification {
    let trigger = &config.triggers.youtube;
    if !trigger.donations_enabled
        || event
            .number("amount")
            .is_none_or(|amount| amount < trigger.minimum_amount)
    {
        return Classification::Ignore;
    }
    event
        .string("message")
        .map_or(Classification::Ignore, |text| speak(event, text))
}

fn youtube_member(event: &InputEvent, config: &Config) -> Classification {
    if !config.triggers.youtube.members_enabled || !event.has_role("subscriber") {
        return Classification::Ignore;
    }
    event
        .string("message")
        .map_or(Classification::Ignore, |text| speak(event, text))
}

fn kick(event: &InputEvent, config: &Config) -> Classification {
    let trigger = &config.triggers.kick;
    if !trigger.enabled
        || !event.user.roles.iter().any(|role| {
            trigger
                .roles
                .iter()
                .any(|allowed| role.eq_ignore_ascii_case(allowed))
        })
    {
        return Classification::Ignore;
    }
    let mut parts = event
        .string("message")
        .unwrap_or_default()
        .splitn(2, char::is_whitespace);
    if !parts
        .next()
        .is_some_and(|cmd| cmd.eq_ignore_ascii_case(&trigger.command))
    {
        return Classification::Ignore;
    }
    match parts
        .next()
        .map(str::trim_start)
        .filter(|text| !text.is_empty())
    {
        Some(text) => speak(event, text),
        None => Classification::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::User;

    fn event(platform: &str, kind: &str, roles: &[&str], data: serde_json::Value) -> InputEvent {
        InputEvent {
            id: "event-1".into(),
            platform: platform.into(),
            kind: kind.into(),
            user: User {
                id: "user-1".into(),
                name: "Ana".into(),
                roles: roles.iter().map(|role| (*role).into()).collect(),
            },
            data,
            ts: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn admin_precedes_kick_trigger_and_subscriber_cannot_administer() {
        let admin = event("kick", "chat", &["moderator"], json!({"message":"!skip"}));
        assert_eq!(
            classify(&admin, &Config::default()),
            Classification::Admin(AdminCommand::Skip)
        );
        let subscriber = event("kick", "chat", &["subscriber"], json!({"message":"!skip"}));
        assert_eq!(
            classify(&subscriber, &Config::default()),
            Classification::Ignore
        );
    }

    #[test]
    fn donation_requires_a_finite_number_not_a_string() {
        let string = event(
            "youtube",
            "donation",
            &[],
            json!({"amount":"2","message":"hola"}),
        );
        assert_eq!(
            classify(&string, &Config::default()),
            Classification::Ignore
        );
        let nan = event(
            "youtube",
            "donation",
            &[],
            json!({"amount":f64::INFINITY,"message":"hola"}),
        );
        assert_eq!(classify(&nan, &Config::default()), Classification::Ignore);
    }
}
