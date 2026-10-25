//! Host ortaminda bellek yoneticisi birim testleri (BumpAllocator & FixedSizeBlockAllocator).

use crate::allocator::{
    align_up, align_up_checked, BumpAllocator, FixedSizeBlockAllocator, Locked,
};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

#[test]
fn test_align_up_normal() {
    assert_eq!(align_up_checked(0, 8), Some(0));
    assert_eq!(align_up_checked(1, 8), Some(8));
    assert_eq!(align_up_checked(7, 8), Some(8));
    assert_eq!(align_up_checked(8, 8), Some(8));
    assert_eq!(align_up_checked(9, 8), Some(16));
    assert_eq!(align_up_checked(4095, 4096), Some(4096));
    assert_eq!(align_up_checked(4096, 4096), Some(4096));
    assert_eq!(align_up_checked(4097, 4096), Some(8192));
}

#[test]
fn test_align_up_overflow_and_invalid() {
    // Tasma kontrolu (addr + align - 1 > usize::MAX)
    assert_eq!(align_up_checked(usize::MAX - 3, 8), None);
    assert_eq!(align_up_checked(usize::MAX, 16), None);
    assert_eq!(align_up(usize::MAX - 3, 8), usize::MAX);

    // Gecersiz hizalamalar (0 veya 2'nin kuvveti olmayanlar)
    assert_eq!(align_up_checked(16, 0), None);
    assert_eq!(align_up_checked(16, 3), None);
    assert_eq!(align_up_checked(16, 5), None);
    assert_eq!(align_up_checked(16, 7), None);
}

#[test]
fn test_bump_allocator_basic_and_overlap() {
    let mut memory = [0u8; 1024];
    let mut bump = BumpAllocator::new();
    bump.init(memory.as_mut_ptr() as usize, memory.len());

    let l1 = Layout::from_size_align(16, 8).unwrap();
    let l2 = Layout::from_size_align(32, 16).unwrap();
    let l3 = Layout::from_size_align(64, 64).unwrap();

    let p1 = bump.alloc(l1);
    let p2 = bump.alloc(l2);
    let p3 = bump.alloc(l3);

    assert!(!p1.is_null());
    assert!(!p2.is_null());
    assert!(!p3.is_null());

    assert_eq!((p1 as usize) % 8, 0);
    assert_eq!((p2 as usize) % 16, 0);
    assert_eq!((p3 as usize) % 64, 0);

    // Cakisim engeli kontrolu
    assert!(p1 as usize + 16 <= p2 as usize);
    assert!(p2 as usize + 32 <= p3 as usize);

    // Bellek yazma ve okuma testi (guvenli dilimler ile)
    if let Some(non_null1) = core::ptr::NonNull::new(p1) {
        let s1 = unsafe { core::slice::from_raw_parts_mut(non_null1.as_ptr(), 16) };
        s1.fill(0xAA);
        assert_eq!(s1[0], 0xAA);
    }
    if let Some(non_null2) = core::ptr::NonNull::new(p2) {
        let s2 = unsafe { core::slice::from_raw_parts_mut(non_null2.as_ptr(), 32) };
        s2.fill(0xBB);
        assert_eq!(s2[0], 0xBB);
    }
    if let Some(non_null3) = core::ptr::NonNull::new(p3) {
        let s3 = unsafe { core::slice::from_raw_parts_mut(non_null3.as_ptr(), 64) };
        s3.fill(0xCC);
        assert_eq!(s3[0], 0xCC);
    }
}

#[test]
fn test_bump_allocator_zero_size() {
    let mut memory = [0u8; 256];
    let mut bump = BumpAllocator::new();
    bump.init(memory.as_mut_ptr() as usize, memory.len());

    let l_zero = Layout::from_size_align(0, 32).unwrap();
    let ptr = bump.alloc(l_zero);
    assert!(!ptr.is_null());
    assert_eq!((ptr as usize) % 32, 0);
    assert_eq!(bump.used_bytes(), 0);
    assert_eq!(bump.allocations_count(), 0);

    bump.dealloc(ptr, l_zero);
    assert_eq!(bump.allocations_count(), 0);
}

