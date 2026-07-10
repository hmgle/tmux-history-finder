use unicode_segmentation::UnicodeSegmentation;

use super::{rendering::grapheme_width_at, Match, Pane, TabMode};
use crate::types::CaseMode;

pub(super) fn find_matches(
    panes: &[Pane],
    pattern: &str,
    case_mode: CaseMode,
    smartsign: bool,
    tab_mode: TabMode,
) -> Vec<Match> {
    let patterns = smartsign_patterns(pattern, smartsign);
    let sensitive = case_mode.is_sensitive_for(pattern);
    let pattern_graphemes: Vec<Vec<String>> = patterns
        .iter()
        .map(|candidate| {
            candidate
                .graphemes(true)
                .map(|grapheme| {
                    if sensitive {
                        grapheme.to_string()
                    } else {
                        grapheme.to_lowercase()
                    }
                })
                .collect()
        })
        .collect();
    let pattern_len = pattern_graphemes.first().map(Vec::len).unwrap_or_default();
    if pattern_len == 0 {
        return Vec::new();
    }
    let mut matches = Vec::new();

    for (pane_index, pane) in panes.iter().enumerate() {
        for (line_no, line) in pane.lines.iter().enumerate() {
            let mut entries = Vec::<(usize, String)>::new();
            let mut visual_col = 0;
            for grapheme in line.graphemes(true) {
                entries.push((
                    visual_col,
                    if sensitive {
                        grapheme.to_string()
                    } else {
                        grapheme.to_lowercase()
                    },
                ));
                visual_col += grapheme_width_at(grapheme, visual_col, tab_mode);
            }
            if entries.len() < pattern_len {
                continue;
            }
            for start in 0..=entries.len() - pattern_len {
                if pattern_graphemes.iter().any(|candidate| {
                    candidate
                        .iter()
                        .zip(&entries[start..start + pattern_len])
                        .all(|(expected, (_, actual))| expected == actual)
                }) {
                    matches.push(Match {
                        pane_index,
                        line_no,
                        visual_col: entries[start].0,
                    });
                }
            }
        }
    }

    matches
}

pub(super) fn smartsign_patterns(pattern: &str, enabled: bool) -> Vec<String> {
    if !enabled {
        return vec![pattern.to_string()];
    }

    let options: Vec<Vec<char>> = pattern
        .chars()
        .map(|ch| {
            let mut chars = vec![ch];
            if let Some(mapped) = smartsign_char(ch) {
                chars.push(mapped);
            }
            chars
        })
        .collect();
    let mut patterns = vec![String::new()];
    for chars in options {
        let mut next = Vec::new();
        for prefix in &patterns {
            for ch in &chars {
                let mut pattern = prefix.clone();
                pattern.push(*ch);
                next.push(pattern);
            }
        }
        patterns = next;
    }
    patterns
}

fn smartsign_char(ch: char) -> Option<char> {
    match ch {
        '1' => Some('!'),
        '2' => Some('@'),
        '3' => Some('#'),
        '4' => Some('$'),
        '5' => Some('%'),
        '6' => Some('^'),
        '7' => Some('&'),
        '8' => Some('*'),
        '9' => Some('('),
        '0' => Some(')'),
        '-' => Some('_'),
        '=' => Some('+'),
        '[' => Some('{'),
        ']' => Some('}'),
        '\\' => Some('|'),
        ';' => Some(':'),
        '\'' => Some('"'),
        '`' => Some('~'),
        ',' => Some('<'),
        '.' => Some('>'),
        '/' => Some('?'),
        _ => None,
    }
}
