use chrono::{DateTime, Utc};
use fsync::path::{FsPath, FsPathBuf};
use oauth2::{AccessToken, RefreshToken, Scope, TokenResponse, TokenType};
use serde::{Deserialize, Serialize};

use crate::PersistCache;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenEntry {
    scopes_hash: u64,
    scopes: Vec<Scope>,
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

#[derive(Debug, Default)]
pub struct TokenStore {
    entries: Vec<TokenEntry>,
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Attempts to read the cache from disk
    /// Returns `Ok(None)` if the path doesn't exist.
    /// Returns `Ok(Some)` if succesfully reads entries.
    /// Returns `Err` if the deserialization failed.
    async fn try_read_from_disk(path: &FsPath) -> anyhow::Result<Option<Self>> {
        log::info!("reading cached tokens from {path}");
        let json = tokio::fs::read_to_string(path).await;
        if json.is_err() {
            return Ok(None);
        }
        let json = json.unwrap();
        let entries = serde_json::from_str(&json)?;
        Ok(Some(TokenStore { entries }))
    }

    async fn write_to_disk(&self, path: &FsPath) -> anyhow::Result<()> {
        log::info!("caching tokens to {path}");
        let json = serde_json::to_string_pretty(&self.entries)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }

    pub fn insert<T, TT>(&mut self, tok: &T)
    where
        T: TokenResponse<TT>,
        TT: TokenType,
    {
        let scopes = {
            let mut scopes = tok.scopes().cloned().unwrap_or_default();
            scopes.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
            scopes
        };
        log::trace!(target: "fsyncd::oauth2::TokenStore", "inserting token for scopes {scopes:#?}");

        let scopes_hash = {
            use std::hash::{Hash, Hasher};
            let mut state = std::collections::hash_map::DefaultHasher::new();
            scopes.hash(&mut state);
            state.finish()
        };
        let expiration = tok.expires_in().map(|exp| Utc::now() + exp);
        let entry = TokenEntry {
            scopes_hash,
            scopes,
            access_token: tok.access_token().clone(),
            refresh_token: tok.refresh_token().cloned(),
            expiration,
        };
        self.emplace_entry(entry);
    }

    fn emplace_entry(&mut self, token: TokenEntry) {
        for ent in self.entries.iter_mut() {
            if ent.scopes_hash == token.scopes_hash {
                *ent = token;
                return;
            }
        }
        self.entries.push(token);
    }

    pub fn get(&self, scopes: &[Scope]) -> CacheResult {
        for ent in self.entries.iter() {
            if !scopes.iter().all(|s| ent.scopes.contains(s)) {
                continue;
            }
            // ent contains all scopes, let's check expiration
            // Note: Typically only a handful of scopes are used with few combinations.
            // Therefore, to keep things simpler, we stop at the first hit that meet all
            // required scopes.
            if let Some(expiration) = ent.expiration {
                if expiration < Utc::now() {
                    if let Some(refresh_token) = &ent.refresh_token {
                        let scopes = ent.scopes.clone();
                        return CacheResult::Expired(refresh_token.clone(), scopes);
                    } else {
                        return CacheResult::None;
                    }
                }
            }
            return CacheResult::Ok(ent.access_token.clone());
        }
        CacheResult::None
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

#[derive(Debug)]
pub struct TokenCache {
    persist: TokenPersist,
    store: TokenStore,
}

impl TokenCache {
    pub async fn new(persist: TokenPersist) -> anyhow::Result<Self> {
        let store = if let Some(path) = persist.try_path() {
            TokenStore::try_read_from_disk(path).await?
        } else {
            None
        };
        let store = store.unwrap_or_default();
        Ok(TokenCache { persist, store })
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
        self.store.insert(tok);
    }

    pub fn check(&self, scopes: &[Scope]) -> CacheResult {
        if !self.persist.has_mem() {
            return CacheResult::None;
        }
        let res = self.store.get(scopes);

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
            self.store.write_to_disk(path).await?;
        }
        Ok(())
    }
}
