use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
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
            Err(_) => {
                let _ = Command::new("copyq")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                std::thread::sleep(std::time::Duration::from_millis(150));
                match load_copyq_snapshot() {
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
    let output = command_stdout_bytes("copyq", ["eval", "--", script.as_str()])?;
    parse_copyq_snapshot(&output)
}

fn parse_copyq_snapshot(output: &[u8]) -> Result<ClipboardSnapshot> {
    let encoded = std::str::from_utf8(output).context("CopyQ snapshot is not UTF-8 base64 data")?;
    let mut data = NamedTempFile::new()?;
    let mut entries = Vec::new();
    let mut offset = 0_u64;
    for line in encoded.lines() {
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

#[cfg(test)]
mod tests {
    use super::{parse_copyq_snapshot, read_clipboard_entries, sanitize_preview_text};

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
}
