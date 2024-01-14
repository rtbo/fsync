pub mod oauth;
use futures::{future::{BoxFuture, self}, Future};

pub mod service;
pub mod storage;
pub mod tree;

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

pub trait Shutdown: Sync + Send + 'static {
    fn shutdown(&self) -> BoxFuture<'_, anyhow::Result<()>>;
}

pub trait PersistCache {
    fn persist_cache(&self) -> impl Future<Output = anyhow::Result<()>> + Send {
        future::ready(Ok(()))
    }
}
