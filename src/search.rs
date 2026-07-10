use anyhow::Result;
use regex::RegexBuilder;

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
    let unicode_insensitive = (!sensitive && !query.is_ascii()).then(|| {
        RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()
            .expect("escaped literal regex is valid")
    });

    index
        .records
        .iter()
        .filter(|record| {
            literal_match(
                index,
                record,
                query,
                sensitive,
                unicode_insensitive.as_ref(),
            )
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
        .filter(|record| {
            searchable_text(index, record).is_some_and(|haystack| regex.is_match(&haystack))
        })
        .map(|record| record.id)
        .collect())
}

fn literal_match(
    index: &SearchIndex,
    record: &Record,
    needle: &str,
    sensitive: bool,
    unicode_insensitive: Option<&regex::Regex>,
) -> bool {
    if needle.contains('\t') {
        return searchable_text(index, record).is_some_and(|haystack| {
            field_contains(&haystack, needle, sensitive, unicode_insensitive)
        });
    }

    if field_contains(&record.location, needle, sensitive, unicode_insensitive)
        || field_contains(&record.text, needle, sensitive, unicode_insensitive)
    {
        return true;
    }

    index.pane_for(record).is_some_and(|pane| {
        field_contains(&pane.command, needle, sensitive, unicode_insensitive)
            || field_contains(&pane.window_name, needle, sensitive, unicode_insensitive)
    })
}

fn field_contains(
    haystack: &str,
    needle: &str,
    sensitive: bool,
    unicode_insensitive: Option<&regex::Regex>,
) -> bool {
    if sensitive {
        haystack.contains(needle)
    } else if let Some(regex) = unicode_insensitive {
        regex.is_match(haystack)
    } else {
        contains_ascii_case_insensitive(haystack, needle)
    }
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }

    haystack
        .windows(needle.len())
        .any(|window| ascii_eq_ignore_case(window, needle))
}

fn ascii_eq_ignore_case(left: &[u8], right: &[u8]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn searchable_text(index: &SearchIndex, record: &Record) -> Option<String> {
    let pane = index.pane_for(record)?;
    Some(format!(
        "{}\t{}\t{}\t{}",
        record.location, pane.command, pane.window_name, record.text
    ))
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
            motion_key: "s".into(),
            motion2_key: String::new(),
            motion_copy_mode_no_prefix: false,
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
            motion_hints: "asdghklqwertyuiopzxcvbnmfj;".into(),
            motion_case_mode: CaseMode::Insensitive,
            motion_smartsign: false,
            motion_vertical_border: "|".into(),
            motion_horizontal_border: "-".into(),
            motion_hint1_fg: "1;31".into(),
            motion_hint2_fg: "1;32".into(),
            motion_dim: "2".into(),
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

    fn index_with_pane_metadata() -> SearchIndex {
        SearchIndex {
            version: 1,
            panes: vec![PaneSnapshot {
                session: "s".into(),
                window_index: "1".into(),
                pane_index: "0".into(),
                pane_id: "%1".into(),
                command: "ZshShell".into(),
                window_name: "LogsWindow".into(),
                history_start_line: 0,
                lines: vec!["alpha".into()],
            }],
            records: vec![Record {
                id: 0,
                pane_index: 0,
                raw_line_no: 1,
                logical_line_no: 1,
                location: "s:1.0".into(),
                text: "alpha".into(),
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

    #[test]
    fn regex_search_can_match_across_search_fields() {
        assert_eq!(
            filter_record_ids(
                &index_with_pane_metadata(),
                Some("1\\.0\tZshShell\tLogsWindow\talpha"),
                &config(CaseMode::Sensitive, SearchMode::Regex)
            )
            .unwrap(),
            vec![0]
        );
    }

    #[test]
    fn literal_search_matches_pane_metadata_without_case() {
        assert_eq!(
            filter_record_ids(
                &index_with_pane_metadata(),
                Some("logswindow"),
                &config(CaseMode::Insensitive, SearchMode::Literal)
            )
            .unwrap(),
            vec![0]
        );
        assert_eq!(
            filter_record_ids(
                &index_with_pane_metadata(),
                Some("zshshell"),
                &config(CaseMode::Insensitive, SearchMode::Literal)
            )
            .unwrap(),
            vec![0]
        );
    }

    #[test]
    fn literal_search_can_match_explicit_field_separators() {
        assert_eq!(
            filter_record_ids(
                &index_with_pane_metadata(),
                Some("1.0\tzshshell"),
                &config(CaseMode::Insensitive, SearchMode::Literal)
            )
            .unwrap(),
            vec![0]
        );
    }

    #[test]
    fn literal_search_is_unicode_case_insensitive() {
        let mut index = index();
        index.records[0].text = "ÄPFEL".into();
        assert_eq!(
            filter_record_ids(
                &index,
                Some("äpfel"),
                &config(CaseMode::Insensitive, SearchMode::Literal)
            )
            .unwrap(),
            vec![0]
        );
    }
}
