use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub session: String,
    pub window_index: String,
    pub pane_index: String,
    pub pane_id: String,
    pub command: String,
    pub window_name: String,
    pub lines: Vec<String>,
}

impl PaneSnapshot {
    pub fn location(&self) -> String {
        format!("{}:{}.{}", self.session, self.window_index, self.pane_index)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: usize,
    pub pane_index: usize,
    pub raw_line_no: usize,
    pub logical_line_no: usize,
    pub location: String,
    pub text: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchIndex {
    pub version: u32,
    pub panes: Vec<PaneSnapshot>,
    pub records: Vec<Record>,
}

impl SearchIndex {
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_vec(self)?;
        fs::write(path, data).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_slice(&data).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn record(&self, id: usize) -> Option<&Record> {
        self.records.get(id).filter(|record| record.id == id)
    }

    pub fn pane_for(&self, record: &Record) -> Option<&PaneSnapshot> {
        self.panes.get(record.pane_index)
    }

    pub fn legacy_tsv(&self, record: &Record) -> Option<String> {
        let pane = self.pane_for(record)?;
        Some(format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            pane.pane_id,
            record.location,
            pane.command,
            pane.window_name,
            record.raw_line_no,
            record.text
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyRecord {
    pub pane_id: String,
    pub location: String,
    pub command: String,
    pub window_name: String,
    pub line_no: usize,
    pub text: String,
}

impl LegacyRecord {
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.splitn(6, '\t');
        Some(Self {
            pane_id: parts.next()?.to_string(),
            location: parts.next()?.to_string(),
            command: parts.next()?.to_string(),
            window_name: parts.next()?.to_string(),
            line_no: parts.next()?.parse().ok()?,
            text: parts
                .next()
                .unwrap_or_default()
                .trim_end_matches('\r')
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{LegacyRecord, Record, SearchIndex};

    #[test]
    fn parses_legacy_record_with_tabs_in_text() {
        let record = LegacyRecord::parse("%1\tmain:1.0\tzsh\tlogs\t7\talpha\tbeta").unwrap();
        assert_eq!(record.pane_id, "%1");
        assert_eq!(record.line_no, 7);
        assert_eq!(record.text, "alpha\tbeta");
    }

    #[test]
    fn record_lookup_uses_the_record_id_as_index() {
        let index = SearchIndex {
            records: vec![Record {
                id: 0,
                pane_index: 0,
                raw_line_no: 1,
                logical_line_no: 1,
                location: "s:1.0".into(),
                text: "alpha".into(),
                before: None,
                after: None,
            }],
            ..SearchIndex::default()
        };

        assert_eq!(
            index.record(0).map(|record| record.text.as_str()),
            Some("alpha")
        );
        assert!(index.record(1).is_none());
    }
}
