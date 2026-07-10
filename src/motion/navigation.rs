use anyhow::{Context, Result};

use super::{rendering::true_position, Match, Pane, TabMode};
use crate::tmux;

pub(super) fn move_to_match(
    panes: &[Pane],
    target: &Match,
    tab_mode: TabMode,
    target_client: Option<&str>,
) -> Result<()> {
    let pane = panes
        .get(target.pane_index)
        .context("motion target pane not found")?;
    let line = pane
        .lines
        .get(target.line_no)
        .context("motion target line not found")?;
    let true_col = true_position(line, target.visual_col, tab_mode);
    move_cursor(pane, target.line_no, true_col, target_client, tab_mode)
}

fn move_cursor(
    pane: &Pane,
    line_no: usize,
    true_col: usize,
    target_client: Option<&str>,
    tab_mode: TabMode,
) -> Result<()> {
    let client = tmux::TmuxClient::from_env()?;
    move_cursor_with_client(&client, pane, line_no, true_col, target_client, tab_mode)
}

pub(super) fn move_cursor_with_client(
    client: &tmux::TmuxClient,
    pane: &Pane,
    line_no: usize,
    true_col: usize,
    target_client: Option<&str>,
    tab_mode: TabMode,
) -> Result<()> {
    if let Some(target_client) = target_client {
        client.run([
            "switch-client",
            "-c",
            target_client,
            "-t",
            pane.window_id.as_str(),
        ])?;
    } else {
        client.run(["select-window", "-t", pane.window_id.as_str()])?;
    }
    client.run(["select-pane", "-t", pane.pane_id.as_str()])?;
    if !pane.copy_mode {
        client.run(["copy-mode", "-t", pane.pane_id.as_str()])?;
    }
    send_copy_command(client, pane, &["top-line"])?;
    send_copy_command(client, pane, &["start-of-line"])?;

    let mut rows_remaining = line_no;
    if tab_mode == TabMode::PositionAware {
        let first_non_empty = pane
            .lines
            .iter()
            .position(|line| !line.is_empty())
            .unwrap_or_default();
        if first_non_empty > 0 && first_non_empty <= line_no {
            let first = first_non_empty.to_string();
            send_copy_command(client, pane, &["-N", first.as_str(), "cursor-down"])?;
            send_copy_command(client, pane, &["start-of-line"])?;
            rows_remaining -= first_non_empty;
        }
    }
    if rows_remaining > 0 {
        let rows = rows_remaining.to_string();
        send_copy_command(client, pane, &["-N", rows.as_str(), "cursor-down"])?;
    }
    if true_col > 0 {
        let col = true_col.to_string();
        send_copy_command(client, pane, &["-N", col.as_str(), "cursor-right"])?;
    }
    Ok(())
}

fn send_copy_command(client: &tmux::TmuxClient, pane: &Pane, args: &[&str]) -> Result<()> {
    let mut command = vec!["send-keys", "-X", "-t", pane.pane_id.as_str()];
    command.extend_from_slice(args);
    client.run(command)
}
