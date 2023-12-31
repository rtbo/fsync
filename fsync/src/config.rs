use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use glob::{MatchOptions, Pattern, PatternError};
use serde::{Deserialize, Serialize};

use crate::path::Path;

#[derive(Default)]
pub struct PatternList(Vec<Pattern>, MatchOptions);

impl PatternList {
    pub fn _new<I>(patterns: I, opts: MatchOptions) -> Result<PatternList, PatternError>
    where
        I: Iterator,
        I::Item: AsRef<str>,
    {
        let patterns: Result<Vec<_>, _> = patterns.map(|p| Pattern::new(p.as_ref())).collect();
        Ok(PatternList(patterns?, opts))
    }

    pub fn matches_with<P: AsRef<Path>>(&self, path: P) -> bool {
        self.0
            .iter()
            .any(|p| p.matches_with(path.as_ref().as_str(), self.1))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub local_dir: Utf8PathBuf,
    pub provider: crate::Provider,
}

impl Config {
    pub async fn load_from_file(path: &Utf8Path) -> anyhow::Result<Self> {
        let config_json = tokio::fs::read(&path)
            .await
            .with_context(|| format!("Failed to read config from {path}"))?;
        let config_json = std::str::from_utf8(&config_json)?;
        Ok(serde_json::from_str(config_json)?)
    }
}
