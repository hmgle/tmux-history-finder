use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tempfile::NamedTempFile;

use super::{choose_with_preview, command, selected_rows, ManagerContext, PreviewSpec, Row};
use crate::tmux;

const COPYQ_MAX_ITEMS: usize = 1_000;
const COPYQ_MAX_BYTES: usize = 64 * 1024 * 1024;
const COPYQ_MAX_RECORD_BYTES: usize = COPYQ_MAX_BYTES.div_ceil(3) * 4 + 32;
const COPYQ_MAX_INTERVAL_MS: u64 = 400;
const CLIPBOARD_PREVIEW_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
struct ClipboardEntry {
    source_index: usize,
    summary: String,
    offset: u64,
    length: u64,
}

#[derive(Debug)]
struct ClipboardSnapshot {
    entries: Vec<ClipboardEntry>,
    data: NamedTempFile,
}

pub(super) fn run(action: Option<&str>, context: &ManagerContext) -> Result<()> {
    let mut use_copyq = action != Some("buffer") && tmux::have("copyq");
    let mut snapshot = None;
    if use_copyq {
        match load_copyq_snapshot() {
            Ok(loaded) => snapshot = Some(loaded),
            Err(first_error) => {
                // A cold CopyQ server may simply not be running yet, which this
                // first failure cannot distinguish from a real snapshot problem
                // (oversized, malformed, or an I/O error). If the server already
                // answers, restarting it and re-downloading would not help, so
                // keep the original error instead of paying for a retry loop.
                let result = if copyq_ready() {
                    Err(first_error)
                } else {
                    start_copyq_and_snapshot(context)
                };
                match result {
                    Ok(loaded) => snapshot = Some(loaded),
                    Err(error) if action == Some("system") => {
                        return Err(error).context(
                            "CopyQ is installed but its clipboard service or snapshot is unavailable",
                        );
                    }
                    Err(_) => use_copyq = false,
                }
            }
        }
    }
    if use_copyq {
        let mut snapshot = snapshot.context("CopyQ snapshot is unavailable")?;
        let rows: Vec<Row> = snapshot
            .entries
            .iter()
            .map(|entry| Row::new(entry.source_index.to_string(), &entry.summary))
            .collect();
        let spans = snapshot
            .entries
            .iter()
            .map(|entry| (entry.offset, entry.length))
            .collect::<Vec<_>>();
        let selected = choose_with_preview(
            &rows,
            context,
            "clipboard> ",
            "TAB selects multiple clipboard entries",
            true,
            Some(PreviewSpec::Blob {
                path: snapshot.data.path(),
                entries: &spans,
            }),
        )?;
        if !selected.is_empty() {
            let content = read_clipboard_entries(&mut snapshot, &selected)?;
            paste_bytes(context, &content)?;
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

fn load_copyq_snapshot() -> Result<ClipboardSnapshot> {
    let script = format!(
        "var limit = Math.min(size(), {COPYQ_MAX_ITEMS}); \
         for (var i = 0; i < limit; ++i) {{ \
         print(i + '\\t' + toBase64(read(i)) + '\\n'); }}"
    );
    let mut child = Command::new("copyq")
        .args(["eval", "--", script.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start copyq")?;
    let stdout = child.stdout.take().context("CopyQ stdout is unavailable")?;
    let mut stderr = child.stderr.take().context("CopyQ stderr is unavailable")?;
    let stderr_reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stderr.read_to_end(&mut output).map(|_| output)
    });
    let snapshot = parse_copyq_snapshot_reader(BufReader::new(stdout));
    if snapshot.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("CopyQ stderr reader panicked"))??;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(error).with_context(|| {
                let stderr = String::from_utf8_lossy(&stderr);
                format!("failed to parse CopyQ snapshot: {}", stderr.trim())
            })
        }
    };
    if !status.success() {
        anyhow::bail!("copyq failed: {}", String::from_utf8_lossy(&stderr).trim());
    }
    Ok(snapshot)
}

