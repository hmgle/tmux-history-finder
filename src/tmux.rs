use std::{
    ffi::{OsStr, OsString},
    io::Write,
    process::{Command, Output, Stdio},
};

use anyhow::{Context, Result};

pub fn have(program: &str) -> bool {
    which::which(program).is_ok()
}

fn tmux_args_from_env() -> Vec<OsString> {
    let Some(raw) = std::env::var_os("THF_TMUX_ARGS") else {
        return Vec::new();
    };
    let raw = raw.to_string_lossy();
    if raw.trim().is_empty() {
        return Vec::new();
    }
    match shell_words::split(&raw) {
        Ok(parts) => parts.into_iter().map(OsString::from).collect(),
        Err(_) => raw.split_whitespace().map(OsString::from).collect(),
    }
}

pub fn command() -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(tmux_args_from_env());
    cmd
}

pub fn output<I, S>(args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = command()
        .args(args)
        .output()
        .context("failed to execute tmux")?;
    Ok(output)
}

pub fn stdout<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = output(args)?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn try_stdout<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = output(args).ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string()
    })
}

pub fn run<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = output(args)?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

pub fn run_ignore<I, S>(args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let _ = output(args);
}

pub fn display_message(message: &str) {
    if have("tmux") {
        run_ignore(["display-message", message]);
    } else {
        eprintln!("{message}");
    }
}

pub fn show_option(name: &str) -> Option<String> {
    try_stdout(["show-option", "-gqv", name]).filter(|value| !value.is_empty())
}

pub fn show_options(prefix: &str) -> Vec<(String, String)> {
    let Ok(output) = stdout(["show-options", "-gq"]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(parse_show_option_line)
        .filter(|(name, _)| name.starts_with(prefix))
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

fn parse_show_option_line(line: &str) -> Option<(String, String)> {
    let (name, value) = line.split_once(' ')?;
    Some((name.to_string(), unquote_option_value(value)))
}

fn unquote_option_value(value: &str) -> String {
    shell_words::split(value)
        .ok()
        .and_then(|mut parts| (parts.len() == 1).then(|| parts.remove(0)))
        .unwrap_or_else(|| value.to_string())
}

pub fn command_version(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn write_to_command(program: &str, args: &[&str], input: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {program}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .with_context(|| format!("failed to write to {program}"))?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("{program} exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_show_option_line;

    #[test]
    fn parses_unquoted_tmux_option() {
        assert_eq!(
            parse_show_option_line("@tmux_history_finder_scope all"),
            Some(("@tmux_history_finder_scope".into(), "all".into()))
        );
    }

    #[test]
    fn parses_quoted_tmux_option() {
        assert_eq!(
            parse_show_option_line("@tmux_history_finder_fzf_options \"--height 80%\""),
            Some((
                "@tmux_history_finder_fzf_options".into(),
                "--height 80%".into()
            ))
        );
    }
}
