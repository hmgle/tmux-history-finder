pub fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let is_safe = value
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b'@' | b'%' | b'+'));
    if is_safe {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub fn trim_prefix_chars(value: &str, max_chars: usize) -> String {
    value.trim_start().chars().take(max_chars).collect()
}

pub fn version_at_least(value: &str, major: u64, minor: u64, patch: u64) -> bool {
    let Some(version) = value
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .find(|part| part.bytes().any(|byte| byte.is_ascii_digit()))
    else {
        return false;
    };
    let mut pieces = version.split('.');
    let found = (
        pieces
            .next()
            .and_then(|part| part.parse().ok())
            .unwrap_or(0),
        pieces
            .next()
            .and_then(|part| part.parse().ok())
            .unwrap_or(0),
        pieces
            .next()
            .and_then(|part| part.parse().ok())
            .unwrap_or(0),
    );
    found >= (major, minor, patch)
}

#[cfg(test)]
mod tests {
    use super::{shell_quote, version_at_least};

    #[test]
    fn shell_quote_handles_spaces_and_quotes() {
        assert_eq!(shell_quote("/tmp/a"), "/tmp/a");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    fn compares_tool_versions() {
        assert!(version_at_least("tmux 3.3a", 3, 2, 0));
        assert!(version_at_least("0.24.4 (devel)", 0, 24, 4));
        assert!(version_at_least("fzf 0.60", 0, 60, 0));
        assert!(!version_at_least("tmux 3.1", 3, 2, 0));
    }
}
