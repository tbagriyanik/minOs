//! Kernel context switching.
//!
//! This is deliberately separate from the interrupt entry code: the timer
//! interrupt requests a reschedule, while the scheduler performs this switch
//! at a safe scheduler boundary.  The routine saves the callee-saved ABI
//! registers and the current stack pointer, then resumes the saved return
//! address on the destination stack.

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Context {
    pub rsp: u64,
}

/// Switch from `old` to `new` without returning until `new` yields back.
///
/// Both stacks must contain a valid return address at their top.  The caller
/// owns the stacks and must keep them mapped for the whole lifetime of the
/// context.  This is the primitive used by the preemptive scheduler; it does
/// not invoke Rust while the old stack is being modified.
#[inline(never)]
pub unsafe extern "C" fn switch_context(old: *mut Context, new: *const Context) {
    core::arch::asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",
        "mov rsp, [rsi]",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "ret",
        in("rdi") old,
        in("rsi") new,
        clobber_abi("C"),
    );
}
