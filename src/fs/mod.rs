pub mod vfs;
pub mod diskfs;
pub mod part;
pub mod ext2;
pub mod ext4;
pub mod exfat;

pub struct FileSystemManager {
    pub vfs: vfs::VirtualFileSystem,
}

impl FileSystemManager {
    pub fn new() -> Self {
        diskfs::init();
        crate::drivers::usb::init();
        Self {
            vfs: vfs::VirtualFileSystem::new(),
        }
    }
}
