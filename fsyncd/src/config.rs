use camino::Utf8Path;
use glob::{MatchOptions, Pattern, PatternError};

pub struct PatternList(Vec<Pattern>);

impl PatternList {
    pub fn new<I>(patterns: I) -> Result<PatternList, PatternError>
    where
        I: Iterator,
        I::Item: AsRef<str>,
    {
        let patterns: Result<Vec<_>, _> = patterns.map(|p| Pattern::new(p.as_ref())).collect();
        Ok(PatternList(patterns?))
    }

    fn matches_with<P: AsRef<Utf8Path>>(&self, path: P, opts: MatchOptions) -> bool {
        self.0
            .iter()
            .any(|p| p.matches_with(path.as_ref().as_str(), opts))
    }
}

pub struct Config {
    pub case_sensitive: bool,
    pub ignore: PatternList,
    pub ignore_local: PatternList,
    pub ignore_remote: PatternList,
}

impl Config {
    pub fn new() -> Config {
        Config {
            case_sensitive: false,
            ignore: PatternList(Vec::new()),
            ignore_local: PatternList(Vec::new()),
            ignore_remote: PatternList(Vec::new()),
        }
    }

    pub fn ignored_local<P: AsRef<Utf8Path>>(&self, path: P) -> bool {
        let opts = self.ignore_match_options();
        self.ignore_local.matches_with(&path, opts) || self.ignore.matches_with(&path, opts)
    }
}

impl Config {
    fn ignore_match_options(&self) -> MatchOptions {
        MatchOptions {
            case_sensitive: self.case_sensitive,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        }
    }
}
