//! A minimal wrapper around the x87 80-bit extended-precision format.
//!
//! This crate uses x86 x87 instructions and is supported on x86 and x86_64 targets.

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
compile_error!("x87 supports only x86 and x86_64 targets");

use core::arch::asm;
use core::cmp::Ordering;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// An x87 80-bit extended-precision floating-point value.
///
/// Values can be created from `f64`, used in arithmetic and comparisons, and
/// converted back to `f64`.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct f80 {
    bytes: [u8; 10],
}

impl f80 {
    #[inline]
    fn partial_cmp_x87(self, rhs: Self) -> Option<Ordering> {
        let unordered: u8;
        let less: u8;
        let equal: u8;

        unsafe {
            asm!(
                "fld tbyte ptr [{rhs}]",
                "fld tbyte ptr [{lhs}]",
                "fucomip st(0), st(1)",
                "fstp st(0)",
                "setp {unordered}",
                "setb {less}",
                "sete {equal}",

                lhs = in(reg) self.bytes.as_ptr(),
                rhs = in(reg) rhs.bytes.as_ptr(),
                unordered = lateout(reg_byte) unordered,
                less = lateout(reg_byte) less,
                equal = lateout(reg_byte) equal,

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

        if unordered != 0 {
            None
        } else if less != 0 {
            Some(Ordering::Less)
        } else if equal != 0 {
            Some(Ordering::Equal)
        } else {
            Some(Ordering::Greater)
        }
    }
}

impl PartialEq for f80 {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp_x87(*other) == Some(Ordering::Equal)
    }
}

impl PartialOrd for f80 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.partial_cmp_x87(*other)
    }
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

