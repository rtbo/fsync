use byte_unit::AdjustedByte;
use fsync::loc::{inst, user};

use crate::{Error, Result};

/// If a single instance of fsyncd exists, get its name
pub fn single_instance_name() -> Result<Option<String>> {
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

pub fn instance_port(instance_name: &str) -> Result<u16> {
    let pf = inst::runtime_port_file(instance_name)?;
    if !pf.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "Could not find {pf}. Are you sure the fsyncd {instance_name} instance is running?"
            ),
        )));
    }
    let content = std::fs::read(&pf)?;
    let content = String::from_utf8(content).map_err(|err| err.utf8_error())?;
    let port: u16 = serde_json::from_str(&content)?;
    Ok(port)
}


pub fn adjusted_byte(val: u64) -> AdjustedByte {
    use byte_unit::{Byte, UnitType};

    let byte = Byte::from(val);
    byte.get_appropriate_unit(UnitType::Binary)
}