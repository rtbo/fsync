use byte_unit::AdjustedByte;
use fsync::loc::{inst, user};

/// If a single instance of fsyncd exists, get its name
pub fn single_instance_name() -> anyhow::Result<Option<String>> {
    let config_dir = user::config_dir()?;
    if !config_dir.exists() {
        return Ok(None);
    }
    let dirent = config_dir.read_dir_utf8()?.collect::<Vec<_>>();

    if dirent.len() != 1 {
        return Ok(None);
    }
    let entry = dirent.into_iter().next().unwrap()?;
    if entry.file_type()?.is_dir() {
        let filename = entry.file_name().to_owned();
        Ok(Some(filename))
    } else {
        Ok(None)
    }
}

pub fn instance_port(instance_name: &str) -> anyhow::Result<u16> {
    let pf = inst::runtime_port_file(instance_name)?;
    if !pf.exists() {
        anyhow::bail!(
            "Could not find {pf}. Are you sure the fsyncd {instance_name} instance is running?"
        );
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
