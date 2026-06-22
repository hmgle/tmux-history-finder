use anyhow::{Context, Result};

use crate::{
    config::Config,
    index::{LegacyRecord, Record, SearchIndex},
    tmux,
    types::ActionKind,
    util::trim_prefix_chars,
};

struct ActionTarget {
    pane_id: String,
    location: String,
    raw_line_no: usize,
    text: String,
}

pub fn execute(
    index: &SearchIndex,
    record_id: usize,
    action: ActionKind,
    _config: &Config,
) -> Result<()> {
    let record = index
        .record(record_id)
        .with_context(|| format!("record {record_id} not found"))?;
    let pane = index
        .pane_for(record)
        .context("pane not found for record")?;
    let history_line_no = history_line_no(pane.history_start_line, record.raw_line_no);
    perform(
        action,
        &ActionTarget {
            pane_id: pane.pane_id.clone(),
            location: record.location.clone(),
            raw_line_no: history_line_no,
            text: record.text.clone(),
        },
    )
}

pub fn execute_legacy(record: &LegacyRecord, action: ActionKind) -> Result<()> {
    perform(
        action,
        &ActionTarget {
            pane_id: record.pane_id.clone(),
            location: record.location.clone(),
            raw_line_no: record.line_no,
            text: record.text.clone(),
        },
    )
}

fn perform(action: ActionKind, target: &ActionTarget) -> Result<()> {
    match action {
        ActionKind::Jump => jump(target),
        ActionKind::Copy => copy_text(&target.text),
        ActionKind::Send => {
            tmux::run(["send-keys", "-l", "--", target.text.as_str()])?;
            Ok(())
        }
        ActionKind::Print => {
            println!("{}", target.text);
            Ok(())
        }
    }
}

fn jump(target: &ActionTarget) -> Result<()> {
    let (session, window) = split_location(&target.location);
    if !session.is_empty() {
        tmux::run_ignore(["switch-client", "-t", session.as_str()]);
    }
    if !session.is_empty() && !window.is_empty() {
        let window_target = format!("{session}:{window}");
        tmux::run_ignore(["select-window", "-t", window_target.as_str()]);
    }
    tmux::run_ignore(["select-pane", "-t", target.pane_id.as_str()]);
    tmux::run_ignore(["copy-mode", "-t", target.pane_id.as_str()]);

    let approx = target.raw_line_no.saturating_sub(5).max(1).to_string();
    tmux::run_ignore([
        "send-keys",
        "-t",
        target.pane_id.as_str(),
        "-X",
        "goto-line",
        approx.as_str(),
    ]);

    let needle = trim_prefix_chars(&target.text, 80);
    if needle.is_empty() {
        let raw = target.raw_line_no.to_string();
        tmux::run_ignore([
            "send-keys",
            "-t",
            target.pane_id.as_str(),
            "-X",
            "goto-line",
            raw.as_str(),
        ]);
    } else {
        let escaped = regex::escape(&needle);
        tmux::run_ignore([
            "send-keys",
            "-t",
            target.pane_id.as_str(),
            "-X",
            "search-forward",
            escaped.as_str(),
        ]);
    }

    Ok(())
}

fn history_line_no(history_start_line: usize, raw_line_no: usize) -> usize {
    history_start_line + raw_line_no
}

fn split_location(location: &str) -> (String, String) {
    let Some((session, rest)) = location.split_once(':') else {
        return (String::new(), String::new());
    };
    let window = rest.split('.').next().unwrap_or_default();
    (session.to_string(), window.to_string())
}

fn copy_text(text: &str) -> Result<()> {
    tmux::run_ignore(["set-buffer", "--", text]);

    let mut tried_clipboard = false;
    for (program, args) in clipboard_commands() {
        tried_clipboard = true;
        if tmux::write_to_command(program, &args, text).is_ok() {
            return Ok(());
        }
    }

    let reason = if tried_clipboard {
        "system clipboard helper failed"
    } else {
        "no system clipboard found"
    };
    tmux::display_message(&format!(
        "tmux-history-finder: copied to tmux buffer ({reason})"
    ));
    Ok(())
}

fn clipboard_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    [
        ("pbcopy", vec![]),
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
        ("clip.exe", vec![]),
    ]
    .into_iter()
    .filter(|(program, _)| tmux::have(program))
    .collect()
}

#[allow(dead_code)]
fn _record_target(index: &SearchIndex, record: &Record) -> Option<ActionTarget> {
    let pane = index.pane_for(record)?;
    Some(ActionTarget {
        pane_id: pane.pane_id.clone(),
        location: record.location.clone(),
        raw_line_no: record.raw_line_no,
        text: record.text.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::history_line_no;

    #[test]
    fn unbounded_capture_uses_snapshot_line_number() {
        assert_eq!(history_line_no(0, 42), 42);
    }

    #[test]
    fn limited_capture_restores_history_offset() {
        assert_eq!(history_line_no(1200, 25), 1225);
    }
}
