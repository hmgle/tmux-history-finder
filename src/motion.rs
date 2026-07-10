use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    io::{self, Read, Write},
    os::fd::AsRawFd,
};

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{config::Config, tmux, types::CaseMode, util::shell_quote};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum MotionKind {
    S,
    S2,
}

impl MotionKind {
    pub fn pattern_len(self) -> usize {
        match self {
            Self::S => 1,
            Self::S2 => 2,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::S => "s",
            Self::S2 => "s2",
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct MotionArgs {
    #[arg(value_enum, default_value = "s")]
    pub kind: MotionKind,
    #[arg(value_name = "PATTERN")]
    pub pattern: Option<String>,
    #[arg(short = 'q', long = "query")]
    pub query: Option<String>,
    #[arg(long = "query-option", hide = true)]
    pub query_option: Option<String>,
    #[arg(long = "target-window", hide = true)]
    pub target_window: Option<String>,
    #[arg(long = "target-client", hide = true)]
    pub target_client: Option<String>,
    #[arg(long = "overlay", hide = true)]
    pub overlay: bool,
    #[arg(long = "case", value_enum)]
    pub case_mode: Option<CaseMode>,
    #[arg(long = "smartsign", conflicts_with = "no_smartsign")]
    pub smartsign: bool,
    #[arg(long = "no-smartsign", conflicts_with = "smartsign")]
    pub no_smartsign: bool,
}

pub fn run(args: MotionArgs, config: &Config) -> Result<()> {
    let mut motion_config = MotionConfig::from_config(config);
    if let Some(case_mode) = args.case_mode {
        motion_config.case_mode = case_mode;
    }
    if args.smartsign {
        motion_config.smartsign = true;
    }
    if args.no_smartsign {
        motion_config.smartsign = false;
    }

    let Some(pattern) = resolve_pattern(&args, args.kind.pattern_len()) else {
        return Ok(());
    };
    let target_window = resolve_target_window(args.target_window.as_deref());
    let target_client = resolve_target_client(args.target_client.as_deref());

    let panes = init_panes(target_window.as_deref())?;
    if panes.is_empty() {
        tmux::display_message("tmux-history-finder motion: no visible panes");
        return Ok(());
    }

    let matches = find_matches(
        &panes,
        &pattern,
        motion_config.case_mode,
        motion_config.smartsign,
        motion_config.tab_mode,
    );
    if matches.is_empty() {
        tmux::display_message("tmux-history-finder motion: no match");
        return Ok(());
    }
    if matches.len() == 1 {
        move_to_match(
            &panes,
            &matches[0],
            motion_config.tab_mode,
            target_client.as_deref(),
        )?;
        return Ok(());
    }
    if !args.overlay {
        return run_popup(
            &args,
            &pattern,
            target_window.as_deref(),
            target_popup_pane(&panes).as_deref(),
            target_client.as_deref(),
        );
    }

    let current = panes
        .iter()
        .find(|pane| pane.active)
        .context("active pane not found")?;
    let cursor_y = current.start_y + current.cursor_y;
    let cursor_x = current.start_x + current.cursor_x;
    let hint_mapping =
        assign_hints_by_distance(&panes, &matches, cursor_y, cursor_x, &motion_config.hints);
    let positions = hint_positions(&panes, &hint_mapping, motion_config.tab_mode);

    if positions.is_empty() {
        tmux::display_message("tmux-history-finder motion: no drawable hints");
        return Ok(());
    }

    let (window_width, window_height) = window_size(target_window.as_deref())?;
    let terminal_height = window_height;
    let max_x = panes
        .iter()
        .map(|pane| pane.start_x + pane.width)
        .max()
        .unwrap_or(window_width);
    let mut screen = AnsiScreen::new(&motion_config);
    screen.init()?;
    let result = run_overlay(
        &mut screen,
        Overlay {
            panes: &panes,
            positions: &positions,
            hint_mapping: &hint_mapping,
            max_x,
            terminal_height,
            target_client: target_client.as_deref(),
            config: &motion_config,
        },
    );
    screen.cleanup()?;
    result
}

fn resolve_pattern(args: &MotionArgs, len: usize) -> Option<String> {
    let raw = args
        .query
        .as_deref()
        .or(args.pattern.as_deref())
        .map(str::to_string)
        .or_else(|| {
            args.query_option
                .as_deref()
                .and_then(tmux::show_option)
                .inspect(|_| {
                    if let Some(option) = args.query_option.as_deref() {
                        tmux::run_ignore(["set-option", "-gu", option]);
                    }
                })
        })?;

    let pattern: String = raw
        .chars()
        .filter(|ch| *ch != '\n' && *ch != '\r')
        .take(len)
        .collect();
    (!pattern.is_empty()).then_some(pattern)
}

fn resolve_target_window(explicit: Option<&str>) -> Option<String> {
    explicit
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| tmux::try_stdout(["display-message", "-p", "#{window_id}"]))
        .filter(|value| !value.is_empty())
}

fn resolve_target_client(explicit: Option<&str>) -> Option<String> {
    explicit
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| tmux::try_stdout(["display-message", "-p", "#{client_name}"]))
        .filter(|value| !value.is_empty())
}

fn run_popup(
    args: &MotionArgs,
    pattern: &str,
    target_window: Option<&str>,
    target_pane: Option<&str>,
    target_client: Option<&str>,
) -> Result<()> {
    let target_window = target_window.context("motion target window not found")?;
    let target_pane = target_pane.context("motion target pane not found")?;
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    tmux::run(popup_command(
        exe.to_string_lossy().as_ref(),
        args,
        pattern,
        target_window,
        target_pane,
        target_client,
    ))
}

fn target_popup_pane(panes: &[Pane]) -> Option<String> {
    panes
        .iter()
        .find(|pane| pane.active)
        .or_else(|| panes.first())
        .map(|pane| pane.pane_id.clone())
}

fn popup_command(
    exe: &str,
    args: &MotionArgs,
    pattern: &str,
    target_window: &str,
    target_pane: &str,
    target_client: Option<&str>,
) -> Vec<String> {
    let mut command = vec![
        shell_quote(exe),
        "motion".into(),
        args.kind.as_str().into(),
        "--query".into(),
        shell_quote(pattern),
        "--target-window".into(),
        shell_quote(target_window),
        "--overlay".into(),
    ];
    if let Some(target_client) = target_client {
        command.push("--target-client".into());
        command.push(shell_quote(target_client));
    }
    if let Some(case_mode) = args.case_mode {
        command.push("--case".into());
        command.push(case_mode.to_string());
    }
    if args.smartsign {
        command.push("--smartsign".into());
    }
    if args.no_smartsign {
        command.push("--no-smartsign".into());
    }

    let mut popup = vec![
        "display-popup".to_string(),
        "-E".to_string(),
        "-B".to_string(),
        "-w".to_string(),
        "100%".to_string(),
        "-h".to_string(),
        "100%".to_string(),
        "-t".to_string(),
        target_pane.to_string(),
    ];
    if let Some(target_client) = target_client {
        popup.push("-c".to_string());
        popup.push(target_client.to_string());
    }
    popup.push(command.join(" "));
    popup
}

#[derive(Clone, Debug)]
struct MotionConfig {
    hints: String,
    case_mode: CaseMode,
    smartsign: bool,
    tab_mode: TabMode,
    vertical_border: String,
    horizontal_border: String,
    hint1_fg: String,
    hint2_fg: String,
    dim: String,
}

impl MotionConfig {
    fn from_config(config: &Config) -> Self {
        Self {
            hints: config.motion_hints.clone(),
            case_mode: config.motion_case_mode,
            smartsign: config.motion_smartsign,
            tab_mode: detect_tmux_tab_mode(),
            vertical_border: config.motion_vertical_border.clone(),
            horizontal_border: config.motion_horizontal_border.clone(),
            hint1_fg: config.motion_hint1_fg.clone(),
            hint2_fg: config.motion_hint2_fg.clone(),
            dim: config.motion_dim.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabMode {
    Fixed,
    PositionAware,
}

fn detect_tmux_tab_mode() -> TabMode {
    tmux::command_version("tmux", &["-V"])
        .as_deref()
        .map(tmux_tab_mode_from_version)
        .unwrap_or(TabMode::Fixed)
}

fn tmux_tab_mode_from_version(version: &str) -> TabMode {
    let lower = version.to_ascii_lowercase();
    if lower.contains("master") || lower.contains("openbsd-") {
        return TabMode::Fixed;
    }

    parse_tmux_major_minor(version)
        .filter(|version| *version >= (3, 6))
        .map(|_| TabMode::PositionAware)
        .unwrap_or(TabMode::Fixed)
}

fn parse_tmux_major_minor(version: &str) -> Option<(usize, usize)> {
    let bytes = version.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if !bytes[idx].is_ascii_digit() {
            idx += 1;
            continue;
        }

        let major_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx >= bytes.len() || bytes[idx] != b'.' {
            continue;
        }
        let major = version[major_start..idx].parse().ok()?;
        idx += 1;
        let minor_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if minor_start == idx {
            continue;
        }
        let minor = version[minor_start..idx].parse().ok()?;
        return Some((major, minor));
    }
    None
}

#[derive(Clone, Debug)]
struct Pane {
    window_id: String,
    pane_id: String,
    active: bool,
    start_y: usize,
    height: usize,
    start_x: usize,
    width: usize,
    copy_mode: bool,
    scroll_position: isize,
    cursor_y: usize,
    cursor_x: usize,
    lines: Vec<String>,
}

#[derive(Clone, Debug)]
struct Match {
    pane_index: usize,
    line_no: usize,
    visual_col: usize,
}

#[derive(Clone, Debug)]
struct HintTarget {
    hint: String,
    target: Match,
}

#[derive(Clone, Debug)]
struct HintPosition {
    screen_y: usize,
    screen_x: usize,
    pane_right_edge: usize,
    original_width: usize,
    original: String,
    next_original: String,
    hint: String,
}

fn init_panes(target_window: Option<&str>) -> Result<Vec<Pane>> {
    let mut panes: Vec<Pane> = list_visible_panes(target_window)?
        .into_iter()
        .filter(|pane| pane.height > 0 && pane.width > 0)
        .collect();
    for pane in &mut panes {
        pane.lines = capture_visible_pane(pane)?;
    }
    Ok(panes)
}

fn list_visible_panes(target_window: Option<&str>) -> Result<Vec<Pane>> {
    let fmt = [
        "#{window_id}",
        "#{pane_id}",
        "#{window_zoomed_flag}",
        "#{pane_active}",
        "#{pane_top}",
        "#{pane_height}",
        "#{pane_left}",
        "#{pane_width}",
        "#{pane_in_mode}",
        "#{scroll_position}",
        "#{cursor_y}",
        "#{cursor_x}",
        "#{copy_cursor_y}",
        "#{copy_cursor_x}",
    ]
    .join("\t");
    let mut args: Vec<OsString> = vec!["list-panes".into(), "-F".into(), fmt.into()];
    if let Some(target_window) = target_window {
        args.push("-t".into());
        args.push(target_window.into());
    }
    let output = tmux::stdout(args)?;
    let zoomed = window_zoomed(&output);
    let panes = output
        .lines()
        .filter_map(parse_pane_line)
        .filter(|pane| pane.active || !zoomed)
        .collect();
    Ok(panes)
}

fn parse_pane_line(line: &str) -> Option<Pane> {
    let mut parts = line.splitn(14, '\t');
    let window_id = parts.next()?.to_string();
    let pane_id = parts.next()?.to_string();
    let _zoomed = parts.next()?;
    let active = parts.next()? == "1";
    let start_y = parse_usize(parts.next()?);
    let height = parse_usize(parts.next()?);
    let start_x = parse_usize(parts.next()?);
    let width = parse_usize(parts.next()?);
    let copy_mode = parts.next()? == "1";
    let scroll_position = parts.next()?.parse().unwrap_or_default();
    let cursor_y = parse_usize(parts.next()?);
    let cursor_x = parse_usize(parts.next()?);
    let copy_cursor_y = parse_usize(parts.next()?);
    let copy_cursor_x = parse_usize(parts.next()?);
    let (cursor_y, cursor_x) = if copy_mode {
        (copy_cursor_y, copy_cursor_x)
    } else {
        (cursor_y, cursor_x)
    };

    Some(Pane {
        window_id,
        pane_id,
        active,
        start_y,
        height,
        start_x,
        width,
        copy_mode,
        scroll_position,
        cursor_y,
        cursor_x,
        lines: Vec::new(),
    })
}

fn parse_usize(value: &str) -> usize {
    value.parse().unwrap_or_default()
}

fn window_zoomed(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split('\t').nth(2))
        .any(|zoomed| zoomed == "1")
}

fn capture_visible_pane(pane: &Pane) -> Result<Vec<String>> {
    let mut args: Vec<OsString> = vec!["capture-pane".into(), "-p".into()];

    if pane.scroll_position > 0 {
        let start = format!("-{}", pane.scroll_position);
        let end = (-(pane.scroll_position - pane.height as isize + 1)).to_string();
        args.push("-S".into());
        args.push(start.into());
        args.push("-E".into());
        args.push(end.into());
    }
    args.push("-t".into());
    args.push(pane.pane_id.clone().into());

    let output = tmux::stdout(args)?;
    let output = output.strip_suffix('\n').unwrap_or(&output);
    Ok(output
        .split('\n')
        .take(pane.height)
        .map(ToOwned::to_owned)
        .collect())
}

fn window_size(target_window: Option<&str>) -> Result<(usize, usize)> {
    let mut args: Vec<OsString> = vec!["display-message".into(), "-p".into()];
    if let Some(target_window) = target_window {
        args.push("-t".into());
        args.push(target_window.into());
    }
    args.push("#{window_width},#{window_height}".into());
    let output = tmux::stdout(args)?;
    let (width, height) = output
        .trim()
        .split_once(',')
        .context("malformed tmux window size")?;
    Ok((parse_usize(width), parse_usize(height)))
}

struct Overlay<'a> {
    panes: &'a [Pane],
    positions: &'a [HintPosition],
    hint_mapping: &'a [HintTarget],
    max_x: usize,
    terminal_height: usize,
    target_client: Option<&'a str>,
    config: &'a MotionConfig,
}

fn run_overlay(screen: &mut AnsiScreen, overlay: Overlay<'_>) -> Result<()> {
    draw_all_panes(
        screen,
        overlay.panes,
        overlay.max_x,
        overlay.terminal_height,
        overlay.config,
    )?;
    draw_all_hints(screen, overlay.positions, overlay.terminal_height)?;

    let hints_chars: HashSet<char> = overlay.config.hints.chars().collect();
    let max_hint_len = overlay
        .hint_mapping
        .iter()
        .map(|target| target.hint.chars().count())
        .max()
        .unwrap_or_default();
    let mut key_sequence = String::new();
    let _raw = RawMode::new()?;
    loop {
        let ch = read_key(&mut io::stdin())?;
        if !hints_chars.contains(&ch) {
            return Ok(());
        }

        key_sequence.push(ch);
        if let Some(target) = overlay
            .hint_mapping
            .iter()
            .find(|target| target.hint == key_sequence)
            .map(|target| &target.target)
        {
            move_to_match(
                overlay.panes,
                target,
                overlay.config.tab_mode,
                overlay.target_client,
            )?;
            return Ok(());
        }
        if key_sequence.chars().count() >= max_hint_len {
            return Ok(());
        }
        update_hints_display(screen, overlay.positions, &key_sequence)?;
    }
}

fn draw_all_panes(
    screen: &mut AnsiScreen,
    panes: &[Pane],
    max_x: usize,
    terminal_height: usize,
    config: &MotionConfig,
) -> Result<()> {
    let mut sorted = panes.to_vec();
    sorted.sort_by_key(|pane| pane.start_y + pane.height);

    for pane in sorted {
        let visible_height = pane
            .height
            .min(terminal_height.saturating_sub(pane.start_y));
        for (y, line) in pane.lines.iter().take(visible_height).enumerate() {
            let expanded = expand_tabs(line, config.tab_mode);
            let sliced = visual_slice(&expanded, pane.width, config.tab_mode);
            screen.addstr(pane.start_y + y, pane.start_x, &sliced, Attr::Normal)?;
        }

        if pane.start_x + pane.width < max_x {
            for y in pane.start_y..pane.start_y + visible_height {
                screen.addstr(
                    y,
                    pane.start_x + pane.width,
                    &config.vertical_border,
                    Attr::Dim,
                )?;
            }
        }

        let end_y = pane.start_y + visible_height;
        if end_y < terminal_height {
            screen.addstr(
                end_y,
                pane.start_x,
                &config.horizontal_border.repeat(pane.width),
                Attr::Dim,
            )?;
        }
    }
    screen.refresh()
}

fn draw_all_hints(
    screen: &mut AnsiScreen,
    positions: &[HintPosition],
    terminal_height: usize,
) -> Result<()> {
    for position in positions {
        if position.screen_y >= terminal_height {
            continue;
        }
        let mut hint_chars = position.hint.chars();
        if let Some(first) = hint_chars.next() {
            screen.addstr(
                position.screen_y,
                position.screen_x,
                &first.to_string(),
                Attr::Hint1,
            )?;
        }
        if let Some(second) = hint_chars.next() {
            let next_x = position.screen_x + position.original_width;
            if next_x < position.pane_right_edge {
                screen.addstr(position.screen_y, next_x, &second.to_string(), Attr::Hint2)?;
            }
        }
    }
    screen.refresh()
}

fn update_hints_display(
    screen: &mut AnsiScreen,
    positions: &[HintPosition],
    current_key: &str,
) -> Result<()> {
    for position in positions {
        let next_x = position.screen_x + position.original_width;
        let restore_next = || {
            if next_x < position.pane_right_edge {
                if position.next_original.is_empty() {
                    " ".to_string()
                } else {
                    position.next_original.clone()
                }
            } else {
                String::new()
            }
        };

        if !position.hint.starts_with(current_key) {
            screen.addstr(
                position.screen_y,
                position.screen_x,
                &position.original,
                Attr::Normal,
            )?;
            let next = restore_next();
            if !next.is_empty() {
                screen.addstr(position.screen_y, next_x, &next, Attr::Normal)?;
            }
            continue;
        }

        let current_len = current_key.chars().count();
        if position.hint.chars().count() > current_len {
            if let Some(next_hint) = position.hint.chars().nth(current_len) {
                let next = restore_next();
                if !next.is_empty() {
                    screen.addstr(position.screen_y, next_x, &next, Attr::Normal)?;
                }
                screen.addstr(
                    position.screen_y,
                    position.screen_x,
                    &next_hint.to_string(),
                    Attr::Hint2,
                )?;
            }
        } else {
            screen.addstr(
                position.screen_y,
                position.screen_x,
                &position.original,
                Attr::Normal,
            )?;
            let next = restore_next();
            if !next.is_empty() {
                screen.addstr(position.screen_y, next_x, &next, Attr::Normal)?;
            }
        }
    }
    screen.refresh()
}

fn move_to_match(
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
    move_cursor(pane, target.line_no, true_col, target_client)
}

fn move_cursor(
    pane: &Pane,
    line_no: usize,
    true_col: usize,
    target_client: Option<&str>,
) -> Result<()> {
    if let Some(target_client) = target_client {
        tmux::run([
            "switch-client",
            "-c",
            target_client,
            "-t",
            pane.window_id.as_str(),
        ])?;
    } else {
        tmux::run(["select-window", "-t", pane.window_id.as_str()])?;
    }
    tmux::run(["select-pane", "-t", pane.pane_id.as_str()])?;
    if !pane.copy_mode {
        tmux::run(["copy-mode", "-t", pane.pane_id.as_str()])?;
    }
    send_copy_command(pane, &["top-line"])?;
    send_copy_command(pane, &["start-of-line"])?;

    let first_non_empty = pane
        .lines
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or_default();
    let mut rows_remaining = line_no;
    if first_non_empty > 0 && first_non_empty <= line_no {
        let first = first_non_empty.to_string();
        send_copy_command(pane, &["-N", first.as_str(), "cursor-down"])?;
        send_copy_command(pane, &["start-of-line"])?;
        rows_remaining -= first_non_empty;
    }
    if rows_remaining > 0 {
        let rows = rows_remaining.to_string();
        send_copy_command(pane, &["-N", rows.as_str(), "cursor-down"])?;
    }
    if true_col > 0 {
        let col = true_col.to_string();
        send_copy_command(pane, &["-N", col.as_str(), "cursor-right"])?;
    }
    Ok(())
}

fn send_copy_command(pane: &Pane, args: &[&str]) -> Result<()> {
    let mut command = vec!["send-keys", "-X", "-t", pane.pane_id.as_str()];
    command.extend_from_slice(args);
    tmux::run(command)
}

fn find_matches(
    panes: &[Pane],
    pattern: &str,
    case_mode: CaseMode,
    smartsign: bool,
    tab_mode: TabMode,
) -> Vec<Match> {
    let patterns = smartsign_patterns(pattern, smartsign);
    let sensitive = case_mode.is_sensitive_for(pattern);
    let pattern_len = pattern.chars().count();
    let mut matches = Vec::new();

    for (pane_index, pane) in panes.iter().enumerate() {
        for (line_no, line) in pane.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() < pattern_len {
                continue;
            }
            let metrics = line_metrics(line, tab_mode);
            for idx in 0..=chars.len() - pattern_len {
                let end = idx + pattern_len;
                if !metrics.is_boundary(idx) || !metrics.is_boundary(end) {
                    continue;
                }

                let substring: String = chars[idx..idx + pattern_len].iter().collect();
                if patterns
                    .iter()
                    .any(|candidate| text_eq(&substring, candidate, sensitive))
                {
                    matches.push(Match {
                        pane_index,
                        line_no,
                        visual_col: metrics.visual_col(idx),
                    });
                }
            }
        }
    }

