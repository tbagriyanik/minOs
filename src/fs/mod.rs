pub mod block;
pub mod diskfs;
#[cfg(feature = "exfat")]
pub mod exfat;
#[cfg(feature = "exfat")]
pub mod exfat_rw;
pub mod ext2;
pub mod ext4;
pub mod ext4_rw;
pub mod part;
pub mod vfs;

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
