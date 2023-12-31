//! Locations module

/// Locations for the user
pub mod user {
    use camino::Utf8PathBuf;

    pub fn home_dir() -> anyhow::Result<Utf8PathBuf> {
        let dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Can't get HOME directory"))?;
        Ok(Utf8PathBuf::try_from(dir)?)
    }

    pub fn runtime_dir() -> anyhow::Result<Utf8PathBuf> {
        let dir = dirs::runtime_dir()
            .ok_or_else(|| anyhow::anyhow!("Can't get the user runtime directory"))?;
        let dir = Utf8PathBuf::try_from(dir)?;
        Ok(dir.join("fsync"))
    }

    pub fn config_dir() -> anyhow::Result<Utf8PathBuf> {
        let dir =
            dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Can't get config directory"))?;
        let dir = Utf8PathBuf::try_from(dir)?;
        Ok(dir.join("fsync"))
    }

    pub fn cache_dir() -> anyhow::Result<Utf8PathBuf> {
        let dir = dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("Can't get cache directory"))?;
        let dir = Utf8PathBuf::try_from(dir)?;
        Ok(dir.join("fsync"))
    }
}

pub mod inst {
    use camino::Utf8PathBuf;

    pub fn runtime_port_file(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(super::user::runtime_dir()?.join(format!("{instance_name}.port")))
    }

    pub fn config_dir(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(super::user::config_dir()?.join(instance_name))
    }

    pub fn config_file(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(config_dir(instance_name)?.join("config.json"))
    }

    pub fn oauth_secret_file(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(config_dir(instance_name)?.join("client_secret.json"))
    }

    pub fn cache_dir(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(super::user::cache_dir()?.join(instance_name))
    }

    pub fn token_cache_file(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(cache_dir(instance_name)?.join("token_cache.json"))
    }

    pub fn remote_cache_file(instance_name: &str) -> anyhow::Result<Utf8PathBuf> {
        Ok(cache_dir(instance_name)?.join("remote.bin"))
    }
}
