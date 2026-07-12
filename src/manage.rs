use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fs::File,
    io::{ErrorKind, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    fzf, tmux,
    util::{shell_quote, version_at_least},
};

const CATEGORIES: &[(&str, &str)] = &[
    ("history", "Search pane history"),
    ("copy-mode", "Run a copy-mode command"),
    ("session", "Manage sessions"),
    ("window", "Manage windows"),
    ("pane", "Manage panes"),
    ("command", "Insert a tmux command"),
    ("keybinding", "Run a key binding"),
    ("clipboard", "Paste clipboard history"),
    ("process", "Inspect or signal processes"),
    ("menu", "Run a configured command"),
];

#[derive(Clone, Debug, Default, Args)]
pub struct ManageArgs {
    #[arg(value_name = "CATEGORY")]
    category: Option<String>,
    #[arg(value_name = "ACTION")]
    action: Option<String>,
}

#[derive(Clone, Debug, Args)]
pub struct PreviewArgs {
    #[arg(long)]
    kind: String,
    #[arg(long)]
    data: PathBuf,
    #[arg(long)]
    row: usize,
}

#[derive(Clone, Debug)]
struct ManagerConfig {
    key: String,
    order: Vec<String>,
    fzf_options: String,
    preview: bool,
    preview_follow: bool,
    confirm: bool,
    switch_current: bool,
    session_format: Option<String>,
    window_format: Option<String>,
    pane_format: Option<String>,
    window_filter: Option<String>,
    menu: Option<String>,
    menu_popup: bool,
    menu_popup_width: String,
    menu_popup_height: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Row {
    id: String,
    display: String,
}

#[derive(Clone, Debug)]
struct ProcessRow {
    row: Row,
    user: String,
    pid: u32,
}

pub fn run(args: ManageArgs) -> Result<()> {
    ensure_program("tmux", "tmux is required for manage")?;
    ensure_program("fzf", "fzf is required for manage")?;
    let config = ManagerConfig::load()?;
    let category = match args.category {
        Some(category) => category,
        None => select_category(&config)?,
    };
    if category.is_empty() {
        return Ok(());
    }
    match category.as_str() {
        "history" => run_history(),
        "copy-mode" => run_copy_mode(args.action.as_deref(), &config),
        "session" => run_session(args.action.as_deref(), &config),
        "window" => run_window(args.action.as_deref(), &config),
        "pane" => run_pane(args.action.as_deref(), &config),
        "command" => run_command(&config),
        "keybinding" => run_keybinding(&config),
        "clipboard" => run_clipboard(args.action.as_deref(), &config),
        "process" => run_process(args.action.as_deref(), &config),
        "menu" => run_menu(&config),
        other => anyhow::bail!("unknown manager category '{other}'"),
    }
}

pub fn preview(args: PreviewArgs) -> Result<()> {
    let rows: Vec<Row> = serde_json::from_reader(
        File::open(&args.data)
            .with_context(|| format!("failed to open preview data {}", args.data.display()))?,
    )?;
    let row = rows.get(args.row).context("preview row is out of range")?;
    match args.kind.as_str() {
        "session" => print_tmux(["capture-pane", "-ep", "-t", &format!("{}:", row.id)]),
        "window" | "pane" => print_tmux(["capture-pane", "-ep", "-t", row.id.as_str()]),
        "buffer" => print_tmux(["show-buffer", "-b", row.id.as_str()]),
        "copyq" => print_command("copyq", ["read", row.id.as_str()]),
        _ => Ok(()),
    }
}

impl ManagerConfig {
    fn load() -> Result<Self> {
        let options: HashMap<String, String> = tmux::show_options("@tmux_history_finder_manager_")?
            .into_iter()
            .filter_map(|(name, value)| {
                name.strip_prefix("@tmux_history_finder_manager_")
                    .map(|key| (key.to_string(), value))
            })
            .collect();
        let get = |name: &str, env_name: &str, legacy: &str| {
            env::var(env_name)
                .ok()
                .filter(|value| !value.is_empty())
                .or_else(|| options.get(name).cloned())
                .or_else(|| env::var(legacy).ok().filter(|value| !value.is_empty()))
        };
        let boolean = |name: &str, env_name: &str, legacy: &str, default: bool| -> Result<bool> {
            match get(name, env_name, legacy) {
                None => Ok(default),
                Some(value) => parse_bool(&value)
                    .with_context(|| format!("invalid manager setting {env_name}/manager_{name}")),
            }
        };
        let mut order: Vec<String> = get("order", "THF_MANAGER_ORDER", "TMUX_FZF_ORDER")
            .unwrap_or_else(|| {
                "history|copy-mode|session|window|pane|command|keybinding|clipboard|process".into()
            })
            .split('|')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        let menu = get("menu", "THF_MANAGER_MENU", "TMUX_FZF_MENU");
        let key = env::var("THF_MANAGER_KEY")
            .ok()
            .or_else(|| tmux::show_option_allow_empty("@tmux_history_finder_manager_key"))
            .or_else(|| env::var("TMUX_FZF_LAUNCH_KEY").ok())
            .unwrap_or_else(|| "F".into());
        if menu.is_some() && !order.iter().any(|item| item == "menu") {
            order.push("menu".into());
        }
        Ok(Self {
            key,
            order,
            fzf_options: get("fzf_options", "THF_MANAGER_FZF_OPTIONS", "TMUX_FZF_OPTIONS")
                .unwrap_or_else(|| "-p -w 62% -h 38%".into()),
            preview: boolean("preview", "THF_MANAGER_PREVIEW", "TMUX_FZF_PREVIEW", true)?,
            preview_follow: boolean(
                "preview_follow",
                "THF_MANAGER_PREVIEW_FOLLOW",
                "TMUX_FZF_PREVIEW_FOLLOW",
                true,
            )?,
            confirm: boolean("confirm", "THF_MANAGER_CONFIRM", "", true)?,
            switch_current: boolean(
                "switch_current",
                "THF_MANAGER_SWITCH_CURRENT",
                "TMUX_FZF_SWITCH_CURRENT",
                false,
            )?,
            session_format: get(
                "session_format",
                "THF_MANAGER_SESSION_FORMAT",
                "TMUX_FZF_SESSION_FORMAT",
            ),
            window_format: get(
                "window_format",
                "THF_MANAGER_WINDOW_FORMAT",
                "TMUX_FZF_WINDOW_FORMAT",
            ),
            pane_format: get(
                "pane_format",
                "THF_MANAGER_PANE_FORMAT",
                "TMUX_FZF_PANE_FORMAT",
            ),
            window_filter: get(
                "window_filter",
                "THF_MANAGER_WINDOW_FILTER",
                "TMUX_FZF_WINDOW_FILTER",
            ),
            menu,
            menu_popup: boolean(
                "menu_popup",
                "THF_MANAGER_MENU_POPUP",
                "TMUX_FZF_MENU_POPUP",
                false,
            )?,
            menu_popup_width: get(
                "menu_popup_width",
                "THF_MANAGER_MENU_POPUP_WIDTH",
                "TMUX_FZF_MENU_POPUP_WIDTH",
            )
            .unwrap_or_else(|| "50%".into()),
            menu_popup_height: get(
                "menu_popup_height",
                "THF_MANAGER_MENU_POPUP_HEIGHT",
                "TMUX_FZF_MENU_POPUP_HEIGHT",
            )
            .unwrap_or_else(|| "50%".into()),
        })
    }
}

fn select_category(config: &ManagerConfig) -> Result<String> {
    let in_copy_mode =
        tmux::try_stdout(["display-message", "-p", "#{pane_in_mode}"]).as_deref() == Some("1");
    let rows: Vec<Row> = config
        .order
        .iter()
        .filter(|category| category.as_str() != "copy-mode" || in_copy_mode)
        .filter(|category| category.as_str() != "menu" || config.menu.is_some())
        .filter_map(|category| {
            CATEGORIES
                .iter()
                .find(|(name, _)| name == category)
                .map(|(name, description)| Row::new(*name, format!("{name:<12} {description}")))
        })
        .collect();
    Ok(
        choose(&rows, config, "manage> ", "Select a feature", false, None)?
            .first()
            .map(|index| rows[*index].id.clone())
            .unwrap_or_default(),
    )
}

fn run_history() -> Result<()> {
    let status = Command::new(env::current_exe()?).arg("search").status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("history search exited with status {status}")
    }
}

