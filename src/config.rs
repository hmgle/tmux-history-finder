use std::{env, str::FromStr};

use crate::{
    tmux,
    types::{ActionKind, CaseMode, Scope, SearchMode},
};

#[derive(Clone, Debug)]
pub struct Config {
    pub launch_key: String,
    pub scope: Scope,
    pub include_history: bool,
    pub history_lines: Option<usize>,
    pub case_mode: CaseMode,
    pub join_wraps: bool,
    pub skip_blank: bool,
    pub preview: bool,
    pub prompt_query: bool,
    pub default_action: ActionKind,
    pub fzf_options: String,
    pub search_mode: SearchMode,
}

#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    pub scope: Option<Scope>,
    pub include_history: Option<bool>,
    pub history_lines: Option<Option<usize>>,
    pub case_mode: Option<CaseMode>,
    pub join_wraps: Option<bool>,
    pub skip_blank: Option<bool>,
    pub preview: Option<bool>,
    pub default_action: Option<ActionKind>,
    pub search_mode: Option<SearchMode>,
}

impl Config {
    pub fn load(overrides: &ConfigOverrides) -> Self {
        let mut config = Self {
            launch_key: setting("launch_key", "THF_LAUNCH_KEY").unwrap_or_else(|| "g".into()),
            scope: parse_setting("scope", "THF_SCOPE").unwrap_or_default(),
            include_history: bool_setting("include_history", "THF_INCLUDE_HISTORY").unwrap_or(true),
            history_lines: usize_setting("history_lines", "THF_HISTORY_LINES").and_then(nonzero),
            case_mode: parse_setting("case", "THF_CASE").unwrap_or_default(),
            join_wraps: bool_setting("join_wraps", "THF_JOIN_WRAPS").unwrap_or(true),
            skip_blank: bool_setting("skip_blank", "THF_SKIP_BLANK").unwrap_or(true),
            preview: bool_setting("preview", "THF_PREVIEW").unwrap_or(true),
            prompt_query: bool_setting("prompt_query", "THF_PROMPT_QUERY").unwrap_or(false),
            default_action: parse_setting("default_action", "THF_DEFAULT_ACTION")
                .unwrap_or_default(),
            fzf_options: setting("fzf_options", "THF_FZF_OPTIONS").unwrap_or_default(),
            search_mode: SearchMode::Literal,
        };

        if let Some(scope) = overrides.scope {
            config.scope = scope;
        }
        if let Some(include_history) = overrides.include_history {
            config.include_history = include_history;
        }
        if let Some(history_lines) = overrides.history_lines {
            config.history_lines = history_lines;
        }
        if let Some(case_mode) = overrides.case_mode {
            config.case_mode = case_mode;
        }
        if let Some(join_wraps) = overrides.join_wraps {
            config.join_wraps = join_wraps;
        }
        if let Some(skip_blank) = overrides.skip_blank {
            config.skip_blank = skip_blank;
        }
        if let Some(preview) = overrides.preview {
            config.preview = preview;
        }
        if let Some(default_action) = overrides.default_action {
            config.default_action = default_action;
        }
        if let Some(search_mode) = overrides.search_mode {
            config.search_mode = search_mode;
        }

        config
    }
}

fn setting(option_name: &str, env_name: &str) -> Option<String> {
    env::var(env_name)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            env::var_os("THF_OPTIONS_IMPORTED")
                .is_none()
                .then(|| tmux::show_option(&format!("@tmux_history_finder_{option_name}")))
                .flatten()
        })
}

fn parse_setting<T>(option_name: &str, env_name: &str) -> Option<T>
where
    T: FromStr,
{
    setting(option_name, env_name).and_then(|value| value.parse().ok())
}

fn bool_setting(option_name: &str, env_name: &str) -> Option<bool> {
    setting(option_name, env_name).and_then(|value| match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    })
}

fn usize_setting(option_name: &str, env_name: &str) -> Option<usize> {
    parse_setting(option_name, env_name)
}

fn nonzero(value: usize) -> Option<usize> {
    (value > 0).then_some(value)
}
