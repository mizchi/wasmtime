use crate::{isa::aarch64::inst::xreg, machinst::Reg};

pub struct ControlContextLayout {
    pub stack_pointer_offset: u32,
    pub frame_pointer_offset: u32,
    pub ip_offset: u32,
}

pub fn control_context_layout() -> ControlContextLayout {
    ControlContextLayout {
        stack_pointer_offset: 0,
        frame_pointer_offset: 8,
        ip_offset: 16,
    }
}

pub fn payload_register() -> Reg {
    xreg(0)
}