#[cfg(test)]
fn parse_copyq_snapshot(output: &[u8]) -> Result<ClipboardSnapshot> {
    parse_copyq_snapshot_reader(std::io::Cursor::new(output))
}

fn parse_copyq_snapshot_reader(mut reader: impl BufRead) -> Result<ClipboardSnapshot> {
    let mut data = NamedTempFile::new()?;
    let mut entries = Vec::new();
    let mut offset = 0_u64;
    let mut record = Vec::new();
    loop {
        record.clear();
        let count = reader
            .by_ref()
            .take((COPYQ_MAX_RECORD_BYTES + 1) as u64)
            .read_until(b'\n', &mut record)
            .context("failed to read CopyQ snapshot")?;
        if count == 0 {
            break;
        }
        if record.len() > COPYQ_MAX_RECORD_BYTES {
            anyhow::bail!("CopyQ snapshot contains an oversized record");
        }
        if record.last() == Some(&b'\n') {
            record.pop();
        }
        if record.last() == Some(&b'\r') {
            record.pop();
        }
        let line =
            std::str::from_utf8(&record).context("CopyQ snapshot is not UTF-8 base64 data")?;
        let (source_index, encoded_content) = line
            .split_once('\t')
            .context("CopyQ snapshot record is missing its delimiter")?;
        let source_index = source_index
            .parse()
            .context("CopyQ snapshot contains an invalid source index")?;
        let content = BASE64
            .decode(encoded_content)
            .context("CopyQ snapshot contains invalid base64 content")?;
        let next_size = usize::try_from(offset)
            .ok()
            .and_then(|size| size.checked_add(content.len()))
            .context("CopyQ snapshot size overflow")?;
        if next_size > COPYQ_MAX_BYTES {
            anyhow::bail!(
                "CopyQ snapshot exceeds the {} MiB safety limit",
                COPYQ_MAX_BYTES / 1024 / 1024
            );
        }
        data.write_all(&content)?;
        entries.push(ClipboardEntry {
            source_index,
            summary: clipboard_summary(&content),
            offset,
            length: content.len() as u64,
        });
        offset = offset
            .checked_add(content.len() as u64)
            .context("CopyQ snapshot offset overflow")?;
    }
    data.flush()?;
    Ok(ClipboardSnapshot { entries, data })
}

fn start_copyq_and_snapshot(context: &ManagerContext) -> Result<ClipboardSnapshot> {
    let mut launcher = Command::new("copyq")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start the CopyQ server")?;
    wait_for_copyq_snapshot(
        context.config.copyq_start_attempts,
        context.config.copyq_start_interval_ms,
        copyq_ready,
        load_copyq_snapshot,
        // The server we just started can exit without ever becoming reachable
        // (typical in headless or display-less environments): spawning succeeds
        // because the OS creates the process before CopyQ discovers it has no
        // display and quits, so a successful spawn() is not enough on its own.
        || matches!(launcher.try_wait(), Ok(Some(_))),
        |interval| std::thread::sleep(std::time::Duration::from_millis(interval)),
    )
}

/// Drive the post-launch retry loop: probe with a cheap command first so a
/// full snapshot is only requested once the server is actually answering
/// (this also attributes failures to "server not ready" rather than
/// "snapshot unparsable"), back off exponentially between probes, and stop
/// early when the launcher has already exited because nothing is left to
/// wait for.
fn wait_for_copyq_snapshot<T>(
    attempts: usize,
    initial_interval: u64,
    mut ready: impl FnMut() -> bool,
    mut load: impl FnMut() -> Result<T>,
    mut launcher_exited: impl FnMut() -> bool,
    mut sleep: impl FnMut(u64),
) -> Result<T> {
    let mut interval = initial_interval.min(COPYQ_MAX_INTERVAL_MS);
    let mut last_error = None;
    for attempt in 0..attempts.max(1) {
        if attempt > 0 {
            sleep(interval);
            interval = next_backoff_interval(interval);
        }
        if ready() {
            match load() {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => last_error = Some(error),
            }
        } else if launcher_exited() {
            return Err(last_error.unwrap_or_else(|| {
                anyhow::anyhow!("the CopyQ server exited before it became available")
            }));
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("CopyQ did not become available")))
}