fn run_session(action: Option<&str>, config: &ManagerConfig) -> Result<()> {
    let actions = ["switch", "new", "rename", "detach", "kill"];
    let action = resolve_action(action, &actions, config, "session action> ")?;
    if action.is_empty() {
        return Ok(());
    }
    if action == "new" {
        if let Some(name) = prompt_text(config, "new session> ", "Enter a new session name")? {
            let mut commands = vec![command(["new-session", "-d", "-s", name.as_str()])];
            commands.extend(client_switch_commands(&name));
            tmux::TmuxClient::from_env()?.run_commands(&commands)?;
        }
        return Ok(());
    }
    let mut rows = session_rows(config)?;
    let current = tmux::try_stdout(["display-message", "-p", "#{session_id}"]).unwrap_or_default();
    if action == "switch" && !config.switch_current {
        rows.retain(|row| row.id != current);
    }
    if action == "detach" {
        rows.retain(|row| session_attached(&row.id));
    }
    let multi = matches!(action.as_str(), "detach" | "kill");
    let selected = selected_rows(
        &rows,
        config,
        "session> ",
        if multi {
            "TAB selects multiple sessions"
        } else {
            "Select a session"
        },
        multi,
        Some("session"),
    )?;
    if selected.is_empty() {
        return Ok(());
    }
    let client = tmux::TmuxClient::from_env()?;
    match action.as_str() {
        "switch" => {
            let commands = client_switch_commands(&selected[0].id);
            if commands.is_empty() {
                Ok(())
            } else {
                client.run_commands(&commands)
            }
        }
        "rename" => {
            if let Some(name) = prompt_text(config, "rename session> ", "Enter a new name")? {
                client.run([
                    "rename-session",
                    "-t",
                    selected[0].id.as_str(),
                    name.as_str(),
                ])?;
            }
            Ok(())
        }
        "detach" => destructive_tmux(
            config,
            "Detach selected session client(s)?",
            selected
                .iter()
                .map(|row| command(["detach-client", "-s", row.id.as_str()]))
                .collect(),
        ),
        "kill" => destructive_tmux(
            config,
            "Kill selected session(s)?",
            selected
                .iter()
                .map(|row| command(["kill-session", "-t", row.id.as_str()]))
                .collect(),
        ),
        _ => unreachable!(),
    }
}

