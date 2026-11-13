//! Kullanıcı Bellek Kopyalama Katmanı / Safe User Memory Copy Layer.
//!
//! Ring 3 süreçlerinden gelen tüm işaretçiler (`SYS_WRITE`, `SYS_EXEC`,
//! `SYS_WAIT`, `SYS_MEMINFO`, dosya yolları vb.) bu modül üzerinden kopyalanır.
//! Çekirdek kullanıcı adreslerini **asla doğrudan dereference etmez**.
//!
//! Temel kurallar:
//! * Aralık **tamamı** doğrulanmadan tek bayt bile kopyalanmaz (atomiklik);
//!   yarı eşlenmiş bir tampon kısmi veri sızdırmaz.
//! * Yalnızca `AddressSpace::translate_user` dört sayfalama seviyesinde de
//!   `PRESENT | USER` gördüğü adresler kabul edilir; çekirdek yarısı reddedilir.
//! * Yazma yolları ayrıca `WRITABLE` ve `PAGE_COW == 0` koşulunu arar. COW
//!   sayfasına çekirdekten yazmak, kullanıcının `#PF` yolunu atlayacağı için
//!   reddedilir; çağıran önce kullanıcı tarafında yazdırmalıdır.
//! * Kopyalama sayfa sayfa yapılır (`copy_nonoverlapping`), böylece bayt başına
//!   sayfa tablosu yürüyüşü yapılmaz.
//!
//! All syscall user pointers are validated and copied here. The kernel never
//! dereferences user addresses directly; a range is validated in full before
//! any byte is copied, writes require `WRITABLE` and non-COW pages, and the
//! actual copy runs page-by-page with `copy_nonoverlapping`.

use alloc::vec::Vec;

use crate::mm::pmm::PAGE_SIZE;
use crate::mm::process::PAGE_COW;
use crate::mm::vmm::{AddressSpace, PageFlags};

/// Tek bir kopyalamada kabul edilen azami bayt sayısı (1 MiB).
pub const MAX_USER_COPY: usize = 1024 * 1024;

/// Kullanıcı bellek erişim hatası / user memory access error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserCopyError {
    /// `NULL` işaretçi (sıfır uzunluk hariç).
    NullPointer,
    /// Adres + uzunluk `u64` taşması.
    Overflow,
    /// Çekirdek yarısı veya kanonik olmayan adres.
    OutOfRange,
    /// Aralıktaki bir sayfa eşlenmemiş ya da `USER` biti yok.
    Unmapped,
    /// Hedef sayfa yazılabilir değil (salt-okunur / COW).
    NotWritable,
    /// `MAX_USER_COPY` sınırı aşıldı.
    TooLarge,
    /// Geçici çekirdek tamponu ayrılamadı.
    OutOfMemory,
}

impl UserCopyError {
    /// Syscall hata sözleşmesine göre dönüş değeri.
    pub const fn as_syscall_result(self) -> u64 {
        u64::MAX
    }
}

/// Kullanıcı adres uzayının üst sınırı (kanonik alt yarı).
const USER_ADDRESS_END: u64 = 0x0000_8000_0000_0000;

fn user_page_of(address: u64) -> u64 {
    address & !(PAGE_SIZE - 1)
}

/// Adres + uzunluk aralığını taşma ve kanoniklik açısından doğrular.
fn checked_user_range(address: u64, len: usize) -> Result<(u64, u64), UserCopyError> {
    if len == 0 {
        return Ok((address, address));
    }
    if address == 0 {
        return Err(UserCopyError::NullPointer);
    }
    let end = address
        .checked_add(len as u64)
        .ok_or(UserCopyError::Overflow)?;
    if address >= USER_ADDRESS_END || end > USER_ADDRESS_END {
        return Err(UserCopyError::OutOfRange);
    }
    Ok((address, end))
}

/// `[address, address + len)` aralığının tamamının kullanıcı erişimli ve
/// (istenirse) yazılabilir olduğunu doğrular. Hiçbir bayt kopyalamaz.
pub fn validate_user_range(
    space: &AddressSpace,
    address: u64,
    len: usize,
    write: bool,
) -> Result<(), UserCopyError> {
    if len > MAX_USER_COPY {
        return Err(UserCopyError::TooLarge);
    }
    let (start, end) = checked_user_range(address, len)?;
    if len == 0 {
        return Ok(());
    }
    let mut page = user_page_of(start);
    while page < end {
        if space.translate_user(page).is_none() {
            return Err(UserCopyError::Unmapped);
        }
        if write {
            let entry = space.read_entry(page).ok_or(UserCopyError::Unmapped)?;
            if entry & PageFlags::WRITABLE == 0 || entry & PAGE_COW != 0 {
                return Err(UserCopyError::NotWritable);
            }
        }
        page = page.saturating_add(PAGE_SIZE);
    }
    Ok(())
}

/// Kullanıcı belleğinden çekirdek tamponuna kopyalar.
///
/// Dönüş: doğrulanmış aralığın baytları. Aralık tamamen geçerli değilse
/// `Err` döner ve **hiçbir veri** kopyalanmamış olur.
pub fn copy_from_user(
    space: &AddressSpace,
    address: u64,
    len: usize,
) -> Result<Vec<u8>, UserCopyError> {
    if len > MAX_USER_COPY {
        return Err(UserCopyError::TooLarge);
    }
    let (start, _end) = checked_user_range(address, len)?;
    if len == 0 {
        return Ok(Vec::new());
    }
    // Atomiklik: tüm sayfaları önce doğrula.
    validate_user_range(space, start, len, false)?;

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| UserCopyError::OutOfMemory)?;

    let mut copied = 0usize;
    let mut virtual_address = start;
    while copied < len {
        let physical = space
            .translate_user(virtual_address)
            .ok_or(UserCopyError::Unmapped)?;
        let page_offset = (virtual_address & (PAGE_SIZE - 1)) as usize;
        let chunk = (len - copied).min(PAGE_SIZE as usize - page_offset);
        unsafe {
            let source = crate::drivers::phys_to_virt(physical) as *const u8;
            bytes.extend_from_slice(core::slice::from_raw_parts(source, chunk));
        }
        virtual_address += chunk as u64;
        copied += chunk;
    }
    Ok(bytes)
}

