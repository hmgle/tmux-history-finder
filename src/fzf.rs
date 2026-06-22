use std::{
    ffi::OsString,
    io::{ErrorKind, Write},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};

use crate::{config::Config, index::SearchIndex, tmux, types::ActionKind, util::shell_quote};

pub struct PickerResult {
    pub action: ActionKind,
    pub record_ids: Vec<usize>,
}

pub fn run_picker(
    index: &SearchIndex,
    record_ids: &[usize],
    config: &Config,
    query: Option<&str>,
    index_path: &Path,
) -> Result<PickerResult> {
    let mut command = picker_command();
    let args = picker_args(config, query, index_path)?;
    command.args(args);
    command.stdin(Stdio::piped()).stdout(Stdio::piped());

    let mut child = command.spawn().context("failed to start fzf")?;
    {
        let stdin = child.stdin.as_mut().context("failed to open fzf stdin")?;
        for record_id in record_ids {
            if let Some(line) = display_line(index, *record_id) {
                if let Err(err) = writeln!(stdin, "{line}") {
                    if err.kind() == ErrorKind::BrokenPipe {
                        break;
                    }
                    return Err(err.into());
                }
            }
        }
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Ok(PickerResult {
            action: config.default_action,
            record_ids: Vec::new(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let key = lines.next().unwrap_or_default();
    let action = match key {
        "ctrl-y" => ActionKind::Copy,
        "ctrl-s" => ActionKind::Send,
        "ctrl-p" => ActionKind::Print,
        _ => config.default_action,
    };

    let record_ids = lines
        .filter_map(|line| line.split('\t').next())
        .filter_map(|id| id.parse::<usize>().ok())
        .collect();

    Ok(PickerResult { action, record_ids })
}

fn picker_command() -> Command {
    if tmux::have("fzf-tmux") && std::env::var_os("TMUX").is_some() {
        let mut cmd = Command::new("fzf-tmux");
        if supports_popup() {
            cmd.args(["-p", "80%,60%"]);
        }
        cmd
    } else {
        Command::new("fzf")
    }
}

fn picker_args(config: &Config, query: Option<&str>, index_path: &Path) -> Result<Vec<OsString>> {
    let mut args = vec![
        "--delimiter".into(),
        "\t".into(),
        "--with-nth".into(),
        "2,3,4,5,6".into(),
        "--ansi".into(),
        "--layout=reverse".into(),
        "--info=inline".into(),
        "--prompt".into(),
        "history> ".into(),
        "--multi".into(),
        "--tiebreak=index".into(),
        "--expect=ctrl-y,ctrl-s,ctrl-p".into(),
        "--header".into(),
        format!(
            "TAB multi-select | Enter={} | Ctrl-Y copy | Ctrl-S send | Ctrl-P print | ESC cancel | scope={}",
            config.default_action.to_string().to_ascii_uppercase(),
            config.scope
        )
        .into(),
    ];

    if config.preview {
        let exe = std::env::current_exe().context("failed to resolve current executable")?;
        let preview = format!(
            "{} preview --index {} --record-id {{1}}{}",
            shell_quote(&exe.to_string_lossy()),
            shell_quote(&index_path.to_string_lossy()),
            query
                .map(|value| format!(" --query {}", shell_quote(value)))
                .unwrap_or_default()
        );
        args.extend([
            "--preview-window".into(),
            "right:60%:wrap".into(),
            "--preview".into(),
            preview.into(),
        ]);
    }

    if let Some(query) = query.filter(|value| !value.is_empty()) {
        args.extend(["--query".into(), query.into()]);
    }

    if !config.fzf_options.trim().is_empty() {
        let extra = shell_words::split(&config.fzf_options).unwrap_or_else(|_| {
            config
                .fzf_options
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect()
        });
        args.extend(extra.into_iter().map(OsString::from));
    }

    Ok(args)
}

fn display_line(index: &SearchIndex, record_id: usize) -> Option<String> {
    let record = index.record(record_id)?;
    let pane = index.pane_for(record)?;
    let text = record.text.replace('\t', "    ");
    Some(format!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        record.id, record.location, pane.command, pane.window_name, record.raw_line_no, text
    ))
}

fn supports_popup() -> bool {
    let tmux_version = tmux::command_version("tmux", &["-V"]).unwrap_or_default();
    let fzf_version = tmux::command_version("fzf", &["--version"]).unwrap_or_default();
    version_at_least(&tmux_version, 3, 2) && version_at_least(&fzf_version, 0, 23)
}

fn version_at_least(value: &str, major: u64, minor: u64) -> bool {
    let Some((found_major, found_minor)) = first_major_minor(value) else {
        return false;
    };
    (found_major, found_minor) >= (major, minor)
}

fn first_major_minor(value: &str) -> Option<(u64, u64)> {
    let mut digits = value
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .filter(|part| !part.is_empty());
    if let Some(part) = digits.by_ref().next() {
        let mut pieces = part.split('.');
        let major = pieces.next()?.parse().ok()?;
        let minor = pieces.next().unwrap_or("0").parse().ok()?;
        return Some((major, minor));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::version_at_least;

    #[test]
    fn parses_versions() {
        assert!(version_at_least("tmux 3.3a", 3, 2));
        assert!(version_at_least("0.60.0 (abc)", 0, 23));
        assert!(!version_at_least("tmux 3.1", 3, 2));
    }
}
