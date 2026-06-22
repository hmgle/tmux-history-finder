use anyhow::Result;
use rayon::prelude::*;

use crate::{
    config::Config,
    index::{PaneSnapshot, Record, SearchIndex},
    tmux,
    types::Scope,
};

#[derive(Clone, Debug)]
struct PaneInfo {
    session: String,
    window_index: String,
    pane_index: String,
    pane_id: String,
    command: String,
    window_name: String,
    history_size: usize,
}

pub fn build_index(config: &Config, target_pane: Option<&str>) -> Result<SearchIndex> {
    let panes = list_panes(config.scope, target_pane)?;
    let snapshots: Vec<PaneSnapshot> = panes
        .par_iter()
        .filter_map(|pane| capture_pane(pane, config).ok())
        .collect();

    let mut records = Vec::new();
    for (pane_index, pane) in snapshots.iter().enumerate() {
        let mut logical_line_no = 0usize;
        for (line_index, line) in pane.lines.iter().enumerate() {
            if config.skip_blank && line.trim().is_empty() {
                continue;
            }

            logical_line_no += 1;
            let text = line.trim_end_matches('\r').to_string();
            let before = line_index
                .checked_sub(1)
                .and_then(|idx| pane.lines.get(idx))
                .cloned();
            let after = pane.lines.get(line_index + 1).cloned();
            records.push(Record {
                id: records.len(),
                pane_index,
                raw_line_no: line_index + 1,
                logical_line_no,
                location: pane.location(),
                text,
                before,
                after,
            });
        }
    }

    Ok(SearchIndex {
        version: 1,
        panes: snapshots,
        records,
    })
}

pub fn legacy_tsv(config: &Config, target_pane: Option<&str>) -> Result<String> {
    let panes = list_panes(config.scope, target_pane)?;
    let chunks: Vec<String> = panes
        .par_iter()
        .filter_map(|pane| capture_pane_legacy_tsv(pane, config).ok())
        .collect();

    Ok(chunks.concat())
}

fn list_panes(scope: Scope, target_pane: Option<&str>) -> Result<Vec<PaneInfo>> {
    let fmt = "#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_id}\t#{pane_current_command}\t#{history_size}\t#{window_name}";
    let output = match scope {
        Scope::All => tmux::stdout(["list-panes", "-a", "-F", fmt])?,
        Scope::Session => tmux::stdout(["list-panes", "-s", "-F", fmt])?,
        Scope::Pane => {
            let pane = target_pane
                .map(ToOwned::to_owned)
                .or_else(|| std::env::var("TMUX_PANE").ok())
                .or_else(|| tmux::try_stdout(["display-message", "-p", "#{pane_id}"]));
            let Some(pane) = pane else {
                return Ok(Vec::new());
            };
            let all = tmux::stdout(["list-panes", "-a", "-F", fmt])?;
            return Ok(parse_panes(&all)
                .into_iter()
                .filter(|info| info.pane_id == pane)
                .collect());
        }
    };

    let panes = parse_panes(&output)
        .into_iter()
        .filter(|info| target_pane.is_none_or(|target| info.pane_id == target))
        .collect();
    Ok(panes)
}

fn parse_panes(output: &str) -> Vec<PaneInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(7, '\t');
            Some(PaneInfo {
                session: parts.next()?.to_string(),
                window_index: parts.next()?.to_string(),
                pane_index: parts.next()?.to_string(),
                pane_id: parts.next()?.to_string(),
                command: parts.next().unwrap_or_default().to_string(),
                history_size: parts.next().unwrap_or_default().parse().unwrap_or_default(),
                window_name: parts.next().unwrap_or_default().to_string(),
            })
        })
        .filter(|pane| !pane.pane_id.is_empty())
        .collect()
}