macro_rules! impl_binary_op {
    ($trait:ident, $method:ident, $instruction:literal) => {
        impl $trait for f80 {
            type Output = Self;

            #[inline]
            fn $method(self, rhs: Self) -> Self {
                let mut result = Self { bytes: [0; 10] };

                unsafe {
                    asm!(
                        "fld tbyte ptr [{a}]",
                        "fld tbyte ptr [{b}]",
                        $instruction,
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
    };
}

impl_binary_op!(Sub, sub, "fsubp st(1), st(0)");
impl_binary_op!(Mul, mul, "fmulp st(1), st(0)");
impl_binary_op!(Div, div, "fdivp st(1), st(0)");

macro_rules! impl_assign_op {
    ($trait:ident, $method:ident, $op:tt) => {
        impl $trait for f80 {
            #[inline]
            fn $method(&mut self, rhs: Self) {
                *self = *self $op rhs;
            }
        }
    };
}

impl_assign_op!(AddAssign, add_assign, +);
impl_assign_op!(SubAssign, sub_assign, -);
impl_assign_op!(MulAssign, mul_assign, *);
impl_assign_op!(DivAssign, div_assign, /);

impl Neg for f80 {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        let mut result = Self { bytes: [0; 10] };

        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fchs",
                "fstp tbyte ptr [{out}]",

                src = in(reg) self.bytes.as_ptr(),
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

    #[inline]
    fn integer_parts(self) -> (bool, u16, u64) {
        let significand = u64::from_le_bytes(self.bytes[..8].try_into().unwrap());
        let sign_exponent = u16::from_le_bytes([self.bytes[8], self.bytes[9]]);
        (sign_exponent & 0x8000 != 0, sign_exponent & 0x7fff, significand)
    }

    #[inline]
    fn integer_magnitude(self) -> Option<(bool, u128)> {
        let (negative, exponent, significand) = self.integer_parts();
        if exponent == 0x7fff {
            return None;
        }
        let unbiased = if exponent == 0 { 1 - 16383 } else { exponent as i32 - 16383 };
        if unbiased < 0 {
            return Some((negative, 0));
        }
        if unbiased > 127 {
            return None;
        }
        let shift = unbiased - 63;
        let magnitude = if shift >= 0 {
            (significand as u128).checked_shl(shift as u32)?
        } else {
            (significand as u128) >> (-shift as u32)
        };
        Some((negative, magnitude))
    }

    #[inline]
    fn to_unsigned(self, max: u128) -> u128 {
        match self.integer_magnitude() {
            None if self.integer_parts().1 == 0x7fff
                && self.integer_parts().2 != 0x8000_0000_0000_0000 => 0,
            None => if self.integer_parts().0 { 0 } else { max },
            Some((true, _)) => 0,
            Some((false, value)) => value.min(max),
        }
    }

    #[inline]
    fn to_signed(self, min: i128, max: i128) -> i128 {
        match self.integer_magnitude() {
            None if self.integer_parts().1 == 0x7fff
                && self.integer_parts().2 != 0x8000_0000_0000_0000 => 0,
            None => if self.integer_parts().0 { min } else { max },
            Some((negative, magnitude)) if negative => {
                if magnitude >= min.unsigned_abs() { min } else { -(magnitude as i128) }
            }
            Some((_, magnitude)) => magnitude.min(max as u128) as i128,
        }
    }

    #[inline]
    fn from_i64(value: i64) -> Self {
        let mut result = Self { bytes: [0; 10] };
        unsafe {
            asm!(
                "fild qword ptr [{src}]",
                "fstp tbyte ptr [{dst}]",
                src = in(reg) &value,
                dst = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    #[inline]
    fn from_u64(value: u64) -> Self {
        let high = Self::from_i64((value >> 32) as i64);
        let low = Self::from_i64((value & 0xffff_ffff) as i64);
        high * Self::from(4_294_967_296.0_f64) + low
    }

    #[inline]
    fn from_u128(value: u128) -> Self {
        let high = Self::from_u64((value >> 64) as u64);
        let low = Self::from_u64(value as u64);
        high * Self::from(18_446_744_073_709_551_616.0_f64) + low
    }

    #[inline]
    fn from_i128(value: i128) -> Self {
        let result = Self::from_u128(value.unsigned_abs());
        if value < 0 { -result } else { result }
    }

    #[inline]
    pub fn to_f32(self) -> f32 { self.to_f64() as f32 }

    #[inline]
    pub fn to_i8(self) -> i8 { self.to_signed(i8::MIN as i128, i8::MAX as i128) as i8 }
    #[inline]
    pub fn to_i16(self) -> i16 { self.to_signed(i16::MIN as i128, i16::MAX as i128) as i16 }
    #[inline]
    pub fn to_i32(self) -> i32 { self.to_signed(i32::MIN as i128, i32::MAX as i128) as i32 }
    #[inline]
    pub fn to_i64(self) -> i64 { self.to_signed(i64::MIN as i128, i64::MAX as i128) as i64 }
    #[inline]
    pub fn to_i128(self) -> i128 { self.to_signed(i128::MIN, i128::MAX) }
    #[inline]
    pub fn to_isize(self) -> isize { self.to_signed(isize::MIN as i128, isize::MAX as i128) as isize }

    #[inline]
    pub fn to_u8(self) -> u8 { self.to_unsigned(u8::MAX as u128) as u8 }
    #[inline]
    pub fn to_u16(self) -> u16 { self.to_unsigned(u16::MAX as u128) as u16 }
    #[inline]
    pub fn to_u32(self) -> u32 { self.to_unsigned(u32::MAX as u128) as u32 }
    #[inline]
    pub fn to_u64(self) -> u64 { self.to_unsigned(u64::MAX as u128) as u64 }
    #[inline]
    pub fn to_u128(self) -> u128 { self.to_unsigned(u128::MAX) }
    #[inline]
    pub fn to_usize(self) -> usize { self.to_unsigned(usize::MAX as u128) as usize }
}

macro_rules! impl_from_signed {
    ($($t:ty),* $(,)?) => {$ (
        impl From<$t> for f80 {
            #[inline]
            fn from(value: $t) -> Self { Self::from_i128(value as i128) }
        }
    )*};
}

macro_rules! impl_from_unsigned {
    ($($t:ty),* $(,)?) => {$ (
        impl From<$t> for f80 {
            #[inline]
            fn from(value: $t) -> Self { Self::from_u128(value as u128) }
        }
    )*};
}

impl_from_signed!(i8, i16, i32, i64, i128, isize);
impl_from_unsigned!(u8, u16, u32, u64, u128, usize);

impl From<f32> for f80 {
    #[inline]
    fn from(value: f32) -> Self { Self::from(value as f64) }
}


impl From<f80> for f64 {
    #[inline]
    fn from(value: f80) -> Self {
        value.to_f64()
    }
}

impl From<f80> for f32 {
    #[inline]
    fn from(value: f80) -> Self { value.to_f32() }
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
