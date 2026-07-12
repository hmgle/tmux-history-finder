use std::{
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};

use super::{command, selected_rows, ManagerContext, Row};
use crate::tmux;

pub(super) fn run(action: Option<&str>, context: &ManagerContext) -> Result<()> {
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
        let mut rows = Vec::with_capacity(count);
        for index in 0..count {
            let id = index.to_string();
            let content = copyq_output(["read", id.as_str()])?;
            rows.push(Row::new(&id, one_line(&String::from_utf8_lossy(&content))));
        }
        let selected = selected_rows(
            &rows,
            context,
            "clipboard> ",
            "TAB selects multiple clipboard entries",
            true,
            Some("copyq"),
        )?;
        for row in selected {
            paste_bytes(context, &copyq_output(["read", row.id.as_str()])?)?;
        }
    } else {
        let output = context.tmux.stdout([
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
            context,
            "buffer> ",
            "TAB selects multiple tmux buffers",
            true,
            Some("buffer"),
        )?;
        let commands = selected
            .iter()
            .map(|row| command(["paste-buffer", "-b", row.id.as_str()]))
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            context.tmux.run_commands(&commands)?;
        }
    }
    Ok(())
}

pub(super) fn print_copyq_preview(index: &str) -> Result<()> {
    std::io::stdout().write_all(&copyq_output(["read", index])?)?;
    Ok(())
}

fn copyq_count() -> Result<usize> {
    let output = copyq_output(["count"])?;
    String::from_utf8_lossy(&output)
        .trim()
        .parse()
        .context("CopyQ returned an invalid item count")
}

fn copyq_output<const N: usize>(args: [&str; N]) -> Result<Vec<u8>> {
    let output = Command::new("copyq")
        .args(args)
        .output()
        .context("failed to execute CopyQ")?;
    if !output.status.success() {
        anyhow::bail!(
            "CopyQ failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn paste_bytes(context: &ManagerContext, content: &[u8]) -> Result<()> {
    let name = format!("tnx-{}", std::process::id());
    context.tmux.run_with_input(
        [
            "load-buffer",
            "-b",
            name.as_str(),
            "-",
            ";",
            "paste-buffer",
            "-b",
            name.as_str(),
            ";",
            "delete-buffer",
            "-b",
            name.as_str(),
        ],
        content,
    )
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