fn next_backoff_interval(interval: u64) -> u64 {
    interval.saturating_mul(2).min(COPYQ_MAX_INTERVAL_MS)
}

fn copyq_ready() -> bool {
    Command::new("copyq")
        .args(["eval", "--", "size()"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn clipboard_summary(content: &[u8]) -> String {
    let text = String::from_utf8_lossy(content);
    let preview = sanitize_preview_text(&text.chars().take(512).collect::<String>());
    let summary = one_line(&preview);
    if summary.is_empty() {
        "[empty]".into()
    } else {
        summary
    }
}

fn read_clipboard_entries(snapshot: &mut ClipboardSnapshot, indexes: &[usize]) -> Result<Vec<u8>> {
    let total = indexes.iter().try_fold(0_usize, |total, index| {
        let length = usize::try_from(
            snapshot
                .entries
                .get(*index)
                .context("clipboard selection is out of range")?
                .length,
        )
        .context("clipboard entry is too large")?;
        total
            .checked_add(length)
            .context("clipboard paste is too large")
    })?;
    let mut result = Vec::with_capacity(total);
    for index in indexes {
        let entry = snapshot
            .entries
            .get(*index)
            .context("clipboard selection is out of range")?;
        snapshot
            .data
            .as_file_mut()
            .seek(SeekFrom::Start(entry.offset))?;
        let start = result.len();
        result.resize(start + entry.length as usize, 0);
        snapshot
            .data
            .as_file_mut()
            .read_exact(&mut result[start..])?;
    }
    Ok(result)
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

pub(super) fn print_blob_preview(path: &Path, offset: u64, length: u64) -> Result<()> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open preview data {}", path.display()))?;
    let file_size = file.metadata()?.len();
    let end = offset
        .checked_add(length)
        .context("preview byte range overflow")?;
    if end > file_size {
        anyhow::bail!("preview byte range is outside the snapshot");
    }
    let preview_length = length.min(CLIPBOARD_PREVIEW_BYTES as u64) as usize;
    let mut content = vec![0; preview_length];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut content)?;
    match std::str::from_utf8(&content) {
        Ok(text) => print!("{}", sanitize_preview_text(text)),
        Err(_) => {
            println!("[binary clipboard entry: {length} bytes]");
            print_hex_preview(&content[..content.len().min(4096)]);
        }
    }
    if length > preview_length as u64 {
        println!(
            "\n[preview truncated at {} KiB of {} KiB]",
            preview_length / 1024,
            length.div_ceil(1024)
        );
    }
    Ok(())
}

fn sanitize_preview_text(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\t' => character,
            _ if character.is_control() => '�',
            _ => character,
        })
        .collect()
}