fn run_window(action: Option<&str>, config: &ManagerConfig) -> Result<()> {
    let actions = ["switch", "link", "move", "swap", "rename", "kill"];
    let action = resolve_action(action, &actions, config, "window action> ")?;
    if action.is_empty() {
        return Ok(());
    }
    let mut rows = window_rows(config)?;
    let current_window =
        tmux::try_stdout(["display-message", "-p", "#{window_id}"]).unwrap_or_default();
    let current_session =
        tmux::try_stdout(["display-message", "-p", "#{session_id}"]).unwrap_or_default();
    if matches!(action.as_str(), "link" | "move") {
        rows.retain(|row| {
            window_target_parts(&row.id)
                .map(|(session, _)| session != current_session)
                .unwrap_or(false)
        });
        let selected = selected_rows(
            &rows,
            config,
            "source window> ",
            "Select a source window",
            false,
            Some("window"),
        )?;
        if let Some(row) = selected.first() {
            let command_name = if action == "link" {
                "link-window"
            } else {
                "move-window"
            };
            tmux::run([
                command_name,
                "-a",
                "-s",
                row.id.as_str(),
                "-t",
                current_session.as_str(),
            ])?;
        }
        return Ok(());
    }
    if action == "switch" && !config.switch_current {
        let current_target = window_target(&current_session, &current_window);
        rows.retain(|row| row.id != current_target);
    }
    let selected = selected_rows(
        &rows,
        config,
        "window> ",
        if action == "kill" {
            "TAB selects multiple windows"
        } else {
            "Select a window"
        },
        action == "kill",
        Some("window"),
    )?;
    if selected.is_empty() {
        return Ok(());
    }
    let client = tmux::TmuxClient::from_env()?;
    match action.as_str() {
        "switch" => {
            let (session, _) = window_target_parts(&selected[0].id)?;
            let mut commands = client_switch_commands(session);
            commands.push(command(["select-window", "-t", selected[0].id.as_str()]));
            client.run_commands(&commands)
        }
        "rename" => {
            if let Some(name) = prompt_text(config, "rename window> ", "Enter a new name")? {
                client.run([
                    "rename-window",
                    "-t",
                    selected[0].id.as_str(),
                    name.as_str(),
                ])?;
            }
            Ok(())
        }
        "swap" => {
            let source = selected[0].id.clone();
            let (_, source_window) = window_target_parts(&source)?;
            drop(selected);
            rows.retain(|row| {
                window_target_parts(&row.id)
                    .map(|(_, window)| window != source_window)
                    .unwrap_or(false)
            });
            let other = selected_rows(
                &rows,
                config,
                "swap with> ",
                "Select another window",
                false,
                Some("window"),
            )?;
            if let Some(other) = other.first() {
                client.run([
                    "swap-window",
                    "-s",
                    source.as_str(),
                    "-t",
                    other.id.as_str(),
                ])?;
            }
            Ok(())
        }
        "kill" => destructive_tmux(
            config,
            "Unlink/kill selected window(s)?",
            selected
                .iter()
                .map(|row| command(["unlink-window", "-k", "-t", row.id.as_str()]))
                .collect(),
        ),
        _ => unreachable!(),
    }
}

fn run_pane(action: Option<&str>, config: &ManagerConfig) -> Result<()> {
    let actions = [
        "switch", "break", "join", "swap", "layout", "kill", "resize",
    ];
    let action = resolve_action(action, &actions, config, "pane action> ")?;
    if action.is_empty() {
        return Ok(());
    }
    if action == "layout" {
        let layouts = rows(&[
            "even-horizontal",
            "even-vertical",
            "main-horizontal",
            "main-vertical",
            "tiled",
        ]);
        if let Some(layout) =
            selected_rows(&layouts, config, "layout> ", "Select a layout", false, None)?.first()
        {
            tmux::run(["select-layout", layout.id.as_str()])?;
        }
        return Ok(());
    }
    if action == "resize" {
        return resize_pane(config);
    }
    let mut panes = pane_rows(config)?;
    let current = tmux::try_stdout(["display-message", "-p", "#{pane_id}"]).unwrap_or_default();
    if action == "join" || (action == "switch" && !config.switch_current) {
        panes.retain(|row| row.id != current);
    }
    let multi = matches!(action.as_str(), "join" | "kill");
    let selected = selected_rows(
        &panes,
        config,
        "pane> ",
        if multi {
            "TAB selects multiple panes"
        } else {
            "Select a pane"
        },
        multi,
        Some("pane"),
    )?;
    if selected.is_empty() {
        return Ok(());
    }
    let client = tmux::TmuxClient::from_env()?;
    match action.as_str() {
        "switch" => {
            let session = tmux::stdout([
                "display-message",
                "-p",
                "-t",
                selected[0].id.as_str(),
                "#{session_id}",
            ])?;
            let window = tmux::stdout([
                "display-message",
                "-p",
                "-t",
                selected[0].id.as_str(),
                "#{window_id}",
            ])?;
            let mut commands = client_switch_commands(session.trim());
            commands.push(command(["select-window", "-t", window.trim()]));
            commands.push(command(["select-pane", "-t", selected[0].id.as_str()]));
            client.run_commands(&commands)
        }
        "break" => {
            let session = tmux::stdout(["display-message", "-p", "#{session_id}"])?;
            client.run([
                "break-pane",
                "-d",
                "-s",
                selected[0].id.as_str(),
                "-t",
                &format!("{}:", session.trim()),
            ])
        }
        "join" => {
            let commands = selected
                .iter()
                .map(|row| command(["move-pane", "-s", row.id.as_str(), "-t", current.as_str()]))
                .collect::<Vec<_>>();
            client.run_commands(&commands)
        }
        "swap" => {
            let source = selected[0].id.clone();
            drop(selected);
            panes.retain(|row| row.id != source);
            let other = selected_rows(
                &panes,
                config,
                "swap with> ",
                "Select another pane",
                false,
                Some("pane"),
            )?;
            if let Some(other) = other.first() {
                client.run(["swap-pane", "-s", source.as_str(), "-t", other.id.as_str()])?;
            }
            Ok(())
        }
        "kill" => destructive_tmux(
            config,
            "Kill selected pane(s)?",
            selected
                .iter()
                .map(|row| command(["kill-pane", "-t", row.id.as_str()]))
                .collect(),
        ),
        _ => unreachable!(),
    }
}

