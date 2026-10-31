//! Small format and I/O adapters used by kernel services.

use embedded_io::{ErrorType, Read, Write};
use heapless::Vec;
use serde::{Deserialize, Serialize};

/// A non-blocking byte source can implement this adapter without `std::io`.
pub trait KernelRead: Read<Error = KernelIoError> {}
impl<T: Read<Error = KernelIoError>> KernelRead for T {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelIoError {
    WouldBlock,
    Fault,
}
impl embedded_io::Error for KernelIoError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::WouldBlock | Self::Fault => embedded_io::ErrorKind::Other,
        }
    }
}

/// Compact task/IPC message representation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMessage {
    pub kind: u8,
    pub value: u32,
}

pub fn encode_task_message(message: &TaskMessage) -> Result<Vec<u8, 16>, postcard::Error> {
    let mut out = Vec::<u8, 16>::new();
    postcard::to_slice(message, &mut out)?;
    Ok(out)
}

/// Compile-time checked ELF magic used before handing an image to the loader.
pub fn is_elf64(image: &[u8]) -> bool {
    image.len() >= 5 && &image[..4] == b"\x7fELF" && image[4] == 2
}

pub fn elf64_header_size() -> usize {
    core::mem::size_of::<elf::file::Elf64_Ehdr>()
}

pub fn _write_trait_marker<T: Write>(_writer: &mut T) {}
pub fn _error_type_marker<T: ErrorType>(_value: core::marker::PhantomData<T>) {}
