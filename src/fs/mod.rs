pub mod vfs;
pub mod diskfs;

pub struct FileSystemManager {
    pub vfs: vfs::VirtualFileSystem,
}

impl FileSystemManager {
    pub const fn new() -> Self {
        Self {
            vfs: vfs::VirtualFileSystem::new(),
        }
    }
}
