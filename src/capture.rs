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

fn list_panes(scope: Scope, target_pane: Option<&str>) -> Result<Vec<PaneInfo>> {
    let fmt = "#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_id}\t#{pane_current_command}\t#{window_name}";
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
            let mut parts = line.splitn(6, '\t');
            Some(PaneInfo {
                session: parts.next()?.to_string(),
                window_index: parts.next()?.to_string(),
                pane_index: parts.next()?.to_string(),
                pane_id: parts.next()?.to_string(),
                command: parts.next().unwrap_or_default().to_string(),
                window_name: parts.next().unwrap_or_default().to_string(),
            })
        })
        .filter(|pane| !pane.pane_id.is_empty())
        .collect()
}

fn capture_pane(pane: &PaneInfo, config: &Config) -> Result<PaneSnapshot> {
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

    let output = tmux::stdout(args)?;
    let lines = output.lines().map(ToOwned::to_owned).collect();

    Ok(PaneSnapshot {
        session: pane.session.clone(),
        window_index: pane.window_index.clone(),
        pane_index: pane.pane_index.clone(),
        pane_id: pane.pane_id.clone(),
        command: pane.command.clone(),
        window_name: pane.window_name.clone(),
        lines,
    })
}

fn history_start(config: &Config) -> String {
    config
        .history_lines
        .map(|lines| format!("-{lines}"))
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::history_start;
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

    #[test]
    fn unlimited_history_starts_at_top() {
        assert_eq!(history_start(&config(None)), "-");
    }

    #[test]
    fn limited_history_uses_negative_line_count() {
        assert_eq!(history_start(&config(Some(5000))), "-5000");
    }
}
