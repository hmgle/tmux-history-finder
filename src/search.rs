use anyhow::Result;
use regex::{Regex, RegexBuilder};

use crate::{
    config::Config,
    index::{Record, SearchIndex},
    types::{CaseMode, SearchMode},
};

pub fn filter_record_ids(
    index: &SearchIndex,
    query: Option<&str>,
    config: &Config,
) -> Result<Vec<usize>> {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Ok(index.records.iter().map(|record| record.id).collect());
    };

    match config.search_mode {
        SearchMode::Literal => Ok(filter_literal(index, query, config.case_mode)),
        SearchMode::Regex => filter_regex(index, query, config.case_mode),
    }
}

fn filter_literal(index: &SearchIndex, query: &str, case_mode: CaseMode) -> Vec<usize> {
    let sensitive = case_mode.is_sensitive_for(query);
    let needle = if sensitive {
        query.to_string()
    } else {
        query.to_ascii_lowercase()
    };

    index
        .records
        .iter()
        .filter(|record| literal_match(index, record, &needle, sensitive))
        .map(|record| record.id)
        .collect()
}

fn filter_regex(index: &SearchIndex, query: &str, case_mode: CaseMode) -> Result<Vec<usize>> {
    let regex = RegexBuilder::new(query)
        .case_insensitive(!case_mode.is_sensitive_for(query))
        .build()?;

    Ok(index
        .records
        .iter()
        .filter(|record| regex_match(index, record, &regex))
        .map(|record| record.id)
        .collect())
}

fn literal_match(index: &SearchIndex, record: &Record, needle: &str, sensitive: bool) -> bool {
    if field_contains(&record.location, needle, sensitive)
        || field_contains(&record.text, needle, sensitive)
    {
        return true;
    }

    index.pane_for(record).is_some_and(|pane| {
        field_contains(&pane.command, needle, sensitive)
            || field_contains(&pane.window_name, needle, sensitive)
    })
}

fn field_contains(haystack: &str, needle: &str, sensitive: bool) -> bool {
    if sensitive {
        haystack.contains(needle)
    } else {
        haystack.to_ascii_lowercase().contains(needle)
    }
}

fn regex_match(index: &SearchIndex, record: &Record, regex: &Regex) -> bool {
    if regex.is_match(&record.location) || regex.is_match(&record.text) {
        return true;
    }

    index
        .pane_for(record)
        .is_some_and(|pane| regex.is_match(&pane.command) || regex.is_match(&pane.window_name))
}

#[cfg(test)]
mod tests {
    use super::filter_record_ids;
    use crate::{
        config::Config,
        index::{PaneSnapshot, Record, SearchIndex},
        types::{ActionKind, CaseMode, Scope, SearchMode},
    };

    fn config(case_mode: CaseMode, search_mode: SearchMode) -> Config {
        Config {
            launch_key: "g".into(),
            scope: Scope::All,
            include_history: true,
            history_lines: None,
            case_mode,
            join_wraps: true,
            skip_blank: true,
            preview: true,
            prompt_query: false,
            default_action: ActionKind::Jump,
            fzf_options: String::new(),
            search_mode,
        }
    }

    fn index() -> SearchIndex {
        SearchIndex {
            version: 1,
            panes: vec![PaneSnapshot {
                session: "s".into(),
                window_index: "1".into(),
                pane_index: "0".into(),
                pane_id: "%1".into(),
                command: "zsh".into(),
                window_name: "main".into(),
                history_start_line: 0,
                lines: vec!["Error: alpha".into()],
            }],
            records: vec![Record {
                id: 0,
                pane_index: 0,
                raw_line_no: 1,
                logical_line_no: 1,
                location: "s:1.0".into(),
                text: "Error: alpha".into(),
                before: None,
                after: None,
            }],
        }
    }

    #[test]
    fn literal_search_uses_smart_case() {
        assert_eq!(
            filter_record_ids(
                &index(),
                Some("error"),
                &config(CaseMode::Smart, SearchMode::Literal)
            )
            .unwrap(),
            vec![0]
        );
        assert!(filter_record_ids(
            &index(),
            Some("error"),
            &config(CaseMode::Sensitive, SearchMode::Literal)
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn regex_search_works() {
        assert_eq!(
            filter_record_ids(
                &index(),
                Some("Error: [a-z]+"),
                &config(CaseMode::Sensitive, SearchMode::Regex)
            )
            .unwrap(),
            vec![0]
        );
    }
}