fn resize_pane(config: &ManagerConfig) -> Result<()> {
    let directions = rows(&["left", "right", "up", "down"]);
    let Some(direction) = selected_rows(
        &directions,
        config,
        "resize> ",
        "Select a direction",
        false,
        None,
    )?
    .first()
    .cloned() else {
        return Ok(());
    };
    let sizes = if matches!(direction.id.as_str(), "left" | "right") {
        rows(&["1", "2", "3", "5", "10", "20", "30"])
    } else {
        rows(&["1", "2", "3", "5", "10", "15", "20"])
    };
    let Some(size) = selected_rows(
        &sizes,
        config,
        "size> ",
        "Select cells to adjust",
        false,
        None,
    )?
    .first()
    .cloned() else {
        return Ok(());
    };
    let flag = match direction.id.as_str() {
        "left" => "-L",
        "right" => "-R",
        "up" => "-U",
        _ => "-D",
    };
    tmux::run(["resize-pane", flag, size.id.as_str()])
}

fn run_copy_mode(action: Option<&str>, config: &ManagerConfig) -> Result<()> {
    const COMMANDS: &[(&str, &str, bool)] = &[
        ("append-selection", "Append selection to clipboard", false),
        (
            "append-selection-and-cancel",
            "Append selection and cancel",
            false,
        ),
        ("back-to-indentation", "Move to indentation", false),
        ("begin-selection", "Begin selection", false),
        ("bottom-line", "Move to bottom line", false),
        ("cancel", "Cancel copy mode", false),
        ("clear-selection", "Clear selection", false),
        ("copy-end-of-line", "Copy to end of line", false),
        (
            "copy-end-of-line-and-cancel",
            "Copy to end and cancel",
            false,
        ),
        (
            "copy-pipe-end-of-line",
            "Copy to end through a shell command",
            true,
        ),
        ("copy-line", "Copy line", false),
        ("copy-line-and-cancel", "Copy line and cancel", false),
        ("copy-selection", "Copy selection", false),
        (
            "copy-selection-and-cancel",
            "Copy selection and cancel",
            false,
        ),
        ("cursor-down", "Move cursor down", false),
        ("cursor-left", "Move cursor left", false),
        ("cursor-right", "Move cursor right", false),
        ("cursor-up", "Move cursor up", false),
        ("end-of-line", "Move to end of line", false),
        ("goto-line", "Go to line", true),
        ("history-bottom", "Scroll to history bottom", false),
        ("history-top", "Scroll to history top", false),
        ("jump-again", "Repeat last jump", false),
        ("jump-backward", "Jump backward to text", true),
        ("jump-forward", "Jump forward to text", true),
        ("jump-to-mark", "Jump to mark", false),
        ("middle-line", "Move to middle line", false),
        ("next-matching-bracket", "Next matching bracket", false),
        ("next-paragraph", "Next paragraph", false),
        ("next-word", "Next word", false),
        ("page-down", "Page down", false),
        ("page-up", "Page up", false),
        (
            "previous-matching-bracket",
            "Previous matching bracket",
            false,
        ),
        ("previous-paragraph", "Previous paragraph", false),
        ("previous-word", "Previous word", false),
        ("rectangle-toggle", "Toggle rectangle selection", false),
        ("refresh-from-pane", "Refresh from pane", false),
        ("search-again", "Repeat search", false),
        ("search-backward", "Search backward", true),
        ("search-forward", "Search forward", true),
        ("select-line", "Select line", false),
        ("select-word", "Select word", false),
        ("start-of-line", "Move to start of line", false),
        ("top-line", "Move to top line", false),
    ];
    let selected = if let Some(action) = action {
        COMMANDS.iter().find(|(name, _, _)| name == &action)
    } else {
        let choices: Vec<Row> = COMMANDS
            .iter()
            .map(|(name, description, _)| Row::new(*name, format!("{name:<34} {description}")))
            .collect();
        choose(
            &choices,
            config,
            "copy-mode> ",
            "Select a copy-mode command",
            false,
            None,
        )?
        .first()
        .and_then(|index| COMMANDS.get(*index))
    };
    let Some((name, _, requires_arg)) = selected else {
        return Ok(());
    };
    if *requires_arg {
        if let Some(value) = prompt_text(config, &format!("{name}> "), "Enter command argument")? {
            tmux::run(["send-keys", "-X", name, value.as_str()])?;
        }
    } else {
        tmux::run(["send-keys", "-X", name])?;
    }
    Ok(())
}

fn run_command(config: &ManagerConfig) -> Result<()> {
    let output = tmux::stdout(["list-commands"])?;
    let rows: Vec<Row> = output
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(|id| Row::new(id, line)))
        .collect();
    if let Some(row) =
        selected_rows(&rows, config, "command> ", "Select a command", false, None)?.first()
    {
        tmux::run(["command-prompt", "-I", row.id.as_str()])?;
    }
    Ok(())
}

fn run_keybinding(config: &ManagerConfig) -> Result<()> {
    let output = tmux::stdout(["list-keys"])?;
    let rows: Vec<Row> = output
        .lines()
        .enumerate()
        .map(|(index, line)| Row::new(index.to_string(), line))
        .collect();
    let selected = selected_rows(
        &rows,
        config,
        "binding> ",
        "Select a key binding",
        false,
        None,
    )?;
    let Some(row) = selected.first() else {
        return Ok(());
    };
    execute_binding(&row.display)
}

