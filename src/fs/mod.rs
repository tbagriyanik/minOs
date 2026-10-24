pub mod vfs;
pub mod diskfs;
pub mod part;
pub mod block;
pub mod ext2;
pub mod ext4;
pub mod ext4_rw;
pub mod exfat;
pub mod exfat_rw;

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
