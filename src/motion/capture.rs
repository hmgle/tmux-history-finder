use std::{
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{Match, Pane};
use crate::tmux;

#[derive(Serialize, Deserialize)]
pub(super) struct MotionSnapshot {
    pub(super) panes: Vec<Pane>,
    pub(super) matches: Vec<Match>,
}

impl MotionSnapshot {
    pub(super) fn save(&self, path: &Path) -> Result<()> {
        let file =
            File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, self)?;
        writer.flush()?;
        Ok(())
    }

    pub(super) fn load(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("failed to parse {}", path.display()))
    }
}

pub(super) fn init_panes(target_window: Option<&str>) -> Result<Vec<Pane>> {
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

pub(super) fn window_size(target_window: Option<&str>) -> Result<(usize, usize)> {
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
