use camino::Utf8Path;
use glob::{MatchOptions, Pattern, PatternError};

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

    pub fn matches_with<P: AsRef<Utf8Path>>(&self, path: P) -> bool {
        self.0
            .iter()
            .any(|p| p.matches_with(path.as_ref().as_str(), self.1))
    }
}

impl Default for PatternList {
    fn default() -> Self {
        PatternList(Vec::new(), MatchOptions::default())
    }
}
