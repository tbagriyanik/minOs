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

### 5. Process Fork/Exec/Wait test - `src/mm/process.rs` (`test_process_lifecycle_and_fault_recovery`)
**Problem**: Commit `2e8c51f` disabled `SYS_ALLOC_FRAME` (physical frame addresses must not cross into ring‑3; it now returns `u64::MAX`), but the step‑3 test still expected a real frame. `owned_free_succeeded` was therefore always false, the test returned early with `Hatalı sistem çağrısı sınır koruması: BAŞARISIZ`, and the fork/exec/wait lifecycle (step 6/7) never ran — so the `Process Fork Exec Wait` check failed.

**Fix**:
- Step 3 now asserts the new security contract: `SYS_ALLOC_FRAME` must be rejected (`== u64::MAX`); ownership is tested by recording a PMM‑allocated user frame on the process and freeing it via `SYS_FREE_FRAME` (free count must rise by exactly one page).
- Added `exec_cleared_old_image`: after a successful `exec`, the old data page is no longer mapped (old image torn down); combined with the existing `free_memory_bytes == lifecycle_free_before` and `shared_frame_count() == 0` leak checks.
- Added `fork_err_ok`: `SYS_FORK` with a nonexistent parent returns `u64::MAX` with no allocation / no leak.
- Removed a false‑positive pattern in `test_qemu.ps1` (it matched the `...BASARISIZ` line as a pass).

Verified with a fresh QEMU run: `[PROC-TEST] ... fork/exec/wait yasam dongusu: BASARILI` and `[OK] Process Fork Exec Wait`.

## Build Status
`cargo build -Zjson-target-spec --target x86_64-minos.json --release` - **SUCCESS**
`cargo clippy --release` - **SUCCESS (0 warnings)**