fn execute_binding(line: &str) -> Result<()> {
    let parts = shell_words::split(line).context("failed to parse tmux key binding")?;
    if parts.first().map(String::as_str) != Some("bind-key") {
        anyhow::bail!("unsupported key binding output: {line}");
    }
    let mut index = 1;
    let mut copy_mode = false;
    while index < parts.len() {
        match parts[index].as_str() {
            "-r" | "-n" => index += 1,
            "-T" | "-N" => {
                if parts[index] == "-T"
                    && parts
                        .get(index + 1)
                        .is_some_and(|value| value.starts_with("copy-mode"))
                {
                    copy_mode = true;
                }
                index += 2;
            }
            _ => break,
        }
    }
    index += 1;
    if index >= parts.len() {
        anyhow::bail!("binding has no command: {line}");
    }
    if copy_mode {
        tmux::run_ignore(["copy-mode"]);
    }
    tmux::run(parts[index..].iter().map(String::as_str))
}

fn run_clipboard(action: Option<&str>, config: &ManagerConfig) -> Result<()> {
    let mut use_copyq = action != Some("buffer") && tmux::have("copyq");
    if use_copyq && copyq_count().is_err() {
        let _ = Command::new("copyq")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        std::thread::sleep(std::time::Duration::from_millis(150));
        if copyq_count().is_err() {
            if action == Some("system") {
                anyhow::bail!("CopyQ is installed but its clipboard service is unavailable");
            }
            use_copyq = false;
        }
    }
    if use_copyq {
        let count = copyq_count()?;
        let mut rows = Vec::new();
        for index in 0..count {
            let id = index.to_string();
            let content = command_stdout("copyq", ["read", id.as_str()])?;
            rows.push(Row::new(&id, one_line(&content)));
        }
        let selected = selected_rows(
            &rows,
            config,
            "clipboard> ",
            "TAB selects multiple clipboard entries",
            true,
            Some("copyq"),
        )?;
        for row in selected {
            let content = command_stdout_bytes("copyq", ["read", row.id.as_str()])?;
            paste_bytes(&content)?;
        }
    } else {
        let output = tmux::stdout([
            "list-buffers",
            "-F",
            "#{buffer_name}\t#{buffer_size}\t#{buffer_sample}",
        ])?;
        let rows: Vec<Row> = output
            .lines()
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                Some(Row::new(
                    fields.next()?,
                    format!(
                        "{:>8} bytes  {}",
                        fields.next()?,
                        fields.next().unwrap_or_default()
                    ),
                ))
            })
            .collect();
        let selected = selected_rows(
            &rows,
            config,
            "buffer> ",
            "TAB selects multiple tmux buffers",
            true,
            Some("buffer"),
        )?;
        for row in selected {
            tmux::run(["paste-buffer", "-b", row.id.as_str()])?;
        }
    }
    Ok(())
}

fn copyq_count() -> Result<usize> {
    command_stdout("copyq", ["count"])?
        .trim()
        .parse()
        .context("CopyQ returned an invalid item count")
}

fn paste_bytes(content: &[u8]) -> Result<()> {
    let name = format!("thf-{}", std::process::id());
    let client = tmux::TmuxClient::from_env()?;
    client.run_with_input(["load-buffer", "-b", name.as_str(), "-"], content)?;
    let result = client.run(["paste-buffer", "-b", name.as_str()]);
    client.run_ignore(["delete-buffer", "-b", name.as_str()]);
    result
}

fn run_process(action: Option<&str>, config: &ManagerConfig) -> Result<()> {
    let mut actions = vec![
        "display",
        "terminate",
        "kill",
        "interrupt",
        "continue",
        "stop",
        "quit",
        "hangup",
    ];
    if tmux::have("pstree") {
        actions.insert(1, "tree");
    }
    let action = resolve_action(action, &actions, config, "process action> ")?;
    if action.is_empty() {
        return Ok(());
    }
    let processes = process_rows()?;
    let rows: Vec<Row> = processes
        .iter()
        .map(|process| process.row.clone())
        .collect();
    let multi = !matches!(action.as_str(), "display" | "tree");
    let indexes = choose(
        &rows,
        config,
        "process> ",
        "Select a process; TAB selects multiple signal targets",
        multi,
        None,
    )?;
    if indexes.is_empty() {
        return Ok(());
    }
    if action == "display" {
        return display_process(processes[indexes[0]].pid);
    }
    if action == "tree" {
        return popup_or_split(
            &format!("pstree -p {}", processes[indexes[0]].pid),
            "70%",
            "70%",
        );
    }
    let signal = match action.as_str() {
        "terminate" => "TERM",
        "kill" => "KILL",
        "interrupt" => "INT",
        "continue" => "CONT",
        "stop" => "STOP",
        "quit" => "QUIT",
        "hangup" => "HUP",
        _ => unreachable!(),
    };
    let selected: Vec<&ProcessRow> = indexes.iter().map(|index| &processes[*index]).collect();
    signal_processes(config, signal, &selected)
}

fn process_rows() -> Result<Vec<ProcessRow>> {
    let output = command_stdout("ps", ["-eo", "user=,pid=,ppid=,stat=,%cpu=,%mem=,command="])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let pid: u32 = fields.get(1)?.parse().ok()?;
            Some(ProcessRow {
                row: Row::new(pid.to_string(), line.trim()),
                user: fields[0].to_string(),
                pid,
            })
        })
        .collect())
}

