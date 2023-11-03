pub trait File {
    fn name(&self) -> &str;
    fn size(&self) -> u64;
}

pub trait Repo {
    type F : File;
    fn list_files(&self, root: &str) -> &[Self::F];
}
