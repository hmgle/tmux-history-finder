use anyhow::Result;
use regex::RegexBuilder;

use crate::{
    config::Config,
    index::SearchIndex,
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
        .filter(|record| {
            let haystack = searchable_text(index, record.id);
            if sensitive {
                haystack.contains(&needle)
            } else {
                haystack.to_ascii_lowercase().contains(&needle)
            }
        })
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
        .filter(|record| regex.is_match(&searchable_text(index, record.id)))
        .map(|record| record.id)
        .collect())
}

fn searchable_text(index: &SearchIndex, record_id: usize) -> String {
    let Some(record) = index.record(record_id) else {
        return String::new();
    };
    let Some(pane) = index.pane_for(record) else {
        return record.text.clone();
    };
    format!(
        "{}\t{}\t{}\t{}",
        record.location, pane.command, pane.window_name, record.text
    )
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
