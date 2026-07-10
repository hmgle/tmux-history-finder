use anyhow::{Context, Result};

use crate::{
    config::Config,
    index::{LegacyRecord, Record, SearchIndex},
    tmux,
    types::ActionKind,
    util::trim_prefix_chars,
};

struct ActionTarget<'a> {
    pane_id: &'a str,
    location: &'a str,
    raw_line_no: usize,
    text: &'a str,
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
    let text = index
        .text_for(record)
        .context("text not found for record")?;
    let history_line_no = history_line_no(pane.history_start_line, record.raw_line_no());
    perform(
        action,
        &ActionTarget {
            pane_id: &pane.pane_id,
            location: pane.location(),
            raw_line_no: history_line_no,
            text,
        },
    )
}

pub fn execute_legacy(record: &LegacyRecord, action: ActionKind) -> Result<()> {
    perform(
        action,
        &ActionTarget {
            pane_id: &record.pane_id,
            location: &record.location,
            raw_line_no: record.line_no,
            text: &record.text,
        },
    )
}

fn perform(action: ActionKind, target: &ActionTarget<'_>) -> Result<()> {
    match action {
        ActionKind::Jump => jump(target),
        ActionKind::Copy => copy_text(target.text),
        ActionKind::Send => {
            tmux::run(["send-keys", "-l", "--", target.text])?;
            Ok(())
        }
        ActionKind::Print => {
            println!("{}", target.text);
            Ok(())
        }
    }
}

fn jump(target: &ActionTarget<'_>) -> Result<()> {
    let (session, window) = split_location(target.location);
    if !session.is_empty() {
        tmux::run_ignore(["switch-client", "-t", session]);
    }
    if !session.is_empty() && !window.is_empty() {
        let window_target = format!("{session}:{window}");
        tmux::run(["select-window", "-t", window_target.as_str()])
            .context("failed to select target window")?;
    }
    tmux::run(["select-pane", "-t", target.pane_id]).context("failed to select target pane")?;
    tmux::run(["copy-mode", "-t", target.pane_id]).context("failed to enter copy mode")?;

    let approx = target.raw_line_no.saturating_sub(5).max(1).to_string();
    tmux::run([
        "send-keys",
        "-t",
        target.pane_id,
        "-X",
        "goto-line",
        approx.as_str(),
    ])
    .context("failed to position near selected history line")?;

    let needle = trim_prefix_chars(target.text, 80);
    if needle.is_empty() {
        let raw = target.raw_line_no.to_string();
        tmux::run([
            "send-keys",
            "-t",
            target.pane_id,
            "-X",
            "goto-line",
            raw.as_str(),
        ])
        .context("failed to jump to selected history line")?;
    } else {
        tmux::run([
            "send-keys",
            "-t",
            target.pane_id,
            "-X",
            "search-forward-text",
            needle.as_str(),
        ])
        .context("failed to search for selected history text")?;
    }

    Ok(())
}

fn history_line_no(history_start_line: usize, raw_line_no: usize) -> usize {
    history_start_line + raw_line_no
}

fn split_location(location: &str) -> (&str, &str) {
    let Some((session, rest)) = location.split_once(':') else {
        return ("", "");
    };
    let window = rest.split('.').next().unwrap_or_default();
    (session, window)
}

fn copy_text(text: &str) -> Result<()> {
    tmux::run(["set-buffer", "--", text]).context("failed to update tmux buffer")?;

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
fn _record_target<'a>(index: &'a SearchIndex, record: &'a Record) -> Option<ActionTarget<'a>> {
    let pane = index.pane_for(record)?;
    Some(ActionTarget {
        pane_id: &pane.pane_id,
        location: pane.location(),
        raw_line_no: record.raw_line_no(),
        text: index.text_for(record)?,
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