    matches
}

fn text_eq(left: &str, right: &str, sensitive: bool) -> bool {
    if sensitive {
        left == right
    } else {
        left.to_lowercase() == right.to_lowercase()
    }
}

fn smartsign_patterns(pattern: &str, enabled: bool) -> Vec<String> {
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

#[derive(Clone, Debug)]
struct LineMetrics {
    boundaries: Vec<bool>,
    visual_cols: Vec<usize>,
}

impl LineMetrics {
    fn is_boundary(&self, char_index: usize) -> bool {
        self.boundaries.get(char_index).copied().unwrap_or(false)
    }

    fn visual_col(&self, char_index: usize) -> usize {
        self.visual_cols
            .get(char_index)
            .copied()
            .unwrap_or_default()
    }
}

fn line_metrics(line: &str, tab_mode: TabMode) -> LineMetrics {
    let char_len = line.chars().count();
    let mut boundaries = vec![false; char_len + 1];
    let mut visual_cols = vec![0; char_len + 1];
    let mut char_pos = 0;
    let mut visual_pos = 0;

    boundaries[0] = true;
    for grapheme in line.graphemes(true) {
        boundaries[char_pos] = true;
        visual_cols[char_pos] = visual_pos;
        char_pos += grapheme.chars().count();
        visual_pos += grapheme_width_at(grapheme, visual_pos, tab_mode);
        boundaries[char_pos] = true;
        visual_cols[char_pos] = visual_pos;
    }

    LineMetrics {
        boundaries,
        visual_cols,
    }
}

fn assign_hints_by_distance(
    panes: &[Pane],
    matches: &[Match],
    cursor_y: usize,
    cursor_x: usize,
    hint_keys: &str,
) -> Vec<HintTarget> {
    let mut sorted = matches.to_vec();
    sorted.sort_by_key(|target| {
        let pane = &panes[target.pane_index];
        let y = pane.start_y + target.line_no;
        let x = pane.start_x + target.visual_col;
        y.abs_diff(cursor_y).pow(2) + x.abs_diff(cursor_x).pow(2)
    });
    let hints = generate_hints(hint_keys, sorted.len());
    hints
        .into_iter()
        .zip(sorted)
        .map(|(hint, target)| HintTarget { hint, target })
        .collect()
}

fn generate_hints(keys: &str, needed_count: usize) -> Vec<String> {
    if needed_count == 0 {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let keys: Vec<char> = keys.chars().filter(|key| seen.insert(*key)).collect();
    let key_count = keys.len();
    if key_count == 0 {
        return Vec::new();
    }
    if key_count == 1 && needed_count > 1 {
        return Vec::new();
    }
    if needed_count <= key_count {
        return keys
            .iter()
            .take(needed_count)
            .map(char::to_string)
            .collect();
    }

    let mut levels = BTreeMap::<usize, Vec<String>>::new();
    levels.insert(1, keys.iter().map(char::to_string).collect());
    let mut leaf_count = key_count;
    while leaf_count < needed_count {
        let shortest = *levels.keys().next().expect("hint levels are non-empty");
        let prefix = levels
            .get_mut(&shortest)
            .and_then(Vec::pop)
            .expect("shortest hint level is non-empty");
        if levels.get(&shortest).is_some_and(Vec::is_empty) {
            levels.remove(&shortest);
        }
        let children = levels.entry(shortest + 1).or_default();
        children.extend(keys.iter().map(|key| {
            let mut hint = prefix.clone();
            hint.push(*key);
            hint
        }));
        leaf_count += key_count - 1;
    }
    let mut hints: Vec<String> = levels.into_values().flatten().collect();
    hints.truncate(needed_count);
    hints
}

fn hint_positions(
    panes: &[Pane],
    hint_mapping: &[HintTarget],
    tab_mode: TabMode,
) -> Vec<HintPosition> {
    hint_mapping
        .iter()
        .filter_map(|entry| {
            let target = &entry.target;
            let pane = panes.get(target.pane_index)?;
            let line = pane.lines.get(target.line_no)?;
            let true_col = true_position(line, target.visual_col, tab_mode);
            let (original, next_original) = grapheme_at_char_index(line, true_col)?;
            let original_width = grapheme_width_at(original, target.visual_col, tab_mode);
            let original = display_grapheme(original, target.visual_col, tab_mode);
            let next_original = next_original
                .map(|next| display_grapheme(next, target.visual_col + original_width, tab_mode))
                .unwrap_or_else(|| " ".to_string());
            Some(HintPosition {
                screen_y: pane.start_y + target.line_no,
                screen_x: pane.start_x + target.visual_col,
                pane_right_edge: pane.start_x + pane.width,
                original_width,
                original,
                next_original,
                hint: entry.hint.clone(),
            })
        })
        .collect()
}

fn calculate_tab_width(position: usize) -> usize {
    8 - (position % 8)
}

fn grapheme_at_char_index(line: &str, target_char_index: usize) -> Option<(&str, Option<&str>)> {
    let mut char_index = 0;
    let mut iter = line.graphemes(true).peekable();
    while let Some(grapheme) = iter.next() {
        let next_index = char_index + grapheme.chars().count();
        if char_index == target_char_index {
            return Some((grapheme, iter.peek().copied()));
        }
        if next_index > target_char_index {
            return None;
        }
        char_index = next_index;
    }
    None
}

fn grapheme_width_at(grapheme: &str, position: usize, tab_mode: TabMode) -> usize {
    if grapheme == "\t" {
        match tab_mode {
            TabMode::Fixed => 8,
            TabMode::PositionAware => calculate_tab_width(position),
        }
    } else {
        UnicodeWidthStr::width(grapheme).max(1)
    }
}

fn display_grapheme(grapheme: &str, position: usize, tab_mode: TabMode) -> String {
    if grapheme == "\t" {
        " ".repeat(grapheme_width_at(grapheme, position, tab_mode))
    } else {
        grapheme.to_string()
    }
}

#[cfg(test)]
fn string_width(value: &str, tab_mode: TabMode) -> usize {
    let mut width = 0;
    for grapheme in value.graphemes(true) {
        width += grapheme_width_at(grapheme, width, tab_mode);
    }
    width
}

fn true_position(line: &str, target_col: usize, tab_mode: TabMode) -> usize {
    let mut visual_pos = 0;
    let mut true_pos = 0;
    for grapheme in line.graphemes(true) {
        if visual_pos >= target_col {
            break;
        }
        visual_pos += grapheme_width_at(grapheme, visual_pos, tab_mode);
        true_pos += grapheme.chars().count();
    }
    true_pos
}

fn visual_slice(value: &str, max_width: usize, tab_mode: TabMode) -> String {
    let mut visual_pos = 0;
    let mut out = String::new();
    for grapheme in value.graphemes(true) {
        let width = grapheme_width_at(grapheme, visual_pos, tab_mode);
        if visual_pos + width > max_width {
            break;
        }
        out.push_str(grapheme);
        visual_pos += width;
    }
    if visual_pos < max_width {
        out.push_str(&" ".repeat(max_width - visual_pos));
    }
    out
}

fn expand_tabs(line: &str, tab_mode: TabMode) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::new();
    let mut pos = 0;
    for grapheme in line.graphemes(true) {
        if grapheme == "\t" {
            let width = grapheme_width_at(grapheme, pos, tab_mode);
            out.push_str(&" ".repeat(width));
            pos += width;
        } else {
            out.push_str(grapheme);
            pos += UnicodeWidthStr::width(grapheme).max(1);
        }
    }
    out
}

#[derive(Clone, Copy)]
enum Attr {
    Normal,
    Dim,
    Hint1,
    Hint2,
}

struct AnsiScreen {
    dim: String,
    hint1: String,
    hint2: String,
}

impl AnsiScreen {
    fn new(config: &MotionConfig) -> Self {
        Self {
            dim: format!("\x1b[{}m", config.dim),
            hint1: format!("\x1b[{}m", config.hint1_fg),
            hint2: format!("\x1b[{}m", config.hint2_fg),
        }
    }

    fn init(&mut self) -> Result<()> {
        print!("\x1b[?25l\x1b[2J");
        io::stdout().flush()?;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<()> {
        print!("\x1b[0m\x1b[?25h");
        io::stdout().flush()?;
        Ok(())
    }

    fn addstr(&mut self, y: usize, x: usize, text: &str, attr: Attr) -> Result<()> {
        let attr = match attr {
            Attr::Normal => "",
            Attr::Dim => &self.dim,
            Attr::Hint1 => &self.hint1,
            Attr::Hint2 => &self.hint2,
        };
        if attr.is_empty() {
            print!("\x1b[{};{}H{}", y + 1, x + 1, text);
        } else {
            print!("\x1b[{};{}H{}{}\x1b[0m", y + 1, x + 1, attr, text);
        }
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        io::stdout().flush()?;
        Ok(())
    }
}

struct RawMode {
    fd: i32,
    original: libc::termios,
}

impl RawMode {
    fn new() -> Result<Self> {
        let fd = io::stdin().as_raw_fd();
        let original = termios_for_fd(fd)?;
        let mut raw = original;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        set_termios(fd, &raw)?;
        Ok(Self { fd, original })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = set_termios(self.fd, &self.original);
    }
}

fn read_key(reader: &mut impl Read) -> Result<char> {
    let mut first = [0_u8; 1];
    reader.read_exact(&mut first)?;
    if first[0] == 0x03 {
        anyhow::bail!("cancelled");
    }
    let width = utf8_char_width(first[0]).context("invalid utf-8 input")?;
    let mut bytes = vec![first[0]];
    if width > 1 {
        let mut rest = vec![0_u8; width - 1];
        reader.read_exact(&mut rest)?;
        bytes.extend(rest);
    }
    let value = std::str::from_utf8(&bytes)?;
    value.chars().next().context("empty key input")
}

fn utf8_char_width(byte: u8) -> Option<usize> {
    match byte {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn termios_for_fd(fd: i32) -> Result<libc::termios> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    let status = unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) };
    if status != 0 {
        return Err(io::Error::last_os_error()).context("failed to read terminal mode");
    }
    Ok(unsafe { termios.assume_init() })
}

fn set_termios(fd: i32, termios: &libc::termios) -> Result<()> {
    let status = unsafe { libc::tcsetattr(fd, libc::TCSADRAIN, termios) };
    if status != 0 {
        return Err(io::Error::last_os_error()).context("failed to set terminal mode");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        assign_hints_by_distance, expand_tabs, find_matches, generate_hints, hint_positions,
        move_cursor, popup_command, read_key, smartsign_patterns, string_width,
        tmux_tab_mode_from_version, true_position, visual_slice, HintTarget, Match, MotionArgs,
        MotionKind, Pane, TabMode,
    };
    use crate::{tmux, types::CaseMode};
    use std::{
        env,
        ffi::OsString,
        process::Command,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
        thread,
        time::Duration,
    };

    static TMUX_ENV_LOCK: Mutex<()> = Mutex::new(());
    static NEXT_SOCKET_ID: AtomicUsize = AtomicUsize::new(0);

    fn pane(lines: &[&str]) -> Pane {
        Pane {
            window_id: "@1".into(),
            pane_id: "%1".into(),
            active: true,
            start_y: 0,
            height: 10,
            start_x: 0,
            width: 80,
            copy_mode: false,
            scroll_position: 0,
            cursor_y: 0,
            cursor_x: 0,
            lines: lines.iter().map(|line| (*line).to_string()).collect(),
        }
    }

    struct TmuxArgsGuard {
        previous: Option<OsString>,
    }

    impl TmuxArgsGuard {
        fn set(socket: &str) -> Self {
            let previous = env::var_os("THF_TMUX_ARGS");
            env::set_var("THF_TMUX_ARGS", format!("-L {socket}"));
            Self { previous }
        }
    }

    impl Drop for TmuxArgsGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                env::set_var("THF_TMUX_ARGS", previous);
            } else {
                env::remove_var("THF_TMUX_ARGS");
            }
        }
    }

    struct TestTmux {
        socket: String,
        pane_id: String,
        width: usize,
        height: usize,
    }

    impl TestTmux {
        fn start(command: &str, width: usize, height: usize) -> Option<Self> {
            if !tmux::have("tmux") {
                return None;
            }

            let socket = format!(
                "thf-motion-test-{}-{}",
                std::process::id(),
                NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
            );
            let output = Command::new("tmux")
                .arg("-L")
                .arg(&socket)
                .arg("new-session")
                .arg("-d")
                .arg("-x")
                .arg(width.to_string())
                .arg("-y")
                .arg(height.to_string())
                .arg("-P")
                .arg("-F")
                .arg("#{pane_id}")
                .arg(command)
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }

            let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if pane_id.is_empty() {
                return None;
            }

            Some(Self {
                socket,
                pane_id,
                width,
                height,
            })
        }

        fn direct_stdout(socket: &str, args: &[&str]) -> Option<String> {
            let output = Command::new("tmux")
                .arg("-L")
                .arg(socket)
                .args(args)
                .output()
                .ok()?;
            output.status.success().then(|| {
                String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string()
            })
        }

        fn stdout(&self, args: &[&str]) -> String {
            Self::direct_stdout(&self.socket, args)
                .unwrap_or_else(|| panic!("tmux command failed: {args:?}"))
        }

        fn split_window(&self, command: &str) -> String {
            self.stdout(&[
                "split-window",
                "-t",
                self.pane_id.as_str(),
                "-P",
                "-F",
                "#{pane_id}",
                command,
            ])
        }

        fn capture_lines(&self, pane_id: &str) -> Vec<String> {
            let output = self.stdout(&["capture-pane", "-p", "-t", pane_id]);
            output
                .strip_suffix('\n')
                .unwrap_or(&output)
                .split('\n')
                .take(self.height)
                .map(ToOwned::to_owned)
                .collect()
        }

        fn wait_for_lines(&self, pane_id: &str, needle: &str) -> Vec<String> {
            for _ in 0..50 {
                let lines = self.capture_lines(pane_id);
                if lines.iter().any(|line| line.contains(needle)) {
                    return lines;
                }
                thread::sleep(Duration::from_millis(20));
            }
            panic!("tmux pane did not contain {needle:?}");
        }

        fn pane(&self, pane_id: &str, lines: Vec<String>) -> Pane {
            let window_id = self.stdout(&["display-message", "-p", "-t", pane_id, "#{window_id}"]);
            Pane {
                window_id,
                pane_id: pane_id.to_string(),
                active: pane_id == self.active_pane(),
                start_y: 0,
                height: self.height,
                start_x: 0,
                width: self.width,
                copy_mode: false,
                scroll_position: 0,
                cursor_y: 0,
                cursor_x: 0,
                lines,
            }
        }

        fn active_pane(&self) -> String {
            self.stdout(&["list-panes", "-F", "#{pane_active}\t#{pane_id}"])
                .lines()
                .find_map(|line| {
                    let (active, pane_id) = line.split_once('\t')?;
                    (active == "1").then(|| pane_id.to_string())
                })
                .unwrap_or_default()
        }

        fn copy_cursor(&self, pane_id: &str) -> (usize, usize) {
            let output = self.stdout(&[
                "display-message",
                "-p",
                "-t",
                pane_id,
                "#{copy_cursor_x},#{copy_cursor_y}",
            ]);
            let (x, y) = output.split_once(',').expect("copy cursor position");
            (x.parse().unwrap(), y.parse().unwrap())
        }
    }

    impl Drop for TestTmux {
        fn drop(&mut self) {
            let _ = Command::new("tmux")
                .arg("-L")
                .arg(&self.socket)
                .arg("kill-server")
                .output();
        }
    }

    #[test]
    fn detects_tmux_tab_modes() {
        assert_eq!(tmux_tab_mode_from_version("tmux 3.5a"), TabMode::Fixed);
        assert_eq!(
            tmux_tab_mode_from_version("tmux 3.6"),
            TabMode::PositionAware
        );
        assert_eq!(
            tmux_tab_mode_from_version("tmux next-3.7"),
            TabMode::PositionAware
        );
        assert_eq!(tmux_tab_mode_from_version("tmux master"), TabMode::Fixed);
        assert_eq!(
            tmux_tab_mode_from_version("tmux openbsd-6.6"),
            TabMode::Fixed
        );
    }

    #[test]
    fn calculates_cjk_and_tab_widths() {
        assert_eq!(string_width("a", TabMode::PositionAware), 1);
        assert_eq!(string_width("你", TabMode::PositionAware), 2);
        assert_eq!(string_width("a\tb", TabMode::PositionAware), 9);
        assert_eq!(string_width("a\tb", TabMode::Fixed), 10);
        assert_eq!(expand_tabs("a\tb", TabMode::PositionAware), "a       b");
        assert_eq!(expand_tabs("a\tb", TabMode::Fixed), "a        b");
    }

    #[test]
    fn converts_visual_column_to_true_position() {
        assert_eq!(true_position("a你b", 0, TabMode::PositionAware), 0);
        assert_eq!(true_position("a你b", 1, TabMode::PositionAware), 1);
        assert_eq!(true_position("a你b", 3, TabMode::PositionAware), 2);
        assert_eq!(true_position("a\tb", 8, TabMode::PositionAware), 2);
        assert_eq!(true_position("a\tb", 8, TabMode::Fixed), 2);
        assert_eq!(true_position("a\tb", 9, TabMode::Fixed), 2);
    }

    #[test]
    fn visual_slice_pads_and_avoids_splitting_wide_chars() {
        assert_eq!(visual_slice("ab", 4, TabMode::PositionAware), "ab  ");
        assert_eq!(visual_slice("a你b", 2, TabMode::PositionAware), "a ");
        assert_eq!(visual_slice("a你b", 3, TabMode::PositionAware), "a你");
    }

    #[test]
    fn generates_non_ambiguous_hints() {
        assert_eq!(generate_hints("asdf", 4), vec!["a", "s", "d", "f"]);
        let hints = generate_hints("asdf", 7);
        assert_eq!(hints.len(), 7);
        assert!(!hints
            .iter()
            .any(|hint| hint.len() == 2 && hint.starts_with('a')));
    }

    #[test]
    fn generates_hints_beyond_two_keys() {
        let hints = generate_hints("ab", 7);

        assert_eq!(hints.len(), 7);
        assert!(hints.iter().any(|hint| hint.chars().count() == 3));
        for (idx, hint) in hints.iter().enumerate() {
            assert!(!hints
                .iter()
                .enumerate()
                .any(|(other_idx, other)| { idx != other_idx && other.starts_with(hint) }));
        }
    }

    #[test]
    fn generate_hints_ignores_duplicate_keys() {
        assert_eq!(generate_hints("aabb", 2), vec!["a", "b"]);
    }

    #[test]
    fn generate_hints_rejects_an_insufficient_alphabet() {
        assert!(generate_hints("a", 2).is_empty());
        assert!(generate_hints("aaaa", 3).is_empty());
    }

    #[test]
    fn read_key_reads_multibyte_utf8() {
        let mut input = "你".as_bytes();

        assert_eq!(read_key(&mut input).expect("utf-8 key"), '你');
    }

    #[test]
    fn expands_smartsign_patterns() {
        assert_eq!(smartsign_patterns("3", true), vec!["3", "#"]);
        assert_eq!(smartsign_patterns("3x", true), vec!["3x", "#x"]);
        assert_eq!(smartsign_patterns("ab", false), vec!["ab"]);
    }

    #[test]
    fn finds_matches_with_visual_columns() {
        let panes = vec![pane(&["a\tb", "你好 hello"])];
        let matches = find_matches(
            &panes,
            "b",
            CaseMode::Insensitive,
            false,
            TabMode::PositionAware,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].visual_col, 8);

        let matches = find_matches(&panes, "b", CaseMode::Insensitive, false, TabMode::Fixed);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].visual_col, 9);

        let matches = find_matches(
            &panes,
            "he",
            CaseMode::Insensitive,
            false,
            TabMode::PositionAware,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].visual_col, 5);
    }

    #[test]
    fn finds_matches_only_on_grapheme_boundaries() {
        let panes = vec![pane(&["👍🏽 thumbs", "👍 plain"])];

        let matches = find_matches(
            &panes,
            "🏽",
            CaseMode::Insensitive,
            false,
            TabMode::PositionAware,
        );
        assert!(matches.is_empty());

        let matches = find_matches(
            &panes,
            "👍",
            CaseMode::Insensitive,
            false,
            TabMode::PositionAware,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_no, 1);
        assert_eq!(matches[0].visual_col, 0);

        let matches = find_matches(
            &panes,
            "👍🏽",
            CaseMode::Insensitive,
            false,
            TabMode::PositionAware,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_no, 0);
        assert_eq!(matches[0].visual_col, 0);
    }

    #[test]
    fn hint_positions_restore_whole_graphemes() {
        let panes = vec![pane(&["👍🏽x"])];
        let positions = hint_positions(
            &panes,
            &[HintTarget {
                hint: "a".into(),
                target: Match {
                    pane_index: 0,
                    line_no: 0,
                    visual_col: 0,
                },
            }],
            TabMode::PositionAware,
        );

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].original, "👍🏽");
        assert_eq!(positions[0].next_original, "x");
        assert_eq!(positions[0].original_width, 2);
    }

    #[test]
    fn hint_positions_restore_expanded_tabs() {
        let panes = vec![pane(&["a\tb"])];
        let positions = hint_positions(
            &panes,
            &[HintTarget {
                hint: "a".into(),
                target: Match {
                    pane_index: 0,
                    line_no: 0,
                    visual_col: 1,
                },
            }],
            TabMode::PositionAware,
        );

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].original, "       ");
        assert_eq!(positions[0].next_original, "b");
        assert_eq!(positions[0].original_width, 7);
    }

    #[test]
    fn smartsign_matching_finds_shifted_symbol() {
        let panes = vec![pane(&["test 3# code"])];
        let matches = find_matches(
            &panes,
            "3",
            CaseMode::Sensitive,
            true,
            TabMode::PositionAware,
        );
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn assigns_short_hints_to_closer_matches() {
        let pane = pane(&["aaaaaaaaaa"]);
        let matches = vec![
            Match {
                pane_index: 0,
                line_no: 0,
                visual_col: 8,
            },
            Match {
                pane_index: 0,
                line_no: 0,
                visual_col: 1,
            },
        ];
        let panes = vec![pane];
        let targets = assign_hints_by_distance(&panes, &matches, 0, 0, "ab");
        assert_eq!(targets[0].target.visual_col, 1);
        assert_eq!(targets[0].hint, "a");
    }

    #[test]
    fn popup_command_targets_originating_client() {
        let args = MotionArgs {
            kind: MotionKind::S,
            pattern: None,
            query: None,
            query_option: None,
            target_window: None,
            target_client: None,
            overlay: false,
            case_mode: Some(CaseMode::Sensitive),
            smartsign: true,
            no_smartsign: false,
        };

        let command = popup_command(
            "/tmp/thf binary",
            &args,
            "a'b",
            "@1",
            "%1",
            Some("/dev/pts/1"),
        );

        assert_eq!(command[0], "display-popup");
        assert!(command.contains(&"-E".to_string()));
        assert!(command.contains(&"-B".to_string()));
        assert!(command.contains(&"-c".to_string()));
        assert!(command.contains(&"/dev/pts/1".to_string()));
        assert!(command.contains(&"%1".to_string()));
        assert!(!command[..command.len() - 1].contains(&"@1".to_string()));
        assert!(!command.iter().any(|part| part == "new-window"));
        let shell_command = command.last().expect("popup shell command");
        assert!(shell_command.contains("'/tmp/thf binary'"));
        assert!(shell_command.contains("--query 'a'\"'\"'b'"));
        assert!(shell_command.contains("--target-window @1"));
        assert!(shell_command.contains("--target-client /dev/pts/1"));
        assert!(shell_command.contains("--overlay"));
        assert!(shell_command.contains("--case sensitive"));
        assert!(shell_command.contains("--smartsign"));
    }

    #[test]
    fn move_cursor_positions_same_pane() {
        let _lock = TMUX_ENV_LOCK.lock().unwrap();
        let Some(server) =
            TestTmux::start("printf 'line0\\nline1\\nline2_target\\n'; sleep 60", 30, 10)
        else {
            return;
        };
        let _env = TmuxArgsGuard::set(&server.socket);
        let lines = server.wait_for_lines(&server.pane_id, "line2_target");
        let pane = server.pane(&server.pane_id, lines);

        move_cursor(&pane, 2, 7, None).expect("same-pane cursor move");

        assert_eq!(server.copy_cursor(&server.pane_id), (7, 2));
    }

    #[test]
    fn move_cursor_selects_cross_pane_target() {
        let _lock = TMUX_ENV_LOCK.lock().unwrap();
        let Some(server) = TestTmux::start("printf 'left\\n'; sleep 60", 40, 12) else {
            return;
        };
        let pane2 = server.split_window("printf 'line0\\nline1_target\\n'; sleep 60");
        server.stdout(&["select-pane", "-t", server.pane_id.as_str()]);
        let _env = TmuxArgsGuard::set(&server.socket);
        let lines = server.wait_for_lines(&pane2, "line1_target");
        let pane = server.pane(&pane2, lines);

        move_cursor(&pane, 1, 6, None).expect("cross-pane cursor move");

        assert_eq!(server.active_pane(), pane2);
        assert_eq!(server.copy_cursor(&pane.pane_id), (6, 1));
    }

    #[test]
    fn move_cursor_handles_leading_empty_rows() {
        let _lock = TMUX_ENV_LOCK.lock().unwrap();
        let Some(server) = TestTmux::start("printf '\\n\\nleading_target\\n'; sleep 60", 40, 10)
        else {
            return;
        };
        let _env = TmuxArgsGuard::set(&server.socket);
        let lines = server.wait_for_lines(&server.pane_id, "leading_target");
        let target_line = lines
            .iter()
            .position(|line| line.contains("leading_target"))
            .expect("target line");
        let pane = server.pane(&server.pane_id, lines);

        move_cursor(&pane, target_line, 0, None).expect("leading-empty cursor move");

        assert_eq!(server.copy_cursor(&server.pane_id), (0, target_line));
    }
}