#[test]
fn test_bump_allocator_oom() {
    let mut memory = [0u8; 128];
    let mut bump = BumpAllocator::new();
    bump.init(memory.as_mut_ptr() as usize, memory.len());

    let l1 = Layout::from_size_align(100, 8).unwrap();
    let p1 = bump.alloc(l1);
    assert!(!p1.is_null());

    // Yetersiz alan
    let l2 = Layout::from_size_align(50, 8).unwrap();
    let p2 = bump.alloc(l2);
    assert!(p2.is_null());
}

#[test]
fn test_bump_allocator_lifo_and_reset() {
    let mut memory = [0u8; 512];
    let mut bump = BumpAllocator::new();
    bump.init(memory.as_mut_ptr() as usize, memory.len());

    let l = Layout::from_size_align(64, 8).unwrap();
    let p1 = bump.alloc(l);
    let p2 = bump.alloc(l);
    assert_eq!(bump.allocations_count(), 2);
    assert!(bump.used_bytes() >= 128);

    // LIFO serbest birakma: p2 geri sarilir
    let used_before = bump.used_bytes();
    bump.dealloc(p2, l);
    assert_eq!(bump.allocations_count(), 1);
    assert!(bump.used_bytes() < used_before);

    // p1 serbest birakildiginda sayac 0 olur ve yigin sifirlanir
    bump.dealloc(p1, l);
    assert_eq!(bump.allocations_count(), 0);
    assert_eq!(bump.used_bytes(), 0);

    // Reset testi
    let _p3 = bump.alloc(l);
    assert_eq!(bump.allocations_count(), 1);
    bump.reset();
    assert_eq!(bump.allocations_count(), 0);
    assert_eq!(bump.used_bytes(), 0);
}

#[test]
fn test_fixed_size_block_small_reuse() {
    let mut memory = [0u8; 4096];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());

    let l8 = Layout::from_size_align(8, 8).unwrap();
    let p1 = fsb.alloc(l8);
    assert!(!p1.is_null());
    assert_eq!((p1 as usize) % 8, 0);

    let p2 = fsb.alloc(l8);
    assert!(!p2.is_null());
    assert_ne!(p1, p2);

    // p1 serbest birakilir
    fsb.dealloc(p1, l8);

    // Yeni tahsis p1 blogunu yeniden kullanmali
    let p3 = fsb.alloc(l8);
    assert_eq!(p3, p1);

    // Temizleme
    fsb.dealloc(p2, l8);
    fsb.dealloc(p3, l8);
}

#[test]
fn test_fixed_size_block_various_alignments() {
    let mut memory = [0u8; 8192];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());

    let layouts = [
        Layout::from_size_align(4, 4).unwrap(),
        Layout::from_size_align(12, 16).unwrap(),
        Layout::from_size_align(30, 32).unwrap(),
        Layout::from_size_align(50, 64).unwrap(),
        Layout::from_size_align(100, 128).unwrap(),
        Layout::from_size_align(200, 256).unwrap(),
        Layout::from_size_align(500, 512).unwrap(),
        Layout::from_size_align(1000, 1024).unwrap(),
        Layout::from_size_align(1800, 2048).unwrap(),
    ];

    let mut ptrs = [null_mut(); 9];
    for (i, &l) in layouts.iter().enumerate() {
        let ptr = fsb.alloc(l);
        assert!(!ptr.is_null(), "Tahsis basarisiz: {:?}", l);
        assert_eq!((ptr as usize) % l.align(), 0, "Hizalama hatasi: {:?}", l);
        if let Some(non_null) = core::ptr::NonNull::new(ptr) {
            let slice = unsafe { core::slice::from_raw_parts_mut(non_null.as_ptr(), l.size()) };
            slice.fill((i as u8) + 1);
        }
        ptrs[i] = ptr;
    }

    for (i, &l) in layouts.iter().enumerate() {
        if let Some(non_null) = core::ptr::NonNull::new(ptrs[i]) {
            let slice = unsafe { core::slice::from_raw_parts(non_null.as_ptr(), l.size()) };
            assert_eq!(slice[0], (i as u8) + 1);
        }
        fsb.dealloc(ptrs[i], l);
    }
}

