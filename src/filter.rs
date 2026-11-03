use crate::config::Config;

pub fn sanitize(input: &str, config: &Config) -> Option<String> {
    let normalized = normalize_whitespace(input);
    let without_urls = if config.remove_urls {
        remove_urls(&normalized)
    } else {
        normalized
    };
    let replaced = replace_blocked(&without_urls, config);
    let limited = limit_repetitions(&replaced, config.max_repeated_chars);
    let truncated = limited
        .chars()
        .take(config.max_message_chars)
        .collect::<String>();
    let result = truncated.trim();
    (!result.is_empty()).then(|| result.to_owned())
}

fn normalize_whitespace(input: &str) -> String {
    input
        .chars()
        .filter_map(|character| {
            if character.is_whitespace() {
                Some(' ')
            } else if character.is_control() {
                None
            } else {
                Some(character)
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn remove_urls(input: &str) -> String {
    input
        .split(' ')
        .filter(|token| {
            !["http://", "https://", "www."]
                .iter()
                .any(|prefix| starts_with_ascii_case(token, prefix))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn starts_with_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

fn replace_blocked(input: &str, config: &Config) -> String {
    input
        .split(' ')
        .map(|token| replace_token(token, config))
        .collect::<Vec<_>>()
        .join(" ")
}

fn replace_token<'a>(token: &'a str, config: &Config) -> std::borrow::Cow<'a, str> {
    let Some((start, _)) = token.char_indices().find(|(_, c)| c.is_alphanumeric()) else {
        return token.into();
    };
    let Some((last, character)) = token
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
    else {
        return token.into();
    };
    let end = last + character.len_utf8();
    let word = &token[start..end];
    if !word.chars().all(char::is_alphanumeric)
        || !config.blocked_words.contains(&word.to_lowercase())
    {
        return token.into();
    }
    format!(
        "{}{}{}",
        &token[..start],
        config.blocked_replacement,
        &token[end..]
    )
    .into()
}

fn limit_repetitions(input: &str, maximum: usize) -> String {
    let mut output = String::with_capacity(input.len());
    let mut previous = None;
    let mut count = 0;
    for character in input.chars() {
        if previous == Some(character) {
            count += 1;
        } else {
            previous = Some(character);
            count = 1;
        }
        if count <= maximum {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace_and_removes_other_controls() {
        let input = "  hola\r\n\tmundo\u{7f}\u{85}\u{2003}  ";
        assert_eq!(
            sanitize(input, &Config::default()).as_deref(),
            Some("hola mundo")
        );
    }

    #[test]
    fn removes_only_supported_url_prefixes_case_insensitively() {
        let input = "HTTP://a https://b WWW.c example.com; https://d";
        assert_eq!(
            sanitize(input, &Config::default()).as_deref(),
            Some("example.com;")
        );
        let config = Config {
            remove_urls: false,
            ..Config::default()
        };
        assert_eq!(sanitize("HTTPS://a", &config).as_deref(), Some("HTTPS://a"));
    }

    #[test]
    fn replaces_complete_unicode_words_without_touching_substrings() {
        let config = Config {
            blocked_words: vec!["malo".into(), "ñandú".into()],
            blocked_replacement: "***".into(),
            ..Config::default()
        };
        let input = "(malo), maloso ÑANDÚ! malo-malo";
        assert_eq!(
            sanitize(input, &config).as_deref(),
            Some("(***), maloso ***! malo-malo")
        );
    }

    #[test]
    fn limits_runs_before_unicode_scalar_truncation() {
        let config = Config {
            max_repeated_chars: 2,
            max_message_chars: 5,
            ..Config::default()
        };
        assert_eq!(sanitize("aaaab😀😀😀", &config).as_deref(), Some("aab😀😀"));
    }

    #[test]
    fn filtering_can_empty_input_and_preserves_html_as_text() {
        let config = Config::default();
        assert_eq!(sanitize("\0\n\t", &config), None);
        assert_eq!(
            sanitize("<img src=x onerror=alert(1)>", &config).as_deref(),
            Some("<img src=x onerror=alert(1)>")
        );
    }
}
