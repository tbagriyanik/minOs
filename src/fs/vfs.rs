#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
}

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
                    data: b"[MinOS] Kernel booted successfully\n",
                }),
                Some(VfsNode {
                    name: "readme.txt",
                    node_type: NodeType::File,
                    size: 47,
                    data: b"Welcome to MinOS - Fast & Lightweight x86_64 OS\n",
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