/// Çekirdek tamponundaki baytları kullanıcı belleğine kopyalar.
///
/// Hedef aralık tamamen yazılabilir değilse `Err` döner ve hiçbir bayt
/// yazılmaz (`SYS_WAIT` durum kodu, `SYS_MEMINFO` raporu).
pub fn copy_to_user(space: &AddressSpace, address: u64, data: &[u8]) -> Result<(), UserCopyError> {
    if data.len() > MAX_USER_COPY {
        return Err(UserCopyError::TooLarge);
    }
    let (start, _end) = checked_user_range(address, data.len())?;
    if data.is_empty() {
        return Ok(());
    }
    validate_user_range(space, start, data.len(), true)?;

    let mut copied = 0usize;
    let mut virtual_address = start;
    while copied < data.len() {
        let physical = space
            .translate_user(virtual_address)
            .ok_or(UserCopyError::Unmapped)?;
        let page_offset = (virtual_address & (PAGE_SIZE - 1)) as usize;
        let chunk = (data.len() - copied).min(PAGE_SIZE as usize - page_offset);
        unsafe {
            let destination = crate::drivers::phys_to_virt(physical) as *mut u8;
            core::ptr::copy_nonoverlapping(data.as_ptr().add(copied), destination, chunk);
        }
        virtual_address += chunk as u64;
        copied += chunk;
    }
    Ok(())
}

/// Kullanıcı belleğinden tek bir `u64` okur.
pub fn read_user_u64(space: &AddressSpace, address: u64) -> Result<u64, UserCopyError> {
    let bytes = copy_from_user(space, address, core::mem::size_of::<u64>())?;
    let mut value = [0u8; 8];
    value.copy_from_slice(&bytes);
    Ok(u64::from_le_bytes(value))
}

/// Kullanıcı belleğine tek bir `u64` yazar.
pub fn write_user_u64(space: &AddressSpace, address: u64, value: u64) -> Result<(), UserCopyError> {
    copy_to_user(space, address, &value.to_le_bytes())
}

/// Kullanıcı belleğinden NUL ile sonlanan bir dizeyi en fazla `max_len` bayt
/// okuyarak kopyalar. Dönüş değeri NUL **içermez**.
pub fn copy_cstring_from_user(
    space: &AddressSpace,
    address: u64,
    max_len: usize,
) -> Result<Vec<u8>, UserCopyError> {
    if max_len == 0 || max_len > MAX_USER_COPY {
        return Err(UserCopyError::TooLarge);
    }
    if address == 0 {
        return Err(UserCopyError::NullPointer);
    }
    let limit = max_len.min(MAX_USER_COPY);
    let mut out = Vec::new();
    out.try_reserve_exact(limit)
        .map_err(|_| UserCopyError::OutOfMemory)?;

    let mut offset = 0usize;
    while offset < limit {
        let virtual_address = address
            .checked_add(offset as u64)
            .ok_or(UserCopyError::Overflow)?;
        if virtual_address >= USER_ADDRESS_END {
            return Err(UserCopyError::OutOfRange);
        }
        let physical = space
            .translate_user(virtual_address)
            .ok_or(UserCopyError::Unmapped)?;
        let page_offset = (virtual_address & (PAGE_SIZE - 1)) as usize;
        let chunk = (limit - offset).min(PAGE_SIZE as usize - page_offset);
        unsafe {
            let source = crate::drivers::phys_to_virt(physical) as *const u8;
            for index in 0..chunk {
                let byte = core::ptr::read_volatile(source.add(index));
                if byte == 0 {
                    return Ok(out);
                }
                out.push(byte);
            }
        }
        offset += chunk;
    }
    Err(UserCopyError::TooLarge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_length_ranges_are_allowed() {
        let space = AddressSpace { pml4_phys: 0 };
        assert!(validate_user_range(&space, 0, 0, false).is_ok());
        assert!(copy_from_user(&space, 0, 0).is_ok());
        assert!(copy_to_user(&space, 0, &[]).is_ok());
    }

    #[test]
    fn null_pointer_is_rejected() {
        let space = AddressSpace { pml4_phys: 0 };
        assert_eq!(
            copy_from_user(&space, 0, 4),
            Err(UserCopyError::NullPointer)
        );
    }

    #[test]
    fn kernel_half_is_rejected() {
        let space = AddressSpace { pml4_phys: 0 };
        assert_eq!(
            validate_user_range(&space, USER_ADDRESS_END, 4, false),
            Err(UserCopyError::OutOfRange)
        );
    }

    #[test]
    fn oversized_copy_is_rejected() {
        let space = AddressSpace { pml4_phys: 0 };
        assert_eq!(
            copy_from_user(&space, 0x1000, MAX_USER_COPY + 1),
            Err(UserCopyError::TooLarge)
        );
    }

    #[test]
    fn overflow_is_rejected() {
        let space = AddressSpace { pml4_phys: 0 };
        assert_eq!(
            validate_user_range(&space, u64::MAX - 1, 8, false),
            Err(UserCopyError::Overflow)
        );
    }
}
