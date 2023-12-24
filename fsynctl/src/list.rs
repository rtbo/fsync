use fsync::loc::user;

use crate::Error;

pub fn list_drives() -> Result<Vec<String>, Error> {
    let config_dir = user::config_dir()?;
    if !config_dir.exists() {
        return Ok(Vec::new());
    }
    let dirent = config_dir.read_dir_utf8()?;
    let mut drives = Vec::new();
    for di in dirent {
        let di = di?;
        if di.file_type()?.is_dir() {
            drives.push(di.file_name().into());
        }
    }
    Ok(drives)
}

pub fn main() -> Result<(), Error> {
    let drives = list_drives()?;
    if drives.is_empty() {
        println!("(no fsync service yet)");
    }
    for dr in drives.iter() {
        println!("{dr}");
    }
    Ok(())
}
