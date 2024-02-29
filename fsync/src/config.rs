use anyhow::Context;
use glob::{MatchOptions, Pattern, PatternError};
use serde::{Deserialize, Serialize};

use crate::path::{FsPath, FsPathBuf, Path};

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub local_dir: FsPathBuf,
    pub provider: ProviderConfig,
}

impl Config {
    pub async fn load_from_file(path: &FsPath) -> anyhow::Result<Self> {
        let config_json = tokio::fs::read(&path)
            .await
            .with_context(|| format!("Failed to read config from {path}"))?;
        let config_json = std::str::from_utf8(&config_json)?;
        Ok(serde_json::from_str(config_json)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProviderConfig {
    GoogleDrive(drive::Config),
    LocalFs(FsPathBuf),
}

pub mod drive {
    use serde::{Deserialize, Serialize};

    use crate::{oauth2, path::PathBuf};

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Config {
        pub root: Option<PathBuf>,
        pub secret: oauth2::Secret,
    }
}