#[test]
fn test_fixed_size_block_zero_size() {
    let mut memory = [0u8; 1024];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());

    let l0 = Layout::from_size_align(0, 16).unwrap();
    let p = fsb.alloc(l0);
    assert!(!p.is_null());
    assert_eq!((p as usize) % 16, 0);

    fsb.dealloc(p, l0);
}

#[test]
fn test_fixed_size_block_large_reuse() {
    let mut memory = [0u8; 16384];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());

    // 4096 bayt (> 2048) buyuk tahsis
    let l_large = Layout::from_size_align(4096, 4096).unwrap();
    let p1 = fsb.alloc(l_large);
    assert!(!p1.is_null());
    assert_eq!((p1 as usize) % 4096, 0);

    if let Some(non_null) = core::ptr::NonNull::new(p1) {
        let slice = unsafe { core::slice::from_raw_parts_mut(non_null.as_ptr(), 4096) };
        slice.fill(0xEE);
        assert_eq!(slice[0], 0xEE);
    }

    let used_after_p1 = fsb.used_bytes();
    fsb.dealloc(p1, l_large);

    // Yeniden tahsis edildiginde buyuk serbest listeden karsilanmali;
    // yigin ibresi (used_bytes) ileri gitmemeli!
    let p2 = fsb.alloc(l_large);
    assert_eq!(p2, p1);
    assert_eq!(fsb.used_bytes(), used_after_p1);

    fsb.dealloc(p2, l_large);
}

#[test]
fn test_fixed_size_block_wild_free_rejected() {
    let mut memory = [0u8; 1024];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());

    let l = Layout::from_size_align(16, 8).unwrap();
    // Heap disi rastgele gosterici
    let wild_ptr = 0x1000 as *mut u8;
    // Cokmeden guvenle gormezden gelinmeli
    fsb.dealloc(wild_ptr, l);
}

#[test]
fn test_fixed_size_block_stress_cycle() {
    let mut memory = [0u8; 32768];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());

    let sizes = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
    let mut allocated = [(null_mut(), Layout::new::<u8>()); 30];

    // 3 dongu boyunca ardisik tahsis ve serbest birakma
    for cycle in 0..3 {
        for i in 0..30 {
            let sz = sizes[i % sizes.len()];
            let align = sz.min(64);
            let l = Layout::from_size_align(sz, align).unwrap();
            let ptr = fsb.alloc(l);
            assert!(
                !ptr.is_null(),
                "Dongu {} - Indeks {} tahsis basarisiz",
                cycle,
                i
            );
            if let Some(non_null) = core::ptr::NonNull::new(ptr) {
                let slice = unsafe { core::slice::from_raw_parts_mut(non_null.as_ptr(), sz) };
                slice.fill((cycle + i) as u8);
            }
            allocated[i] = (ptr, l);
        }

        // Ters sirada serbest birak
        for i in (0..30).rev() {
            let (ptr, l) = allocated[i];
            if let Some(non_null) = core::ptr::NonNull::new(ptr) {
                let slice = unsafe { core::slice::from_raw_parts(non_null.as_ptr(), l.size()) };
                assert_eq!(slice[0], (cycle + i) as u8);
            }
            fsb.dealloc(ptr, l);
        }
    }
}

#[test]
fn test_locked_global_alloc() {
    let mut memory = [0u8; 4096];
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(memory.as_mut_ptr() as usize, memory.len());
    let locked = Locked::new(fsb);

    let layout = Layout::from_size_align(64, 16).unwrap();
    unsafe {
        let ptr = GlobalAlloc::alloc(&locked, layout);
        assert!(!ptr.is_null());
        assert_eq!((ptr as usize) % 16, 0);
        if let Some(non_null) = core::ptr::NonNull::new(ptr) {
            let slice = core::slice::from_raw_parts_mut(non_null.as_ptr(), 64);
            slice.fill(0x42);
            assert_eq!(slice[0], 0x42);
        }
        GlobalAlloc::dealloc(&locked, ptr, layout);
    }
}
