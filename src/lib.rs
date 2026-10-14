//! A minimal wrapper around the x87 80-bit extended-precision format.
//!
//! This crate uses x86 x87 instructions and is supported on x86 and x86_64 targets.

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
compile_error!("x87 supports only x86 and x86_64 targets");

use core::arch::asm;
use core::ops::Add;

/// An x87 80-bit extended-precision floating-point value.
///
/// Values can be created from `f64`, added, and converted back to `f64`.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct f80 {
    bytes: [u8; 10],
}

impl Add for f80 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        let mut result = Self { bytes: [0; 10] };

        unsafe {
            asm!(
                "fld tbyte ptr [{a}]",
                "fld tbyte ptr [{b}]",
                "faddp st(1), st(0)",
                "fstp tbyte ptr [{out}]",

                a = in(reg) self.bytes.as_ptr(),
                b = in(reg) rhs.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),

                out("st(0)") _,
                out("st(1)") _,
                out("st(2)") _,
                out("st(3)") _,
                out("st(4)") _,
                out("st(5)") _,
                out("st(6)") _,
                out("st(7)") _,
            );
        }

        result
    }
}

impl f80 {
    /// Converts this value to an IEEE 754 `f64` value.
    #[inline]
    pub fn to_f64(self) -> f64 {
        let mut result = 0.0_f64;

        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fstp qword ptr [{dst}]",

                src = in(reg) self.bytes.as_ptr(),
                dst = in(reg) &mut result,

                out("st(0)") _,
                out("st(1)") _,
                out("st(2)") _,
                out("st(3)") _,
                out("st(4)") _,
                out("st(5)") _,
                out("st(6)") _,
                out("st(7)") _,
            );
        }

        result
    }
}

impl From<f80> for f64 {
    #[inline]
    fn from(value: f80) -> Self {
        value.to_f64()
    }
}

impl From<f64> for f80 {
    #[inline]
    fn from(value: f64) -> Self {
        let mut result = Self { bytes: [0; 10] };

        unsafe {
            asm!(
                "fld qword ptr [{src}]",
                "fstp tbyte ptr [{dst}]",

                src = in(reg) &value,
                dst = in(reg) result.bytes.as_mut_ptr(),

                out("st(0)") _,
                out("st(1)") _,
                out("st(2)") _,
                out("st(3)") _,
                out("st(4)") _,
                out("st(5)") _,
                out("st(6)") _,
                out("st(7)") _,
            );
        }

        result
    }
}