fn signal_processes(config: &ManagerConfig, signal: &str, processes: &[&ProcessRow]) -> Result<()> {
    let current_user = command_stdout("whoami", std::iter::empty::<&str>())?
        .trim()
        .to_string();
    let privilege = ["sudo", "doas"]
        .into_iter()
        .find(|program| tmux::have(program));
    let mut commands: Vec<Vec<OsString>> = Vec::new();
    for process in processes {
        if process.user == current_user {
            commands.push(vec![
                "run-shell".into(),
                "-b".into(),
                format!("kill -s {signal} {}", process.pid).into(),
            ]);
        } else if let Some(program) = privilege {
            commands.push(vec![
                "split-window".into(),
                "-v".into(),
                "-l".into(),
                "30%".into(),
                "-b".into(),
                "-c".into(),
                "#{pane_current_path}".into(),
                format!("{program} kill -s {signal} {}", process.pid).into(),
            ]);
        } else {
            anyhow::bail!(
                "process {} belongs to {}; install sudo or doas to signal it",
                process.pid,
                process.user
            );
        }
    }
    destructive_tmux(
        config,
        &format!("Send {signal} to selected process(es)?"),
        commands,
    )
}

fn display_process(pid: u32) -> Result<()> {
    let command = if cfg!(target_os = "macos") {
        format!("top -pid {pid}")
    } else {
        format!("top -p {pid}")
    };
    popup_or_split(&command, "70%", "70%")
}

fn popup_or_split(command: &str, width: &str, height: &str) -> Result<()> {
    let version = tmux::command_version("tmux", &["-V"]).unwrap_or_default();
    if version_at_least(&version, 3, 2, 0) {
        tmux::run([
            "display-popup",
            "-E",
            "-w",
            width,
            "-h",
            height,
            "-d",
            "#{pane_current_path}",
            command,
        ])
    } else {
        tmux::run([
            "split-window",
            "-v",
            "-l",
            "50%",
            "-c",
            "#{pane_current_path}",
            command,
        ])
    }
}

fn run_menu(config: &ManagerConfig) -> Result<()> {
    let Some(raw) = config.menu.as_deref() else {
        tmux::display_message("tmux-history-finder: manager menu is not configured");
        return Ok(());
    };
    let entries = parse_menu(raw)?;
    let rows: Vec<Row> = entries
        .iter()
        .enumerate()
        .map(|(index, (label, _))| Row::new(index.to_string(), label))
        .collect();
    let selected = choose(
        &rows,
        config,
        "menu> ",
        "Select a configured command",
        false,
        None,
    )?;
    let Some(index) = selected.first() else {
        return Ok(());
    };
    let command = &entries[*index].1;
    if config.menu_popup {
        tmux::run([
            "display-popup",
            "-E",
            "-w",
            config.menu_popup_width.as_str(),
            "-h",
            config.menu_popup_height.as_str(),
            "-d",
            "#{pane_current_path}",
            command.as_str(),
        ])
    } else {
        tmux::run([
            "run-shell",
            "-b",
            "-c",
            "#{pane_current_path}",
            command.as_str(),
        ])
    }
}

fn parse_menu(raw: &str) -> Result<Vec<(String, String)>> {
    let normalized = raw.replace("\\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        while index < lines.len() && lines[index].is_empty() {
            index += 1;
        }
        if index >= lines.len() {
            break;
        }
        let label = lines[index].trim();
        index += 1;
        if label == "nil--" {
            break;
        }
        let command = lines
            .get(index)
            .context("manager menu entry is missing a command")?
            .trim();
        index += 1;
        if label.is_empty() || command.is_empty() {
            anyhow::bail!("manager menu labels and commands must not be empty");
        }
        if entries.iter().any(|(existing, _)| existing == label) {
            anyhow::bail!("duplicate manager menu label '{label}'");
        }
        entries.push((label.to_string(), command.to_string()));
    }
    Ok(entries)
}

fn session_rows(config: &ManagerConfig) -> Result<Vec<Row>> {
    let display = config
        .session_format
        .as_deref()
        .map(|format| format!("#S: {format}"))
        .unwrap_or_else(|| {
            "#{session_name}: #{session_windows} windows #{?session_attached,[attached],[detached]}"
                .into()
        });
    list_rows([
        "list-sessions",
        "-F",
        &format!("#{{session_id}}\t{display}"),
    ])
}

fn window_rows(config: &ManagerConfig) -> Result<Vec<Row>> {
    let display = config
        .window_format
        .as_deref()
        .map(|format| format!("#S:#{{window_index}}: {format}"))
        .unwrap_or_else(|| {
            "#S:#{window_index}: #{window_name} #{?window_active,[active],[inactive]}".into()
        });
    let format = format!("#{{window_id}}\t#{{session_id}}\t{display}");
    let mut args = vec!["list-windows", "-a"];
    if let Some(filter) = config.window_filter.as_deref() {
        args.extend(["-f", filter]);
    }
    args.extend(["-F", format.as_str()]);
    let output = tmux::stdout(args)?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let id = fields.next()?;
            let session_id = fields.next()?;
            let display = fields.next().unwrap_or_default();
            Some(Row::new(window_target(session_id, id), display))
        })
        .collect())
}

fn window_target(session_id: &str, window_id: &str) -> String {
    format!("{session_id}:{window_id}")
}

fn window_target_parts(target: &str) -> Result<(&str, &str)> {
    target
        .split_once(':')
        .context("manager window target is missing its session")
}

fn pane_rows(config: &ManagerConfig) -> Result<Vec<Row>> {
    let display = config
        .pane_format
        .as_deref()
        .map(|format| format!("#S:#{{window_index}}.#{{pane_index}}: {format}"))
        .unwrap_or_else(|| "#S:#{window_index}.#{pane_index}: [#{window_name}:#{pane_title}] #{pane_current_command} [#{pane_width}x#{pane_height}] [history #{history_size}/#{history_limit}] #{?pane_active,[active],[inactive]}".into());
    list_rows([
        "list-panes",
        "-a",
        "-F",
        &format!("#{{pane_id}}\t{display}"),
    ])
}

