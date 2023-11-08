use tokio_stream::Stream;

use crate::Result;

pub enum EntryType
{
    Regular,
    Directory,
    Symlink,
    Special,
}

pub trait Entry {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn path(&self) -> &str;
    fn entry_type(&self) -> EntryType;
    fn symlink_target(&self) -> Option<&str>;
    fn mime_type(&self) -> Option<&str>;
}

pub trait Storage {
    type E : Entry;
    async fn entries(&self, dir_id: Option<&str>) -> Result<impl Stream<Item = Result<Self::E>>>;
}
