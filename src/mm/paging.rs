//! Paging-mode capability helpers for x86-64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Four = 4,
    Five = 5,
}

pub const MAX_LEVELS: usize = 5;

pub fn supports_five_level() -> bool {
    let result = core::arch::x86_64::__cpuid_count(7, 0);
    (result.ecx & (1 << 16)) != 0
}

pub fn active_level() -> Level {
    let cr4: u64;
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    if (cr4 & (1 << 12)) != 0 {
        Level::Five
    } else {
        Level::Four
    }
}
