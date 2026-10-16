//! Çok çekirdekli (SMP) senkronizasyon ilkelleri.
//!
//! Kesme altyapısı henüz kurulmadığından tüm kilitler IRQ maskesiz
//! spinlock'tur; kritik bölümler kısa tutulur ve kilit eldeyken
//! başka kilit alınmaz (kilit sıralaması: tek seviye).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// Basit test-and-set spinlock.
pub struct Spinlock {
    locked: AtomicBool,
}

impl Spinlock {
    pub const fn new() -> Self {
        Self { locked: AtomicBool::new(false) }
    }

    #[inline(always)]
    pub fn lock(&self) {
        loop {
            // Önce sadece oku (paylaşımlı önbellek hattı, bus kilidi yapmaz)
            if !self.locked.load(Ordering::Relaxed) {
                // Boş göründüğünde atomik olarak kapmayı dene
                if !self.locked.swap(true, Ordering::Acquire) {
                    return;
                }
            }
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

unsafe impl Send for Spinlock {}
unsafe impl Sync for Spinlock {}

/// Spinlock korumalı hücre. `lock()` bir koruma nesnesi döndürür.
pub struct SpinMutex<T> {
    lock: Spinlock,
    data: UnsafeCell<T>,
}

impl<T> SpinMutex<T> {
    pub const fn new(data: T) -> Self {
        Self { lock: Spinlock::new(), data: UnsafeCell::new(data) }
    }

    #[inline(always)]
    pub fn lock(&self) -> SpinGuard<'_, T> {
        self.lock.lock();
        SpinGuard { mutex: self }
    }
}

unsafe impl<T: Send> Send for SpinMutex<T> {}
unsafe impl<T: Send> Sync for SpinMutex<T> {}

pub struct SpinGuard<'a, T> {
    mutex: &'a SpinMutex<T>,
}

impl<T> core::ops::Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> core::ops::DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.lock.unlock();
    }
}
