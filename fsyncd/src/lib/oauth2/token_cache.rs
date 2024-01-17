use std::fmt::Debug;

use chrono::{DateTime, Utc};
use fsync::path::{FsPath, FsPathBuf};
use oauth2::{AccessToken, RefreshToken, Scope, TokenResponse, TokenType};
use serde::{Deserialize, Serialize};

use crate::PersistCache;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenMapEntry<T> {
    scopes_hash: u64,
    scopes: Vec<Scope>,
    token: T,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenMap<T> {
    entries: Vec<TokenMapEntry<T>>,
}

impl<T> TokenMap<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, scopes: Vec<Scope>, token: T) {
        let mut scopes = scopes;
        scopes.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));

        let scopes_hash = {
            use std::hash::{Hash, Hasher};
            let mut state = std::collections::hash_map::DefaultHasher::new();
            scopes.hash(&mut state);
            state.finish()
        };
        let entry = TokenMapEntry {
            scopes_hash,
            scopes,
            token,
        };
        self.emplace_entry(entry);
    }

    fn emplace_entry(&mut self, token: TokenMapEntry<T>) {
        for ent in self.entries.iter_mut() {
            if ent.scopes_hash == token.scopes_hash {
                *ent = token;
                return;
            }
        }
        self.entries.push(token);
    }

    /// Returns an iterator over all entries that contain all required scopes
    pub fn get<'a, 'b>(
        &'a self,
        scopes: &'b [Scope],
    ) -> impl Iterator<Item = (&'a T, &'a [Scope])> + 'a
    where
        'b: 'a,
    {
        self.entries
            .iter()
            .filter(|&ent| scopes.iter().all(|s| ent.scopes.contains(s)))
            .map(move |ent| (&ent.token, &ent.scopes[..]))
    }
}

/// Specifies how the cache should persist tokens
#[derive(Debug, Clone)]
pub enum TokenPersist {
    /// No persistence. Token is fetch at each request
    None,
    /// Persist in memory, but start from scratch
    /// each time the program starts
    Memory,
    /// Load from disk, when program starts.
    /// Persist in memory for the duration of the program.
    /// Saves to disk in PersistCache implementation.
    MemoryAndDisk(FsPathBuf),
}

impl TokenPersist {
    fn try_path(&self) -> Option<&FsPath> {
        match self {
            Self::MemoryAndDisk(path) => Some(path),
            _ => None,
        }
    }

    fn has_mem(&self) -> bool {
        match self {
            Self::None => false,
            Self::Memory => true,
            Self::MemoryAndDisk(_) => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheToken {
    access_token: AccessToken,
    refresh_token: Option<RefreshToken>,
    expiration: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub enum CacheResult {
    None,
    Expired(RefreshToken, Vec<Scope>),
    Ok(AccessToken),
}

#[derive(Debug)]
pub struct TokenCache {
    persist: TokenPersist,
    map: TokenMap<CacheToken>,
}

impl TokenCache {
    pub async fn new(persist: TokenPersist) -> anyhow::Result<Self> {
        let map: Option<TokenMap<CacheToken>> = if let Some(path) = persist.try_path() {
            log::info!("reading cached tokens from {path}");
            let json = tokio::fs::read_to_string(path).await;
            if let Ok(json) = json {
                serde_json::from_str(&json)?
            } else {
                None
            }
        } else {
            None
        };
        let map = map.unwrap_or_else(|| TokenMap {
            entries: Vec::new(),
        });
        Ok(Self { persist, map })
    }

    pub fn put<T, TT>(&mut self, tok: &T)
    where
        T: TokenResponse<TT>,
        TT: TokenType,
    {
        if !self.persist.has_mem() {
            return;
        }
        log::trace!(
            "Put token for scopes {:?}, expires in {:?}",
            tok.scopes(),
            tok.expires_in()
        );

        let scopes = tok.scopes().cloned().unwrap_or_default();

        let tok = CacheToken {
            access_token: tok.access_token().clone(),
            refresh_token: tok.refresh_token().cloned(),
            expiration: tok.expires_in().map(|dur| Utc::now() + dur),
        };

        self.map.insert(scopes, tok);
    }

    pub fn check(&self, scopes: &[Scope]) -> CacheResult {
        if !self.persist.has_mem() {
            return CacheResult::None;
        }
        let res = self.map.get(scopes).next().map(|(tok, scopes)| {
            if let Some(expiration) = &tok.expiration {
                if *expiration < Utc::now() {
                    if let Some(refresh_token) = &tok.refresh_token {
                        CacheResult::Expired(refresh_token.clone(), scopes.to_vec())
                    } else {
                        CacheResult::None
                    }
                } else {
                    CacheResult::Ok(tok.access_token.clone())
                }
            } else {
                CacheResult::Ok(tok.access_token.clone())
            }
        });

        let res = res.unwrap_or(CacheResult::None);

        if log::max_level() >= log::LevelFilter::Trace {
            let res = match &res {
                CacheResult::None => "None",
                CacheResult::Expired(..) => "Expired",
                CacheResult::Ok(..) => "Ok",
            };
            let scopes: String = scopes
                .iter()
                .map(|s| s.as_str())
                .intersperse(", ")
                .collect();
            log::trace!("check token for scopes {scopes}: {res}");
        }

        res
    }
}

impl PersistCache for TokenCache {
    async fn persist_cache(&self) -> anyhow::Result<()> {
        if let Some(path) = self.persist.try_path() {
            log::info!("caching tokens to {path}");
            let json = serde_json::to_string_pretty(&self.map)?;
            tokio::fs::write(path, json).await?;
        }
        Ok(())
    }
}
