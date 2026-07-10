use anyhow::Result;

use crate::{
    index::{LegacyRecord, PaneSnapshot, SearchIndex},
    tmux,
    util::trim_prefix_chars,
};

pub fn print_index_preview(
    index: &SearchIndex,
    record_id: usize,
    _query: Option<&str>,
) -> Result<()> {
    let Some(record) = index.record(record_id) else {
        println!("(record not found)");
        return Ok(());
    };
    let Some(pane) = index.pane_for(record) else {
        println!("(pane not found)");
        return Ok(());
    };

    print_window(pane, record.line_index);
    Ok(())
}

pub fn print_pane_preview(pane: &PaneSnapshot, line_index: usize) -> Result<()> {
    print_window(pane, line_index);
    Ok(())
}

pub fn print_legacy_preview(record: &LegacyRecord) -> Result<()> {
    let output = tmux::stdout([
        "capture-pane",
        "-p",
        "-J",
        "-S",
        "-",
        "-E",
        "-",
        "-t",
        record.pane_id.as_str(),
    ])?;
    let lines: Vec<String> = output.lines().map(ToOwned::to_owned).collect();
    let needle = trim_prefix_chars(&record.text, 60);
    let target = if needle.is_empty() {
        record.line_no.saturating_sub(1)
    } else {
        lines
            .iter()
            .position(|line| line.contains(&needle))
            .unwrap_or_else(|| record.line_no.saturating_sub(1))
    };
    let pane = PaneSnapshot {
        location: record.location.clone(),
        pane_id: record.pane_id.clone(),
        command: record.command.clone(),
        window_name: record.window_name.clone(),
        history_start_line: 0,
        lines,
    };
    print_window(&pane, target);
    Ok(())
}

fn print_window(pane: &PaneSnapshot, line_index: usize) {
    if pane.lines.is_empty() {
        println!("(no pane content)");
        return;
    }

    let target = line_index.min(pane.lines.len() - 1);
    let half = 10usize;
    let start = target.saturating_sub(half);
    let end = (target + half + 1).min(pane.lines.len());

    println!(
        "\x1b[1;36m{}\x1b[0m  \x1b[2m({})\x1b[0m",
        pane.location(),
        pane.command
    );
    println!(
        "\x1b[2mlines {}-{} of {}\x1b[0m\n",
        start + 1,
        end,
        pane.lines.len()
    );

    for (idx, line) in pane.lines[start..end].iter().enumerate() {
        let line_no = start + idx + 1;
        if line_no == target + 1 {
            println!("\x1b[1;33m>{:6} \x1b[1;37m{}\x1b[0m", line_no, line);
        } else {
            println!(" {:6}  {}", line_no, line);
        }
    }
}
