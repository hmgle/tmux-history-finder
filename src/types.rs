use std::{fmt, str::FromStr};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    All,
    Session,
    Pane,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Session => "session",
            Self::Pane => "pane",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Scope {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "session" | "current" => Ok(Self::Session),
            "pane" => Ok(Self::Pane),
            other => anyhow::bail!("unknown scope '{other}'"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    #[default]
    Jump,
    Copy,
    Send,
    Print,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jump => "jump",
            Self::Copy => "copy",
            Self::Send => "send",
            Self::Print => "print",
        }
    }
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ActionKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "jump" => Ok(Self::Jump),
            "copy" => Ok(Self::Copy),
            "send" => Ok(Self::Send),
            "print" => Ok(Self::Print),
            other => anyhow::bail!("unknown action '{other}'"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum CaseMode {
    #[default]
    Smart,
    Sensitive,
    Insensitive,
}

impl CaseMode {
    pub fn is_sensitive_for(self, query: &str) -> bool {
        match self {
            Self::Sensitive => true,
            Self::Insensitive => false,
            Self::Smart => query.chars().any(char::is_uppercase),
        }
    }
}

impl FromStr for CaseMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "smart" => Ok(Self::Smart),
            "sensitive" => Ok(Self::Sensitive),
            "insensitive" => Ok(Self::Insensitive),
            other => anyhow::bail!("unknown case mode '{other}'"),
        }
    }
}

impl fmt::Display for CaseMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Smart => "smart",
            Self::Sensitive => "sensitive",
            Self::Insensitive => "insensitive",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchMode {
    #[default]
    Literal,
    Regex,
}
