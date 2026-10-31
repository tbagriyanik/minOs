//! Small format and I/O adapters used by kernel services.
//!
//! Bu modül yalnızca `core`+`alloc` kullanır: harici `embedded-io`, `serde`,
//! `postcard` ve `elf` bağımlılıkları kaldırıldı. ELF başlık sabitleri
//! `core::mem` ile ifade edilir, IPC mesajı kodlaması basit ve deterministik
//! bir bayt düzenine sahiptir.

use alloc::vec::Vec;

/// A non-blocking byte source can implement this adapter without `std::io`.
pub trait KernelRead {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, KernelIoError>;
}

/// A non-blocking byte sink.
pub trait KernelWrite {
    fn write(&mut self, buf: &[u8]) -> Result<usize, KernelIoError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelIoError {
    WouldBlock,
    Fault,
}

impl KernelIoError {
    /// `embedded-io` `ErrorKind::Other` eşdeğeri.
    pub fn kind(&self) -> KernelIoErrorKind {
        KernelIoErrorKind::Other
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelIoErrorKind {
    WouldBlock,
    Other,
}

/// Compact task/IPC message representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskMessage {
    pub kind: u8,
    pub value: u32,
}

/// `TaskMessage`'ı 5 baytlık sabit düzene kodlar (kind, value LE).
pub fn encode_task_message(message: &TaskMessage) -> [u8; 5] {
    let v = message.value.to_le_bytes();
    [message.kind, v[0], v[1], v[2], v[3]]
}

/// `encode_task_message` çıktısını çözer.
pub fn decode_task_message(bytes: &[u8]) -> Option<TaskMessage> {
    if bytes.len() < 5 {
        return None;
    }
    Some(TaskMessage {
        kind: bytes[0],
        value: u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]),
    })
}

/// Big-endian yazımı için yardımcı (postcard'ın yaptığı işin küçük karşılığı).
pub fn encode_u32_be(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

/// Üst düzey IPC kodlaması için sınırlı çıktı tamponu.
pub fn encode_task_message_vec(message: &TaskMessage) -> Vec<u8> {
    encode_task_message(message).to_vec()
}

/// Compile-time checked ELF magic used before handing an image to the loader.
pub fn is_elf64(image: &[u8]) -> bool {
    image.len() >= 5 && &image[..4] == b"\x7fELF" && image[4] == 2
}

/// `Elf64_Ehdr` boyutu (ELF64 başlığı: e_ident 16 + 48 bayt alanlar = 64).
pub fn elf64_header_size() -> usize {
    64
}