fn capture_pane_legacy_tsv(pane: &PaneInfo, config: &Config) -> Result<String> {
    let output = capture_pane_output(pane, config)?;
    let location = pane_location(pane);
    let line_offset = history_start_line(pane, config);
    let mut logical_line_no = 0usize;
    let mut tsv = String::new();

    for line in output.lines() {
        if config.skip_blank && line.trim().is_empty() {
            continue;
        }

        logical_line_no += 1;
        tsv.push_str(&pane.pane_id);
        tsv.push('\t');
        tsv.push_str(&location);
        tsv.push('\t');
        tsv.push_str(&pane.command);
        tsv.push('\t');
        tsv.push_str(&pane.window_name);
        tsv.push('\t');
        tsv.push_str(&(line_offset + logical_line_no).to_string());
        tsv.push('\t');
        tsv.push_str(line.trim_end_matches('\r'));
        tsv.push('\n');
    }

    Ok(tsv)
}

fn capture_pane(pane: &PaneInfo, config: &Config) -> Result<PaneSnapshot> {
    let output = capture_pane_output(pane, config)?;
    let lines = output.lines().map(ToOwned::to_owned).collect();

    Ok(PaneSnapshot {
        session: pane.session.clone(),
        window_index: pane.window_index.clone(),
        pane_index: pane.pane_index.clone(),
        pane_id: pane.pane_id.clone(),
        command: pane.command.clone(),
        window_name: pane.window_name.clone(),
        history_start_line: history_start_line(pane, config),
        lines,
    })
}

fn capture_pane_output(pane: &PaneInfo, config: &Config) -> Result<String> {
    let mut args = vec!["capture-pane".to_string(), "-p".to_string()];
    if config.join_wraps {
        args.push("-J".to_string());
    }
    if config.include_history {
        args.extend([
            "-S".to_string(),
            history_start(config),
            "-E".to_string(),
            "-".to_string(),
        ]);
    }
    args.extend(["-t".to_string(), pane.pane_id.clone()]);

    tmux::stdout(args)
}

fn pane_location(pane: &PaneInfo) -> String {
    format!("{}:{}.{}", pane.session, pane.window_index, pane.pane_index)
}

fn history_start(config: &Config) -> String {
    config
        .history_lines
        .map(|lines| format!("-{lines}"))
        .unwrap_or_else(|| "-".to_string())
}

fn history_start_line(pane: &PaneInfo, config: &Config) -> usize {
    config
        .history_lines
        .map(|lines| pane.history_size.saturating_sub(lines))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{history_start, history_start_line, PaneInfo};
    use crate::{
        config::Config,
        types::{ActionKind, CaseMode, Scope, SearchMode},
    };

    fn config(history_lines: Option<usize>) -> Config {
        Config {
            launch_key: "g".into(),
            scope: Scope::All,
            include_history: true,
            history_lines,
            case_mode: CaseMode::Smart,
            join_wraps: true,
            skip_blank: true,
            preview: true,
            prompt_query: false,
            default_action: ActionKind::Jump,
            fzf_options: String::new(),
            search_mode: SearchMode::Literal,
        }
    }

    fn pane(history_size: usize) -> PaneInfo {
        PaneInfo {
            session: "s".into(),
            window_index: "1".into(),
            pane_index: "0".into(),
            pane_id: "%1".into(),
            command: "zsh".into(),
            window_name: "main".into(),
            history_size,
        }
    }

    #[test]
    fn unlimited_history_starts_at_top() {
        assert_eq!(history_start(&config(None)), "-");
    }

    #[test]
    fn limited_history_uses_negative_line_count() {
        assert_eq!(history_start(&config(Some(5000))), "-5000");
    }

    #[test]
    fn unlimited_history_has_no_line_offset() {
        assert_eq!(history_start_line(&pane(10_000), &config(None)), 0);
    }

    #[test]
    fn limited_history_tracks_omitted_scrollback_lines() {
        assert_eq!(history_start_line(&pane(10_000), &config(Some(5000))), 5000);
    }

    #[test]
    fn limited_history_offset_saturates_for_short_history() {
        assert_eq!(history_start_line(&pane(100), &config(Some(5000))), 0);
    }
}