fn list_rows<I, S>(args: I) -> Result<Vec<Row>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = tmux::stdout(args)?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (id, display) = line.split_once('\t')?;
            Some(Row::new(id, display))
        })
        .collect())
}

fn resolve_action(
    action: Option<&str>,
    actions: &[&str],
    config: &ManagerConfig,
    prompt: &str,
) -> Result<String> {
    if let Some(action) = action {
        if actions.contains(&action) {
            return Ok(action.to_string());
        }
        anyhow::bail!("unknown action '{action}'");
    }
    let rows = rows(actions);
    Ok(
        choose(&rows, config, prompt, "Select an action", false, None)?
            .first()
            .map(|index| rows[*index].id.clone())
            .unwrap_or_default(),
    )
}

fn selected_rows<'a>(
    rows: &'a [Row],
    config: &ManagerConfig,
    prompt: &str,
    header: &str,
    multi: bool,
    preview_kind: Option<&str>,
) -> Result<Vec<&'a Row>> {
    Ok(choose(rows, config, prompt, header, multi, preview_kind)?
        .into_iter()
        .filter_map(|index| rows.get(index))
        .collect())
}

fn choose(
    rows: &[Row],
    config: &ManagerConfig,
    prompt: &str,
    header: &str,
    multi: bool,
    preview_kind: Option<&str>,
) -> Result<Vec<usize>> {
    if rows.is_empty() {
        tmux::display_message("tmux-history-finder: no manager items available");
        return Ok(Vec::new());
    }
    let mut data = NamedTempFile::new()?;
    serde_json::to_writer(&mut data, rows)?;
    data.flush()?;
    let mut command = picker_command(config)?;
    command.args([
        "--delimiter",
        "\t",
        "--with-nth",
        "2..",
        "--layout=reverse",
        "--info=inline",
        "--tiebreak=index",
        "--prompt",
        prompt,
        "--header",
        header,
    ]);
    command.arg(if multi { "-m" } else { "+m" });
    if let Some(kind) = preview_kind {
        let exe = env::current_exe()?;
        let preview = format!(
            "{} manage-preview --kind {} --data {} --row {{1}}",
            shell_quote(&exe.to_string_lossy()),
            shell_quote(kind),
            shell_quote(&data.path().to_string_lossy())
        );
        let fzf_version = tmux::command_version("fzf", &["--version"]).unwrap_or_default();
        let follow = if config.preview_follow && version_at_least(&fzf_version, 0, 24, 4) {
            ":follow"
        } else {
            ""
        };
        let hidden = if config.preview { "" } else { ":hidden" };
        command.args([
            "--preview-window",
            &format!("right:60%:wrap{follow}{hidden}"),
            "--preview",
            preview.as_str(),
        ]);
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = command.spawn().context("failed to start manager fzf")?;
    if let Some(stdin) = child.stdin.as_mut() {
        for (index, row) in rows.iter().enumerate() {
            if let Err(error) = writeln!(stdin, "{}\t{}", index, sanitize(&row.display)) {
                if error.kind() != ErrorKind::BrokenPipe {
                    return Err(error.into());
                }
                break;
            }
        }
    }
    let output = child.wait_with_output()?;
    match output.status.code() {
        Some(0) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| {
                line.split('\t')
                    .next()
                    .context("fzf returned an empty row")?
                    .parse::<usize>()
                    .context("fzf returned an invalid row")
            })
            .collect(),
        Some(1) | Some(130) => Ok(Vec::new()),
        _ => anyhow::bail!("manager fzf exited with status {}", output.status),
    }
}

fn prompt_text(config: &ManagerConfig, prompt: &str, header: &str) -> Result<Option<String>> {
    let mut command = picker_command(config)?;
    command.args([
        "+m",
        "--print-query",
        "--phony",
        "--no-sort",
        "--prompt",
        prompt,
        "--header",
        header,
    ]);
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        writeln!(stdin, "Press Enter to accept the query")?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let query = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    Ok((!query.is_empty()).then_some(query))
}

fn picker_command(config: &ManagerConfig) -> Result<Command> {
    let use_tmux = tmux::have("fzf-tmux") && env::var_os("TMUX").is_some();
    let mut command = Command::new(if use_tmux { "fzf-tmux" } else { "fzf" });
    let parsed =
        shell_words::split(&config.fzf_options).context("failed to parse manager fzf options")?;
    let options = if use_tmux {
        let options = without_multi(parsed);
        if fzf::supports_popup() {
            options
        } else {
            without_popup_options(options)
        }
    } else {
        without_tmux_options(without_multi(parsed))
    };
    command.args(options);
    Ok(command)
}

fn without_multi(options: Vec<String>) -> Vec<String> {
    options
        .into_iter()
        .filter(|option| !is_multi_option(option))
        .collect()
}

fn is_multi_option(option: &str) -> bool {
    if matches!(option, "-m" | "+m" | "--multi" | "--no-multi") || option.starts_with("--multi=") {
        return true;
    }
    option
        .strip_prefix("-m")
        .is_some_and(|count| !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()))
}

fn without_tmux_options(options: Vec<String>) -> Vec<String> {
    without_layout_options(options, true, false)
}

fn without_popup_options(options: Vec<String>) -> Vec<String> {
    without_layout_options(options, false, true)
}

