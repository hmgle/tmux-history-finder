use std::{
    ffi::OsString,
    io::{ErrorKind, Write},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};

use crate::{
    config::Config,
    index::SearchIndex,
    tmux,
    types::ActionKind,
    util::{shell_quote, version_at_least},
};

pub struct PickerResult {
    pub action: ActionKind,
    pub record_ids: Vec<usize>,
}

pub fn run_picker(
    index: &SearchIndex,
    record_ids: &[usize],
    config: &Config,
    index_path: &Path,
) -> Result<PickerResult> {
    let mut command = picker_command();
    let args = picker_args(config, index_path)?;
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
    match picker_exit(output.status.code()) {
        PickerExit::Selected => {}
        PickerExit::Cancelled => {
            return Ok(PickerResult {
                action: config.default_action,
                record_ids: Vec::new(),
            });
        }
        PickerExit::Failed => {
            anyhow::bail!("fzf exited with status {}", output.status);
        }
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
        .map(|line| {
            let id = line
                .split('\t')
                .next()
                .context("fzf returned an empty row")?;
            let id = id
                .parse::<usize>()
                .with_context(|| format!("fzf returned invalid record id '{id}'"))?;
            index
                .record(id)
                .with_context(|| format!("fzf returned unknown record id {id}"))?;
            Ok(id)
        })
        .collect::<Result<Vec<_>>>()?;

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

fn picker_args(config: &Config, index_path: &Path) -> Result<Vec<OsString>> {
    let mut args = vec![
        "--delimiter".into(),
        "\t".into(),
        "--with-nth".into(),
        "4,5,6,7,8".into(),
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
            "{} preview --index {} --pane-index {{2}} --line-index {{3}}",
            shell_quote(&exe.to_string_lossy()),
            shell_quote(&index_path.to_string_lossy()),
        );
        args.extend([
            "--preview-window".into(),
            "right:60%:wrap".into(),
            "--preview".into(),
            preview.into(),
        ]);
    }

    if !config.fzf_options.trim().is_empty() {
        let extra = shell_words::split(&config.fzf_options)
            .context("failed to parse fzf_options/TNX_FZF_OPTIONS")?;
        args.extend(extra.into_iter().map(OsString::from));
    }

    Ok(args)
}

fn display_line(index: &SearchIndex, record_id: usize) -> Option<String> {
    let record = index.record(record_id)?;
    let pane = index.pane_for(record)?;
    let location = sanitize_field(pane.location());
    let command = sanitize_field(&pane.command);
    let window_name = sanitize_field(&pane.window_name);
    let text = sanitize_field(index.text_for(record)?);
    Some(format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        record_id,
        record.pane_index,
        record.line_index,
        location,
        command,
        window_name,
        record.raw_line_no(),
        text
    ))
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\t' || ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PickerExit {
    Selected,
    Cancelled,
    Failed,
}

fn picker_exit(code: Option<i32>) -> PickerExit {
    match code {
        Some(0) => PickerExit::Selected,
        Some(1) | Some(130) => PickerExit::Cancelled,
        _ => PickerExit::Failed,
    }
}

pub(crate) fn supports_popup() -> bool {
    let tmux_version = tmux::command_version("tmux", &["-V"]).unwrap_or_default();
    let fzf_version = tmux::command_version("fzf", &["--version"]).unwrap_or_default();
    version_at_least(&tmux_version, 3, 2, 0) && version_at_least(&fzf_version, 0, 23, 0)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{picker_args, picker_exit, sanitize_field, PickerExit};
    use crate::{
        config::Config,
        types::{ActionKind, CaseMode, Scope, SearchMode},
    };

    fn config(preview: bool) -> Config {
        Config {
            launch_key: "g".into(),
            motion_key: "s".into(),
            motion2_key: String::new(),
            motion_copy_mode_no_prefix: false,
            scope: Scope::All,
            include_history: true,
            history_lines: None,
            case_mode: CaseMode::Smart,
            join_wraps: true,
            skip_blank: true,
            preview,
            prompt_query: false,
            default_action: ActionKind::Jump,
            fzf_options: String::new(),
            search_mode: SearchMode::Literal,
            motion_hints: "asdf".into(),
            motion_case_mode: CaseMode::Insensitive,
            motion_smartsign: false,
            motion_vertical_border: "|".into(),
            motion_horizontal_border: "-".into(),
            motion_hint1_fg: "1;31".into(),
            motion_hint2_fg: "1;32".into(),
            motion_dim: "2".into(),
        }
    }

    #[test]
    fn classifies_picker_exit_codes() {
        assert_eq!(picker_exit(Some(0)), PickerExit::Selected);
        assert_eq!(picker_exit(Some(1)), PickerExit::Cancelled);
        assert_eq!(picker_exit(Some(130)), PickerExit::Cancelled);
        assert_eq!(picker_exit(Some(2)), PickerExit::Failed);
        assert_eq!(picker_exit(None), PickerExit::Failed);
    }

    #[test]
    fn sanitizes_fzf_display_fields() {
        assert_eq!(sanitize_field("a\tb\x1b[2J\nc"), "a b [2J c");
    }

    #[test]
    fn picker_uses_hidden_preview_coordinates_without_seeding_query() {
        let args = picker_args(&config(true), Path::new("/tmp/preview dir")).unwrap();
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(!args.iter().any(|arg| arg == "--query"));
        assert!(args.iter().any(|arg| arg == "4,5,6,7,8"));
        let preview = args
            .iter()
            .skip_while(|arg| *arg != "--preview")
            .nth(1)
            .unwrap();
        assert!(preview.contains("--pane-index {2}"));
        assert!(preview.contains("--line-index {3}"));
        assert!(preview.contains("'/tmp/preview dir'"));
    }
}
