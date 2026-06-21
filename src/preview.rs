use anyhow::Result;

use crate::{
    index::{LegacyRecord, PaneSnapshot, Record, SearchIndex},
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

    print_window(pane, record);
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
        session: record
            .location
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string(),
        window_index: String::new(),
        pane_index: String::new(),
        pane_id: record.pane_id.clone(),
        command: record.command.clone(),
        window_name: record.window_name.clone(),
        lines,
    };
    let preview_record = Record {
        id: 0,
        pane_index: 0,
        raw_line_no: target + 1,
        logical_line_no: target + 1,
        location: record.location.clone(),
        text: record.text.clone(),
        before: None,
        after: None,
    };
    print_window(&pane, &preview_record);
    Ok(())
}

fn print_window(pane: &PaneSnapshot, record: &Record) {
    if pane.lines.is_empty() {
        println!("(no pane content)");
        return;
    }

    let target = record
        .raw_line_no
        .saturating_sub(1)
        .min(pane.lines.len() - 1);
    let half = 10usize;
    let start = target.saturating_sub(half);
    let end = (target + half + 1).min(pane.lines.len());

    println!(
        "\x1b[1;36m{}\x1b[0m  \x1b[2m({})\x1b[0m",
        record.location, pane.command
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
