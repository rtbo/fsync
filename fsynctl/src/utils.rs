use fsync::loc::{inst, user};

use crate::{Error, Result};

pub fn get_single_share() -> Result<Option<String>> {
    let config_dir = user::config_dir()?;
    if !config_dir.exists() {
        return Ok(None);
    }
    let dirent: Vec<_> = config_dir.read_dir_utf8()?.try_collect()?;

    if dirent.len() != 1 {
        return Ok(None);
    }
    let entry = &dirent[0];
    if entry.file_type()?.is_dir() {
        Ok(Some(entry.file_name().to_owned()))
    } else {
        Ok(None)
    }
}

pub fn get_share_port(name: &str) -> Result<u16> {
    let pf = inst::runtime_port_file(name)?;
    if !pf.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Could not find {pf}. Are you sure the fsyncd {name} instance is running?"),
        )));
    }
    let content = std::fs::read(&pf)?;
    let content = String::from_utf8(content).map_err(|err| err.utf8_error())?;
    let port: u16 = serde_json::from_str(&content)?;
    Ok(port)
}
