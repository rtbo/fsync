use std::ops;

use serde::{Deserialize, Serialize};

use crate::StorageLoc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dir {
    pub data: i64,
    pub dirs: i32,
    pub files: i32,
}

impl Dir {
    pub fn null() -> Self {
        Self {
            data: 0,
            dirs: 0,
            files: 0,
        }
    }

    pub fn is_null(&self) -> bool {
        self.data == 0 && self.dirs == 0 && self.files == 0
    }

    pub fn is_positive(&self) -> bool {
        self.data >= 0 && self.dirs >= 0 && self.files >= 0
    }

    pub fn entries(&self) -> i32 {
        self.dirs + self.files
    }

    pub fn with_data(self, data: i64) -> Self {
        Self { data, ..self }
    }

    pub fn with_dirs(self, dirs: i32) -> Self {
        Self { dirs, ..self }
    }

    pub fn with_files(self, files: i32) -> Self {
        Self { files, ..self }
    }
}

impl ops::Add for Dir {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            data: self.data + rhs.data,
            dirs: self.dirs + rhs.dirs,
            files: self.files + rhs.files,
        }
    }
}

impl ops::AddAssign for Dir {
    fn add_assign(&mut self, rhs: Self) {
        self.data += rhs.data;
        self.dirs += rhs.dirs;
        self.files += rhs.files;
    }
}

impl ops::Sub for Dir {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            data: self.data - rhs.data,
            dirs: self.dirs - rhs.dirs,
            files: self.files - rhs.files,
        }
    }
}

impl ops::SubAssign for Dir {
    fn sub_assign(&mut self, rhs: Self) {
        self.data -= rhs.data;
        self.dirs -= rhs.dirs;
        self.files -= rhs.files;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Tree {
    pub local: Dir,
    pub remote: Dir,
    pub conflicts: i32,
}

impl Tree {
    pub fn null() -> Self {
        Tree {
            local: Dir::null(),
            remote: Dir::null(),
            conflicts: 0,
        }
    }

    pub fn is_null(&self) -> bool {
        self.local.is_null() && self.remote.is_null() && self.conflicts == 0
    }

    pub fn is_positive(&self) -> bool {
        self.local.is_positive() && self.remote.is_positive() && self.conflicts >= 0
    }

    pub fn by_loc(&self, loc: StorageLoc) -> &Dir {
        match loc {
            StorageLoc::Local => &self.local,
            StorageLoc::Remote => &self.remote,
        }
    }
}

impl ops::Add for Tree {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            local: self.local + rhs.local,
            remote: self.remote + rhs.remote,
            conflicts: self.conflicts + rhs.conflicts,
        }
    }
}

impl ops::AddAssign for Tree {
    fn add_assign(&mut self, rhs: Self) {
        self.local += rhs.local;
        self.remote += rhs.remote;
        self.conflicts += rhs.conflicts;
    }
}

impl ops::Sub for Tree {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            local: self.local - rhs.local,
            remote: self.remote - rhs.remote,
            conflicts: self.conflicts - rhs.conflicts,
        }
    }
}

impl ops::SubAssign for Tree {
    fn sub_assign(&mut self, rhs: Self) {
        self.local -= rhs.local;
        self.remote -= rhs.remote;
        self.conflicts -= rhs.conflicts;
    }
}
