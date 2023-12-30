//! Locations module

/// Locations for the user
pub mod user {
    use crate::path::PathBuf;

    pub fn home_dir() -> anyhow::Result<PathBuf> {
        let dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Can't get HOME directory"))?;
        Ok(PathBuf::from_path_buf(dir).unwrap())
    }

    pub fn runtime_dir() -> anyhow::Result<PathBuf> {
        let dir = dirs::runtime_dir()
            .ok_or_else(|| anyhow::anyhow!("Can't get the user runtime directory"))?;
        let dir = PathBuf::from_path_buf(dir).unwrap();
        Ok(dir.join("fsync"))
    }

    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let dir =
            dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Can't get config directory"))?;
        let dir = PathBuf::from_path_buf(dir).expect("Non Utf8 path");
        Ok(dir.join("fsync"))
    }

    pub fn cache_dir() -> anyhow::Result<PathBuf> {
        let dir = dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("Can't get cache directory"))?;
        let dir = PathBuf::from_path_buf(dir).expect("Non Utf8 path");
        Ok(dir.join("fsync"))
    }
}

pub mod inst {
    use crate::path::PathBuf;

    pub fn runtime_port_file(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(super::user::runtime_dir()?.join(format!("{instance_name}.port")))
    }

    pub fn config_dir(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(super::user::config_dir()?.join(instance_name))
    }

    pub fn config_file(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(config_dir(instance_name)?.join("config.json"))
    }

    pub fn oauth_secret_file(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(config_dir(instance_name)?.join("client_secret.json"))
    }

    pub fn cache_dir(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(super::user::cache_dir()?.join(instance_name))
    }

    pub fn token_cache_file(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(cache_dir(instance_name)?.join("token_cache.json"))
    }

    pub fn remote_cache_file(instance_name: &str) -> anyhow::Result<PathBuf> {
        Ok(cache_dir(instance_name)?.join("remote.bin"))
    }
}
