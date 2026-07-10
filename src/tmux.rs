use std::{
    ffi::{OsStr, OsString},
    io::Write,
    process::{Command, Output, Stdio},
};

use anyhow::{Context, Result};

pub fn have(program: &str) -> bool {
    which::which(program).is_ok()
}

#[derive(Clone, Debug, Default)]
pub struct TmuxClient {
    args: Vec<OsString>,
}

impl TmuxClient {
    pub fn from_env() -> Result<Self> {
        Ok(Self::with_args(tmux_args_from_env()?))
    }

    pub fn with_args<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new("tmux");
        command.args(&self.args);
        command
    }

    pub fn output<I, S>(&self, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<OsString> = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect();
        self.command()
            .args(&args)
            .output()
            .with_context(|| format!("failed to execute tmux {}", display_args(&args)))
    }

    pub fn stdout<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output(args)?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn try_stdout<I, S>(&self, args: I) -> Option<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output(args).ok()?;
        output.status.success().then(|| {
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string()
        })
    }

    pub fn run<I, S>(&self, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output(args)?;
        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

    pub fn run_ignore<I, S>(&self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let _ = self.output(args);
    }
}

fn display_args(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn tmux_args_from_env() -> Result<Vec<OsString>> {
    let Some(raw) = std::env::var_os("THF_TMUX_ARGS") else {
        return Ok(Vec::new());
    };
    let raw = raw.to_string_lossy();
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    shell_words::split(&raw)
        .context("failed to parse THF_TMUX_ARGS")
        .map(|parts| parts.into_iter().map(OsString::from).collect())
}

pub fn stdout<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    TmuxClient::from_env()?.stdout(args)
}

pub fn try_stdout<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    TmuxClient::from_env().ok()?.try_stdout(args)
}

pub fn run<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    TmuxClient::from_env()?.run(args)
}

pub fn run_ignore<I, S>(args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    if let Ok(client) = TmuxClient::from_env() {
        client.run_ignore(args);
    }
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

pub fn show_options(prefix: &str) -> Result<Vec<(String, String)>> {
    let output = stdout(["show-options", "-gq"])?;
    Ok(output
        .lines()
        .filter_map(parse_show_option_line)
        .filter(|(name, _)| name.starts_with(prefix))
        .filter(|(_, value)| !value.is_empty())
        .collect())
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