fn without_layout_options(
    options: Vec<String>,
    include_split: bool,
    preserve_separator: bool,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut iter = options.into_iter();
    while let Some(option) = iter.next() {
        if option == "--" {
            if preserve_separator {
                result.push(option);
            }
            result.extend(iter);
            break;
        }
        if is_tmux_layout_option(&option, include_split) {
            if tmux_layout_option_takes_value(&option)
                && iter
                    .as_slice()
                    .first()
                    .is_some_and(|value| is_tmux_layout_value(value))
            {
                let _ = iter.next();
            }
            continue;
        }
        result.push(option);
    }
    result
}

fn is_tmux_layout_option(option: &str, include_split: bool) -> bool {
    if option == "--popup" || option.starts_with("--popup=") {
        return true;
    }
    let Some(flag) = option
        .strip_prefix('-')
        .and_then(|value| value.chars().next())
    else {
        return false;
    };
    matches!(flag, 'p' | 'w' | 'h' | 'x' | 'y')
        || (include_split && matches!(flag, 'd' | 'u' | 'l' | 'r'))
}

fn tmux_layout_option_takes_value(option: &str) -> bool {
    option == "--popup"
        || matches!(
            option,
            "-p" | "-w" | "-h" | "-x" | "-y" | "-d" | "-u" | "-l" | "-r"
        )
}

fn is_tmux_layout_value(value: &str) -> bool {
    !value.is_empty()
        && (value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'%' | b','))
            || (value.len() == 1 && value.as_bytes()[0].is_ascii_uppercase()))
}

fn destructive_tmux(
    config: &ManagerConfig,
    prompt: &str,
    commands: Vec<Vec<OsString>>,
) -> Result<()> {
    let client = tmux::TmuxClient::from_env()?;
    if !config.confirm {
        return client.run_commands(&commands);
    }
    let command = commands
        .iter()
        .map(|args| {
            args.iter()
                .map(|arg| shell_quote(&arg.to_string_lossy()))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ; ");
    client.run([
        "confirm-before",
        "-p",
        &format!("{prompt} (y/n)"),
        command.as_str(),
    ])
}

pub fn doctor_summary() -> Result<String> {
    let config = ManagerConfig::load()?;
    Ok(format!(
        "key={} order={} preview={} confirm={} copyq={} pstree={} privilege={}",
        config.key,
        config.order.join("|"),
        config.preview,
        config.confirm,
        tmux::have("copyq"),
        tmux::have("pstree"),
        if tmux::have("sudo") {
            "sudo"
        } else if tmux::have("doas") {
            "doas"
        } else {
            "none"
        }
    ))
}

fn command<const N: usize>(args: [&str; N]) -> Vec<OsString> {
    args.into_iter().map(OsString::from).collect()
}

fn client_switch_commands(session: &str) -> Vec<Vec<OsString>> {
    if env::var_os("TMUX").is_some() {
        vec![command(["switch-client", "-t", session])]
    } else {
        Vec::new()
    }
}

fn rows(values: &[&str]) -> Vec<Row> {
    values
        .iter()
        .map(|value| Row::new(*value, *value))
        .collect()
}

impl Row {
    fn new(id: impl Into<String>, display: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display: display.into(),
        }
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("expected a boolean, got '{value}'"),
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character == '\t' || character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn print_tmux<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    print!("{}", tmux::stdout(args)?);
    Ok(())
}

fn print_command<I, S>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    print!("{}", command_stdout(program, args)?);
    Ok(())
}

fn command_stdout<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn command_stdout_bytes<I, S>(program: &str, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn ensure_program(program: &str, message: &str) -> Result<()> {
    if tmux::have(program) {
        Ok(())
    } else {
        anyhow::bail!(message.to_string())
    }
}

fn session_attached(session: &str) -> bool {
    tmux::try_stdout([
        "display-message",
        "-p",
        "-t",
        session,
        "#{session_attached}",
    ])
    .as_deref()
        == Some("1")
}

#[cfg(test)]
mod tests {
    use super::{
        parse_bool, parse_menu, without_multi, without_popup_options, without_tmux_options,
    };

    #[test]
    fn parses_legacy_menu() {
        let menu = parse_menu("foo\\necho hello\\n\\nbar\\nprintf world\\n\\nnil--\\n\\n").unwrap();
        assert_eq!(menu[0], ("foo".into(), "echo hello".into()));
        assert_eq!(menu[1], ("bar".into(), "printf world".into()));
    }

    #[test]
    fn rejects_duplicate_menu_labels() {
        assert!(parse_menu("foo\none\n\nfoo\ntwo\n").is_err());
    }

    #[test]
    fn strips_popup_options_for_plain_fzf() {
        assert_eq!(
            without_tmux_options(vec![
                "-p".into(),
                "80%,60%".into(),
                "-w80%".into(),
                "-d".into(),
                "40%".into(),
                "--".into(),
                "--ansi".into()
            ]),
            vec!["--ansi"]
        );
    }

    #[test]
    fn keeps_split_options_when_popup_is_unsupported() {
        assert_eq!(
            without_popup_options(vec![
                "-p80%,60%".into(),
                "-w".into(),
                "80%".into(),
                "-d".into(),
                "40%".into(),
                "--".into(),
                "-p".into(),
            ]),
            vec!["-d", "40%", "--", "-p"]
        );
    }

    #[test]
    fn manager_controls_multi_selection() {
        assert_eq!(
            without_multi(vec![
                "-m".into(),
                "-m3".into(),
                "--multi=4".into(),
                "--ansi".into(),
            ]),
            vec!["--ansi"]
        );
    }

    #[test]
    fn parses_boolean_values() {
        assert!(parse_bool("yes").unwrap());
        assert!(!parse_bool("off").unwrap());
        assert!(parse_bool("maybe").is_err());
    }
}
