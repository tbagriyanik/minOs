#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
}

// Static buffers for file data storage (max 8 files, each up to 512 bytes)
static mut FILE_BUFFERS: [[u8; 512]; 8] = [[0; 512]; 8];
static mut BUFFER_SIZES: [usize; 8] = [0; 8];

pub struct VfsNode {
    pub name: &'static str,
    pub node_type: NodeType,
    pub size: usize,
    pub data: &'static [u8],
}

pub struct VirtualFileSystem {
    pub nodes: [Option<VfsNode>; 8],
}

impl VirtualFileSystem {
    pub const fn new() -> Self {
        Self {
            nodes: [
                Some(VfsNode {
                    name: "kernel.log",
                    node_type: NodeType::File,
                    size: 34,
                    data: b"[minOs] Kernel booted successfully\n",
                }),
                Some(VfsNode {
                    name: "readme.txt",
                    node_type: NodeType::File,
                    size: 47,
                    data: b"Welcome to minOs - Fast & Lightweight x86_64 OS\n",
                }),
                Some(VfsNode {
                    name: "settings.cfg",
                    node_type: NodeType::File,
                    size: 24,
                    data: b"theme=dark\nvolume=80\n",
                }),
                None,
                None,
                None,
                None,
                None,
            ],
        }
    }

    pub fn find_file(&self, name: &str) -> Option<&VfsNode> {
        for node_opt in self.nodes.iter() {
            if let Some(node) = node_opt {
                if node.node_type == NodeType::File && node.name == name {
                    return Some(node);
                }
            }
        }
        None
    }
}

impl VirtualFileSystem {
    // Write or overwrite a file in the VFS
    pub fn write_file(&mut self, name: &str, data: &[u8]) {
        // Map runtime name to static literals for known files
        let static_name = match name {
            "kernel.log" => "kernel.log",
            "readme.txt" => "readme.txt",
            "settings.cfg" => "settings.cfg",
            _ => return, // ignore unknown files
        };
        // Find existing node and write data
        for (idx, slot) in self.nodes.iter_mut().enumerate() {
            if let Some(node) = slot {
                if node.name == static_name {
                    unsafe {
                        let copy_len = core::cmp::min(data.len(), FILE_BUFFERS[idx].len());
                        FILE_BUFFERS[idx][..copy_len].copy_from_slice(&data[..copy_len]);
                        BUFFER_SIZES[idx] = copy_len;
                        let slice: &'static [u8] = &FILE_BUFFERS[idx][..copy_len];
                        *node = VfsNode {
                            name: static_name,
                            node_type: NodeType::File,
                            size: copy_len,
                            data: slice,
                        };
                    }
                    return;
                }
            }
        }
        // Insert into first empty slot
        for (idx, slot) in self.nodes.iter_mut().enumerate() {
            if slot.is_none() {
                unsafe {
                    let copy_len = core::cmp::min(data.len(), FILE_BUFFERS[idx].len());
                    FILE_BUFFERS[idx][..copy_len].copy_from_slice(&data[..copy_len]);
                    BUFFER_SIZES[idx] = copy_len;
                    let slice: &'static [u8] = &FILE_BUFFERS[idx][..copy_len];
                    *slot = Some(VfsNode {
                        name: static_name,
                        node_type: NodeType::File,
                        size: copy_len,
                        data: slice,
                    });
                }
                return;
            }
        }
        // No space left; ignore
    }

    // Read a file from the VFS
    pub fn read_file(&self, name: &str) -> Option<&[u8]> {
        self.find_file(name).map(|node| node.data)
    }
}

