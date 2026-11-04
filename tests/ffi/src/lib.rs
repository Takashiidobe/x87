use core::mem::{align_of, size_of};
use x87::f80;

#[unsafe(no_mangle)]
pub extern "C" fn rust_f80_size() -> usize {
    size_of::<f80>()
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_f80_align() -> usize {
    align_of::<f80>()
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_add(lhs: f80, rhs: f80) -> f80 {
    lhs + rhs
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_sub(lhs: f80, rhs: f80) -> f80 {
    lhs - rhs
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_mul(lhs: f80, rhs: f80) -> f80 {
    lhs * rhs
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_div(lhs: f80, rhs: f80) -> f80 {
    lhs / rhs
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_mul_add(lhs: f80, rhs: f80, addend: f80) -> f80 {
    lhs.mul_add(rhs, addend)
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_tanh(value: f80) -> f80 {
    value.tanh()
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_gamma(value: f80) -> f80 {
    value.gamma()
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_to_f32(value: f80) -> f32 {
    value.to_f32()
}

