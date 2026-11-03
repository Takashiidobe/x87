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

