use core::arch::naked_asm;

#[inline(never)] // FIXME(rust-lang/rust#148307)
pub fn wasmtime_continuation_start_address() -> *const () {
    wasmtime_continuation_start as *const ()
}

/// Entry trampoline for a freshly initialized continuation stack.
///
/// On entry x29 points at the control context's saved-frame-pointer word and
/// SP points at four words containing, in order, the return count, args array,
/// caller vmctx, and function reference.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn wasmtime_continuation_start() {
    naked_asm!(
        "
        ldr x3, [sp]
        ldr x2, [sp, #8]
        ldr x1, [sp, #16]
        ldr x0, [sp, #24]
        bl {fiber_start}

        cbz w0, 2f
        mov x0, #0
        b 3f
    2:
        mov x0, {trap_control_effect}
    3:
        // x29 addresses the middle word of the control context. The stack
        // switch that entered this continuation installed the parent FP, SP,
        // and PC into these three words.
        ldr x16, [x29, #8]
        ldr x17, [x29, #-8]
        ldr x18, [x29]
        mov sp, x17
        mov x29, x18
        br x16
        ",
        fiber_start = sym super::fiber_start,
        trap_control_effect = const crate::vm::CONTROL_EFFECT_TRAP_ENCODING,
    );
}

#[test]
fn test_control_effect_payloads() {
    assert_eq!(wasmtime_environ::CONTROL_EFFECT_RETURN_DISCRIMINANT, 0);
}
