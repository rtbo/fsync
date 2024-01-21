#![feature(iter_intersperse)]

use futures::{
    future::{self, BoxFuture},
    Future,
};

pub mod service;
pub mod storage;
pub mod tree;

pub mod oauth2;

pub mod uri {
    #[derive(Debug)]
    pub struct QueryMap<'a>(Vec<(&'a str, &'a str)>);

    impl<'a> QueryMap<'a> {
        pub fn parse(query: Option<&'a str>) -> anyhow::Result<QueryMap<'a>> {
            let mut vec = Vec::new();
            if let Some(query) = query {
                let parts = query.split("&");
                for part in parts {
                    let (name, value) = part.split_once('=').unwrap_or((part, ""));
                    vec.push((name, value));
                }
            }
            Ok(QueryMap(vec))
        }

        pub fn get(&'a self, key: &str) -> Option<&'a str> {
            for (k, v) in self.0.iter() {
                if *k == key {
                    return Some(*v);
                }
            }
            None
        }
    }
}

mod error {
    /// Maps error to fsync::Error::Auth (to be used in `map_err`)
    pub fn auth<E: std::error::Error>(err: E) -> fsync::Error {
        fsync::Error::Auth(err.to_string())
    }

    /// Maps error to fsync::Error::Api (to be used in `map_err`)
    pub fn api<E: std::error::Error>(err: E) -> fsync::Error {
        fsync::Error::Api(err.to_string())
    }

    /// Maps error to fsync::Error::Io (to be used in `map_err`)
    pub fn io<E: std::error::Error>(err: E) -> fsync::Error {
        fsync::Error::Io(err.to_string())
    }
}

pub trait PersistCache {
    fn persist_cache(&self) -> impl Future<Output = anyhow::Result<()>> + Send {
        future::ready(Ok(()))
    }
}

pub trait ShutdownObj: Send + Sync + 'static {
    fn shutdown_obj(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(future::ready(Ok(())))
    }
}

impl<T> ShutdownObj for T
where
    T: Shutdown + Send + Sync + 'static,
{
    fn shutdown_obj(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(self.shutdown())
    }
}

pub trait Shutdown {
    fn shutdown(&self) -> impl Future<Output = anyhow::Result<()>> + Send {
        future::ready(Ok(()))
    }
}
