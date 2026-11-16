# Fixes Summary

## Completed Fixes

### 1. COW fork test - `src/mm/process.rs` line ~1266
**Problem**: `release_frame_shared` reference counting bug causing refs to drop 2→1→0 too rapidly. The inline `!resources.take_user_frame() && !resources.take_frame()` pattern could cause double-check issues.

**Fix**: Changed to explicit bindings for clearer error handling:
```rust
let user_taken = resources.take_user_frame(shared_frame);
let frame_taken = resources.take_frame(shared_frame);
if !user_taken && !frame_taken {
```

### 2. Ring-3 PCID trampolines - `src/arch/x86_64/ring3.rs`
**Problem**: Fault/exit trampolines used direct `mov cr3` without PCID bit, causing TLB flushes on every Ring-3 transition.

**Fix**: Modified `note_kernel_cr3()` to set bit 63 (PCID):
```rust
KERNEL_PML4.store(pml4_phys | 1 << 63, Ordering::Relaxed);
```
Now trampolines automatically load CR3 with PCID enabled since they use `KERNEL_PML4`.

### 3. Idle CPU %99-100 - `src/gui/event_loop.rs` line 529
**Problem**: Event loop ran continuously without calling `idle_pause()`, keeping CPU at 100% even when idle.

**Fix**: Added `arch::x86_64::apic::idle_pause()` at start of idle loop. HLT when `BSP_IDLE_HLT` is true (APIC timer verified), spin-loop otherwise.

### 4. GlobalDirtyTracker optimization - `src/gui/dirty_rect.rs`
**Problem**: Once 64 rects were tracked, `needs_full_redraw()` always returned true, forcing full redraws and losing dirty-region optimization.

**Fix**: 
- Added `bounding` field tracking union of all rects
- `track()` updates bounding rect even after overflow
- `needs_full_redraw()` checks bounding rect dimensions
- `bounding_rect()` uses tracked rect instead of recomputing

This allows partial redraws even with >64 dirty rects.

## Build Status
`cargo build -Zjson-target-spec --target x86_64-minos.json --release` - **SUCCESS**