fn print_hex_preview(content: &[u8]) {
    for (offset, chunk) in content.chunks(16).enumerate() {
        print!("{:08x}  ", offset * 16);
        for byte in chunk {
            print!("{byte:02x} ");
        }
        println!();
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::{
        next_backoff_interval, parse_copyq_snapshot, print_blob_preview, read_clipboard_entries,
        sanitize_preview_text, wait_for_copyq_snapshot, COPYQ_MAX_INTERVAL_MS,
    };

    #[test]
    fn backoff_doubles_then_saturates_at_the_cap() {
        assert_eq!(next_backoff_interval(25), 50);
        assert_eq!(next_backoff_interval(300), COPYQ_MAX_INTERVAL_MS);
        assert_eq!(next_backoff_interval(u64::MAX), COPYQ_MAX_INTERVAL_MS);
    }

    #[test]
    fn copyq_wait_caps_the_configured_initial_interval() {
        let mut sleeps = Vec::new();
        let result = wait_for_copyq_snapshot(
            3,
            u64::MAX,
            || false,
            || Ok(()),
            || false,
            |interval| sleeps.push(interval),
        );
        assert!(result.is_err());
        assert_eq!(sleeps, vec![COPYQ_MAX_INTERVAL_MS; 2]);
    }

    #[test]
    fn copyq_wait_snapshots_once_the_server_answers() {
        let mut probes = 0;
        let mut loads = 0;
        let mut sleeps = Vec::new();
        let result = wait_for_copyq_snapshot(
            10,
            25,
            || {
                probes += 1;
                probes >= 3
            },
            || {
                loads += 1;
                Ok("snapshot")
            },
            || false,
            |interval| sleeps.push(interval),
        );
        assert_eq!(result.unwrap(), "snapshot");
        assert_eq!(probes, 3);
        assert_eq!(loads, 1);
        assert_eq!(sleeps, vec![25, 50]);
    }

    #[test]
    fn copyq_wait_stops_as_soon_as_the_launcher_exits() {
        let probes = std::cell::Cell::new(0);
        let mut sleeps = 0;
        let result = wait_for_copyq_snapshot(
            10,
            25,
            || {
                probes.set(probes.get() + 1);
                false
            },
            || -> anyhow::Result<()> { panic!("must not snapshot before the probe succeeds") },
            || probes.get() >= 2,
            |_| sleeps += 1,
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exited before it became available"));
        assert_eq!(probes.get(), 2);
        assert_eq!(sleeps, 1);
    }

    #[test]
    fn copyq_wait_reports_the_last_snapshot_error() {
        let mut loads = 0;
        let result: anyhow::Result<()> = wait_for_copyq_snapshot(
            3,
            25,
            || true,
            || {
                loads += 1;
                Err(anyhow::anyhow!("snapshot failure {loads}"))
            },
            || false,
            |_| {},
        );
        assert_eq!(loads, 3);
        assert_eq!(result.unwrap_err().to_string(), "snapshot failure 3");
    }

    #[test]
    fn copyq_wait_runs_at_least_one_attempt() {
        let mut probes = 0;
        let result = wait_for_copyq_snapshot(
            0,
            25,
            || {
                probes += 1;
                true
            },
            || Ok(()),
            || false,
            |_| panic!("a single attempt must not sleep"),
        );
        assert!(result.is_ok());
        assert_eq!(probes, 1);
    }

    #[test]
    fn copyq_snapshot_preserves_empty_multiline_and_binary_entries() {
        let mut snapshot = parse_copyq_snapshot(b"0\taGVsbG8Kd29ybGQ=\n1\t\n2\tAP+A\n").unwrap();
        assert_eq!(snapshot.entries.len(), 3);
        assert_eq!(snapshot.entries[0].summary, "hello world");
        assert_eq!(snapshot.entries[1].summary, "[empty]");
        assert_eq!(snapshot.entries[2].summary, "���");
        assert_eq!(
            read_clipboard_entries(&mut snapshot, &[2, 0]).unwrap(),
            b"\0\xff\x80hello\nworld"
        );
    }

    #[test]
    fn preview_removes_terminal_control_sequences() {
        assert_eq!(
            sanitize_preview_text("safe\n\x1b[31mred\t\x07"),
            "safe\n�[31mred\t�"
        );
    }

    #[test]
    fn copyq_snapshot_rejects_invalid_and_truncated_base64() {
        assert!(parse_copyq_snapshot(b"0\t%%%\n").is_err());
        assert!(parse_copyq_snapshot(b"0\taGVsbG8\n").is_err());
    }

    #[test]
    fn blob_preview_rejects_ranges_outside_the_snapshot() {
        let mut data = NamedTempFile::new().unwrap();
        data.write_all(b"hello").unwrap();
        data.flush().unwrap();
        assert!(print_blob_preview(data.path(), 4, 2).is_err());
        assert!(print_blob_preview(data.path(), u64::MAX, 1).is_err());
    }
}
