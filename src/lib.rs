//! A minimal wrapper around the x87 80-bit extended-precision format.
//!
//! This crate uses x86 x87 instructions and is supported on x86 and x86_64 targets.

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
compile_error!("x87 supports only x86 and x86_64 targets");

use core::arch::asm;
use core::cmp::Ordering;
use core::fmt;
use core::num::FpCategory;
use core::ops::{
    Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Rem, RemAssign, Sub, SubAssign,
};

const FMA_LIMBS: usize = 772;
type FmaBig = [u64; FMA_LIMBS];

fn fma_put(big: &mut FmaBig, value: u128, shift: usize) {
    let mut bit = 0;
    while bit < 128 {
        if value & (1u128 << bit) != 0 {
            let position = shift + bit;
            big[position / 64] |= 1u64 << (position % 64);
        }
        bit += 1;
    }
}

fn fma_cmp(left: &FmaBig, right: &FmaBig) -> Ordering {
    let mut i = FMA_LIMBS;
    while i != 0 {
        i -= 1;
        match left[i].cmp(&right[i]) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

fn fma_add(left: &FmaBig, right: &FmaBig) -> FmaBig {
    let mut result = [0; FMA_LIMBS];
    let mut carry = false;
    let mut i = 0;
    while i < FMA_LIMBS {
        let (sum, carry_a) = left[i].overflowing_add(right[i]);
        let (sum, carry_b) = sum.overflowing_add(carry as u64);
        result[i] = sum;
        carry = carry_a || carry_b;
        i += 1;
    }
    result
}

fn fma_sub(left: &FmaBig, right: &FmaBig) -> FmaBig {
    let mut result = [0; FMA_LIMBS];
    let mut borrow = false;
    let mut i = 0;
    while i < FMA_LIMBS {
        let (difference, borrow_a) = left[i].overflowing_sub(right[i]);
        let (difference, borrow_b) = difference.overflowing_sub(borrow as u64);
        result[i] = difference;
        borrow = borrow_a || borrow_b;
        i += 1;
    }
    result
}

fn fma_highest(big: &FmaBig) -> Option<usize> {
    let mut i = FMA_LIMBS;
    while i != 0 {
        i -= 1;
        if big[i] != 0 {
            return Some(i * 64 + (63 - big[i].leading_zeros() as usize));
        }
    }
    None
}

fn fma_bit(big: &FmaBig, bit: usize) -> bool {
    big[bit / 64] & (1u64 << (bit % 64)) != 0
}

fn fma_any_below(big: &FmaBig, bit: usize) -> bool {
    let words = bit / 64;
    let mut i = 0;
    while i < words {
        if big[i] != 0 {
            return true;
        }
        i += 1;
    }
    let remaining = bit % 64;
    remaining != 0 && big[words] & ((1u64 << remaining) - 1) != 0
}

fn fma_extract_u64(big: &FmaBig, shift: usize) -> u64 {
    let mut value = 0u64;
    let mut bit = 0;
    while bit < 64 {
        if shift + bit < FMA_LIMBS * 64 && fma_bit(big, shift + bit) {
            value |= 1u64 << bit;
        }
        bit += 1;
    }
    value
}

fn fma_round_up(mode: u16, negative: bool, significand: u64, guard: bool, sticky: bool) -> bool {
    let inexact = guard || sticky;
    match mode {
        0 => guard && (sticky || significand & 1 != 0),
        1 => negative && inexact,
        2 => !negative && inexact,
        _ => false,
    }
}

fn fma_overflow(negative: bool, mode: u16) -> f80 {
    let infinity = match mode {
        0 => true,
        1 => negative,
        2 => !negative,
        _ => false,
    };
    if infinity {
        if negative {
            f80::NEG_INFINITY
        } else {
            f80::INFINITY
        }
    } else if negative {
        f80::MIN
    } else {
        f80::MAX
    }
}

fn fma_round_exact(big: &FmaBig, base_exponent: i32, negative: bool, mode: u16) -> f80 {
    let Some(highest) = fma_highest(big) else {
        return if mode == 1 { f80::NEG_ZERO } else { f80::ZERO };
    };
    let mut exponent = highest as i32 + base_exponent;
    if exponent > 16383 {
        return fma_overflow(negative, mode);
    }

    if exponent >= -16382 {
        let shift = highest as i32 - 63;
        let mut significand = if shift >= 0 {
            fma_extract_u64(big, shift as usize)
        } else {
            fma_extract_u64(big, 0) << (-shift as u32)
        };
        let guard = shift > 0 && fma_bit(big, shift as usize - 1);
        let sticky = shift > 1 && fma_any_below(big, shift as usize - 1);
        if fma_round_up(mode, negative, significand, guard, sticky) {
            significand = significand.wrapping_add(1);
            if significand == 0 {
                significand = 0x8000_0000_0000_0000;
                exponent += 1;
            }
        }
        if exponent > 16383 {
            return fma_overflow(negative, mode);
        }
        f80::from_parts(negative, (exponent + 16383) as u16, significand)
    } else {
        let shift = -16445 - base_exponent;
        let mut significand = if shift >= 0 {
            fma_extract_u64(big, shift as usize)
        } else {
            fma_extract_u64(big, 0) << (-shift as u32)
        };
        let guard = shift > 0 && fma_bit(big, shift as usize - 1);
        let sticky = shift > 1 && fma_any_below(big, shift as usize - 1);
        if fma_round_up(mode, negative, significand, guard, sticky) {
            significand = significand.wrapping_add(1);
        }
        if significand >= 0x8000_0000_0000_0000 {
            f80::from_parts(negative, 1, 0x8000_0000_0000_0000)
        } else {
            f80::from_parts(negative, 0, significand)
        }
    }
}

fn x87_rounding_mode() -> u16 {
    let mut control = 0u16;
    unsafe {
        asm!("fnstcw [{cw}]", cw = in(reg) &mut control, options(nostack));
    }
    (control >> 10) & 3
}

/// An x87 80-bit extended-precision floating-point value.
#[cfg_attr(target_arch = "x86_64", repr(C, align(16)))]
#[cfg_attr(target_arch = "x86", repr(C, align(4)))]
#[derive(Clone, Copy)]
pub struct f80 {
    bytes: [u8; 10],
}

pub trait F80Integer: Sized {
    fn from_f80_truncating(value: f80) -> Self;
}

macro_rules! impl_f80_integer_signed {
    ($($ty:ty),* $(,)?) => {$ (
        impl F80Integer for $ty {
            fn from_f80_truncating(value: f80) -> Self {
                value.to_signed(<$ty>::MIN as i128, <$ty>::MAX as i128) as $ty
            }
        }
    )*};
}

macro_rules! impl_f80_integer_unsigned {
    ($($ty:ty),* $(,)?) => {$ (
        impl F80Integer for $ty {
            fn from_f80_truncating(value: f80) -> Self {
                value.to_unsigned(<$ty>::MAX as u128) as $ty
            }
        }
    )*};
}

impl_f80_integer_signed!(i8, i16, i32, i64, i128, isize);
impl_f80_integer_unsigned!(u8, u16, u32, u64, u128, usize);

impl f80 {
    pub const BITS: u32 = 80;
    pub const ZERO: Self = Self::from_parts(false, 0, 0);
    pub const NEG_ZERO: Self = Self::from_parts(true, 0, 0);
    pub const INFINITY: Self = Self::from_parts(false, 0x7fff, 0x8000_0000_0000_0000);
    pub const NEG_INFINITY: Self = Self::from_parts(true, 0x7fff, 0x8000_0000_0000_0000);
    pub const NAN: Self = Self::from_parts(false, 0x7fff, 0xc000_0000_0000_0000);
    pub const MIN_POSITIVE: Self = Self::from_parts(false, 1, 0x8000_0000_0000_0000);
    pub const MAX: Self = Self::from_parts(false, 0x7ffe, u64::MAX);
    pub const MIN: Self = Self::from_parts(true, 0x7ffe, u64::MAX);
    pub const EPSILON: Self = Self::from_parts(false, 0x3fc0, 0x8000_0000_0000_0000);
    pub const EXPONENT_MASK: u128 = 0x7fff_0000_0000_0000_0000;
    pub const MANTISSA_MASK: u128 = 0x0000_ffff_ffff_ffff_ffff;
    pub const SIGN_MASK: u128 = 0x8000_0000_0000_0000_0000;
    pub const MAX_EXACT_INTEGER: Self = Self::from_parts(false, 0x403f, 0x8000_0000_0000_0000);
    pub const MIN_EXACT_INTEGER: Self = Self::from_parts(true, 0x403f, 0x8000_0000_0000_0000);
    pub const E: Self = Self::from_parts(false, 0x4000, 0xadf85458a2bb4a9b);
    pub const EULER_GAMMA: Self = Self::from_parts(false, 0x3ffe, 0x93c467e37db0c7a5);
    pub const FRAC_1_PI: Self = Self::from_parts(false, 0x3ffd, 0xa2f9836e4e44152a);
    pub const FRAC_1_SQRT_2: Self = Self::from_parts(false, 0x3ffe, 0xb504f333f9de6484);
    pub const FRAC_2_PI: Self = Self::from_parts(false, 0x3ffe, 0xa2f9836e4e44152a);
    pub const FRAC_2_SQRT_PI: Self = Self::from_parts(false, 0x3fff, 0x906eba8214db688d);
    pub const FRAC_PI_2: Self = Self::from_parts(false, 0x3fff, 0xc90fdaa22168c235);
    pub const FRAC_PI_3: Self = Self::from_parts(false, 0x3fff, 0x860a91c16b9b2c23);
    pub const FRAC_PI_4: Self = Self::from_parts(false, 0x3ffe, 0xc90fdaa22168c235);
    pub const FRAC_PI_6: Self = Self::from_parts(false, 0x3ffe, 0x860a91c16b9b2c23);
    pub const FRAC_PI_8: Self = Self::from_parts(false, 0x3ffd, 0xc90fdaa22168c235);
    pub const GOLDEN_RATIO: Self = Self::from_parts(false, 0x3fff, 0xcf1bbcdcbfa53e0b);
    pub const LN_2: Self = Self::from_parts(false, 0x3ffe, 0xb17217f7d1cf79ac);
    pub const LN_10: Self = Self::from_parts(false, 0x4000, 0x935d8dddaaa8ac17);
    pub const LOG2_10: Self = Self::from_parts(false, 0x4000, 0xd49a784bcd1b8afe);
    pub const LOG2_E: Self = Self::from_parts(false, 0x3fff, 0xb8aa3b295c17f0bc);
    pub const LOG10_2: Self = Self::from_parts(false, 0x3ffd, 0x9a209a84fbcff799);
    pub const LOG10_E: Self = Self::from_parts(false, 0x3ffd, 0xde5bd8a937287195);
    pub const PI: Self = Self::from_parts(false, 0x4000, 0xc90fdaa22168c235);
    pub const SQRT_2: Self = Self::from_parts(false, 0x3fff, 0xb504f333f9de6484);
    pub const TAU: Self = Self::from_parts(false, 0x4001, 0xc90fdaa22168c235);
    pub const FRAC_1_SQRT_3: Self = Self::from_parts(false, 0x3ffe, 0x93cd3a2c8198e269);
    pub const FRAC_1_SQRT_5: Self = Self::from_parts(false, 0x3ffd, 0xe4f92e2dff6ec9ab);
    pub const FRAC_1_SQRT_2PI: Self = Self::from_parts(false, 0x3ffd, 0xcc42299ea1b28468);
    pub const FRAC_1_SQRT_PI: Self = Self::from_parts(false, 0x3ffe, 0x906eba8214db688d);
    pub const SQRT_3: Self = Self::from_parts(false, 0x3fff, 0xddb3d742c265539e);
    pub const SQRT_5: Self = Self::from_parts(false, 0x4000, 0x8f1bbcdcbfa53e0b);
    pub const RADIX: u32 = 2;
    pub const MANTISSA_DIGITS: u32 = 64;
    pub const DIGITS: u32 = 18;
    pub const MIN_EXP: i32 = -16381;
    pub const MAX_EXP: i32 = 16384;
    pub const MIN_10_EXP: i32 = -4931;
    pub const MAX_10_EXP: i32 = 4932;

    const fn from_parts(negative: bool, exponent: u16, significand: u64) -> Self {
        let sig = significand.to_le_bytes();
        let exp = (exponent | if negative { 0x8000 } else { 0 }).to_le_bytes();
        Self {
            bytes: [
                sig[0], sig[1], sig[2], sig[3], sig[4], sig[5], sig[6], sig[7], exp[0], exp[1],
            ],
        }
    }

    pub const fn is_nan(self) -> bool {
        let (_, exponent, significand) = self.integer_parts();
        exponent == 0x7fff && significand != 0x8000_0000_0000_0000
    }

    pub const fn is_infinite(self) -> bool {
        let (_, exponent, significand) = self.integer_parts();
        exponent == 0x7fff && significand == 0x8000_0000_0000_0000
    }

    pub const fn is_finite(self) -> bool {
        self.integer_parts().1 != 0x7fff
    }
    pub const fn is_zero(self) -> bool {
        self.integer_parts().1 == 0 && self.integer_parts().2 == 0
    }
    pub const fn is_subnormal(self) -> bool {
        let (_, exponent, significand) = self.integer_parts();
        exponent == 0 && significand != 0 && significand & 0x8000_0000_0000_0000 == 0
    }
    pub const fn is_normal(self) -> bool {
        let (_, e, s) = self.integer_parts();
        (e != 0 && e != 0x7fff) || (e == 0 && s & 0x8000_0000_0000_0000 != 0)
    }
    pub const fn is_sign_negative(self) -> bool {
        self.integer_parts().0
    }
    pub const fn is_sign_positive(self) -> bool {
        !self.is_sign_negative()
    }

    pub const fn classify(self) -> FpCategory {
        let (_, exponent, significand) = self.integer_parts();
        match exponent {
            0 if significand == 0 => FpCategory::Zero,
            0 if significand & 0x8000_0000_0000_0000 == 0 => FpCategory::Subnormal,
            0 => FpCategory::Normal,
            0x7fff if significand == 0x8000_0000_0000_0000 => FpCategory::Infinite,
            0x7fff => FpCategory::Nan,
            _ => FpCategory::Normal,
        }
    }

    pub const fn to_bits(self) -> u128 {
        let mut bits = 0u128;
        let mut i = 0;
        while i < 10 {
            bits |= (self.bytes[i] as u128) << (i * 8);
            i += 1;
        }
        bits
    }

    pub const fn from_bits(bits: u128) -> Self {
        assert!(bits >> 80 == 0);
        let mut bytes = [0; 10];
        let mut i = 0;
        while i < 10 {
            bytes[i] = (bits >> (i * 8)) as u8;
            i += 1;
        }
        Self { bytes }
    }

    pub const fn to_le_bytes(self) -> [u8; 10] {
        self.bytes
    }

    pub const fn to_be_bytes(self) -> [u8; 10] {
        let mut bytes = self.bytes;
        let mut i = 0;
        while i < 5 {
            let j = 9 - i;
            bytes.swap(i, j);
            i += 1;
        }
        bytes
    }

    #[cfg(target_endian = "little")]
    pub const fn to_ne_bytes(self) -> [u8; 10] {
        self.to_le_bytes()
    }

    #[cfg(target_endian = "big")]
    pub const fn to_ne_bytes(self) -> [u8; 10] {
        self.to_be_bytes()
    }

    pub fn total_cmp(&self, other: &Self) -> Ordering {
        const SIGN: u128 = 1 << 79;
        const MASK: u128 = (1 << 80) - 1;
        let left = self.to_bits();
        let right = other.to_bits();
        let left = if left & SIGN != 0 {
            !left & MASK
        } else {
            left | SIGN
        };
        let right = if right & SIGN != 0 {
            !right & MASK
        } else {
            right | SIGN
        };
        left.cmp(&right)
    }

    pub fn next_up(self) -> Self {
        if self.is_nan() || self == Self::INFINITY {
            return self;
        }
        if self.is_zero() {
            return Self::from_bits(1);
        }
        let bits = self.to_bits();
        Self::from_bits(if self.is_sign_negative() {
            bits - 1
        } else {
            bits + 1
        })
    }

    pub fn next_down(self) -> Self {
        if self.is_nan() || self == Self::NEG_INFINITY {
            return self;
        }
        if self.is_zero() {
            return Self::from_bits((1 << 79) | 1);
        }
        let bits = self.to_bits();
        Self::from_bits(if self.is_sign_negative() {
            bits + 1
        } else {
            bits - 1
        })
    }

    #[inline]
    pub const fn abs(self) -> Self {
        let mut result = self;
        result.bytes[9] &= 0x7f;
        result
    }

    #[inline]
    pub const fn copysign(self, sign: Self) -> Self {
        let mut result = self;
        result.bytes[9] = (result.bytes[9] & 0x7f) | (sign.bytes[9] & 0x80);
        result
    }

    #[inline]
    pub fn sqrt(self) -> Self {
        let mut result = Self { bytes: [0; 10] };
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fsqrt",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn sin(self) -> Self {
        let nan = f64::NAN;
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fsin",
                "fnstsw ax",
                "sahf",
                "jp 2f",
                "jmp 3f",
                "2:",
                "fstp st(0)",
                "fld qword ptr [{nan}]",
                "3:",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                nan = in(reg) &nan,
                out = in(reg) result.bytes.as_mut_ptr(),
                lateout("ax") _,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn cos(self) -> Self {
        let nan = f64::NAN;
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fcos",
                "fnstsw ax",
                "sahf",
                "jp 2f",
                "jmp 3f",
                "2:",
                "fstp st(0)",
                "fld qword ptr [{nan}]",
                "3:",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                nan = in(reg) &nan,
                out = in(reg) result.bytes.as_mut_ptr(),
                lateout("ax") _,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn sin_cos(self) -> (Self, Self) {
        (self.sin(), self.cos())
    }

    pub fn tan(self) -> Self {
        let nan = f64::NAN;
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fptan",
                "fnstsw ax",
                "sahf",
                "jp 2f",
                "fstp st(0)",
                "jmp 3f",
                "2:",
                "fstp st(0)",
                "fld qword ptr [{nan}]",
                "3:",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                nan = in(reg) &nan,
                out = in(reg) result.bytes.as_mut_ptr(),
                lateout("ax") _,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn atan(self) -> Self {
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fld1",
                "fpatan",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn atan2(self, x: Self) -> Self {
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{y}]",
                "fld tbyte ptr [{x}]",
                "fpatan",
                "fstp tbyte ptr [{out}]",
                y = in(reg) self.bytes.as_ptr(),
                x = in(reg) x.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn ln(self) -> Self {
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fldln2",
                "fld tbyte ptr [{src}]",
                "fyl2x",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn ln_1p(self) -> Self {
        if self.abs() >= Self::from(0.25_f64) {
            return (Self::from(1.0_f64) + self).ln();
        }
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fldln2",
                "fld tbyte ptr [{src}]",
                "fyl2xp1",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn log2(self) -> Self {
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld1",
                "fld tbyte ptr [{src}]",
                "fyl2x",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn log10(self) -> Self {
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fldlg2",
                "fld tbyte ptr [{src}]",
                "fyl2x",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn log(self, base: Self) -> Self {
        self.log2() / base.log2()
    }

    pub fn exp2(self) -> Self {
        if self.is_infinite() {
            return if self.is_sign_negative() {
                Self::ZERO
            } else {
                Self::INFINITY
            };
        }
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fld st(0)",
                "frndint",
                "fxch st(1)",
                "fsub st(0), st(1)",
                "f2xm1",
                "fld1",
                "faddp st(1), st(0)",
                "fscale",
                "fstp st(1)",
                "fstp tbyte ptr [{out}]",
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn exp(self) -> Self {
        (self * Self::LOG2_E).exp2()
    }
    pub fn exp_m1(self) -> Self {
        if self.abs() >= Self::LN_2 {
            return self.exp() - Self::from(1.0_f64);
        }
        let reduced = self * Self::LOG2_E;
        let mut result = Self::ZERO;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "f2xm1",
                "fstp tbyte ptr [{out}]",
                src = in(reg) reduced.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    pub fn powf(self, exponent: Self) -> Self {
        if exponent.is_zero() || self == Self::from(1.0_f64) {
            return Self::from(1.0_f64);
        }
        if self.is_sign_negative() && exponent.is_finite() && exponent.trunc() == exponent {
            let value = (self.abs().log2() * exponent).exp2();
            return if exponent % Self::from(2.0_f64) != Self::ZERO {
                -value
            } else {
                value
            };
        }
        (self.log2() * exponent).exp2()
    }

    pub fn mul_add(self, multiplier: Self, addend: Self) -> Self {
        let (negative_a, exponent_a, significand_a) = self.integer_parts();
        let (negative_b, exponent_b, significand_b) = multiplier.integer_parts();
        let (negative_c, exponent_c, significand_c) = addend.integer_parts();
        let mode = x87_rounding_mode();

        if self.is_nan() || multiplier.is_nan() || addend.is_nan() {
            return Self::NAN;
        }

        let a_infinite = self.is_infinite();
        let b_infinite = multiplier.is_infinite();
        let c_infinite = addend.is_infinite();
        let product_negative = negative_a ^ negative_b;
        let product_zero = significand_a == 0 || significand_b == 0;

        if (a_infinite && significand_b == 0) || (b_infinite && significand_a == 0) {
            return Self::NAN;
        }

        if a_infinite || b_infinite {
            if c_infinite && negative_c != product_negative {
                return Self::NAN;
            }
            return if product_negative {
                Self::NEG_INFINITY
            } else {
                Self::INFINITY
            };
        }

        if c_infinite {
            return addend;
        }

        if (exponent_a != 0 && significand_a & 0x8000_0000_0000_0000 == 0)
            || (exponent_b != 0 && significand_b & 0x8000_0000_0000_0000 == 0)
            || (exponent_c != 0 && significand_c & 0x8000_0000_0000_0000 == 0)
        {
            return Self::NAN;
        }

        if product_zero {
            if significand_c != 0 {
                return addend;
            }
            let negative = if product_negative == negative_c {
                product_negative
            } else {
                mode == 1
            };
            return if negative { Self::NEG_ZERO } else { Self::ZERO };
        }

        let unit_a = if exponent_a == 0 {
            -16445
        } else {
            exponent_a as i32 - 16446
        };
        let unit_b = if exponent_b == 0 {
            -16445
        } else {
            exponent_b as i32 - 16446
        };
        let unit_c = if exponent_c == 0 {
            -16445
        } else {
            exponent_c as i32 - 16446
        };
        let product_exponent = unit_a + unit_b;
        let base_exponent = product_exponent.min(unit_c);
        let mut product = [0; FMA_LIMBS];
        let mut c = [0; FMA_LIMBS];
        fma_put(
            &mut product,
            significand_a as u128 * significand_b as u128,
            (product_exponent - base_exponent) as usize,
        );
        fma_put(
            &mut c,
            significand_c as u128,
            (unit_c - base_exponent) as usize,
        );

        let (magnitude, negative) = if product_negative == negative_c {
            (fma_add(&product, &c), product_negative)
        } else {
            match fma_cmp(&product, &c) {
                Ordering::Greater => (fma_sub(&product, &c), product_negative),
                Ordering::Less => (fma_sub(&c, &product), negative_c),
                Ordering::Equal => {
                    return if mode == 1 {
                        Self::NEG_ZERO
                    } else {
                        Self::ZERO
                    };
                }
            }
        };
        fma_round_exact(&magnitude, base_exponent, negative, mode)
    }

    pub fn to_int_unchecked<T: F80Integer>(self) -> T {
        T::from_f80_truncating(self)
    }

    pub fn ln_gamma(self) -> (Self, i32) {
        if self.is_nan() {
            return (Self::NAN, 0);
        }
        if self == Self::INFINITY {
            return (Self::INFINITY, 1);
        }
        if self == Self::NEG_INFINITY {
            return (Self::NAN, 0);
        }

        if self.is_zero() {
            return (Self::INFINITY, if self.is_sign_negative() { -1 } else { 1 });
        }
        if self < Self::ZERO && self.trunc() == self {
            return (Self::INFINITY, 0);
        }

        if self < Self::from(0.5_f64) {
            let sine = (Self::PI * self).sin();
            let (reflected, sign) = (Self::from(1.0_f64) - self).ln_gamma();
            let sign = if sine.is_sign_negative() { -sign } else { sign };
            return (Self::PI.ln() - sine.abs().ln() - reflected, sign);
        }

        let p = [
            Self::from_parts(false, 0x3ffe, 0xffff_ffff_ffca_802d),
            Self::from_parts(false, 0x4008, 0xa921_4db6_1858_27fb),
            Self::from_parts(true, 0x4009, 0x9d64_7476_a0b8_da3a),
            Self::from_parts(false, 0x4008, 0xc0d4_b30e_9da6_bc14),
            Self::from_parts(true, 0x4006, 0xb09d_728d_197a_eb65),
            Self::from_parts(false, 0x4002, 0xc81e_13fc_5ce4_063a),
            Self::from_parts(true, 0x3ffc, 0x8de5_94c9_5942_a7bc),
            Self::from_parts(false, 0x3fee, 0xa782_8a72_7192_7495),
            Self::from_parts(false, 0x3fe8, 0xa1aa_8479_fd77_7636),
        ];
        let z = self - Self::from(1.0_f64);
        let mut sum = p[0];
        let mut i = 1;
        while i < p.len() {
            sum += p[i] / (z + Self::from(i as i32));
            i += 1;
        }
        let t = z + Self::from(7.5_f64);
        let log_sqrt_two_pi = Self::from(0.5_f64) * (Self::from(2.0_f64) * Self::PI).ln();
        (
            log_sqrt_two_pi + (z + Self::from(0.5_f64)) * t.ln() - t + sum.ln(),
            1,
        )
    }

    pub fn gamma(self) -> Self {
        if self == Self::NEG_INFINITY {
            return Self::NAN;
        }
        let (log_abs, sign) = self.ln_gamma();
        if sign == 0 {
            return Self::INFINITY;
        }
        log_abs.exp().copysign(Self::from(sign))
    }

    pub fn erf(self) -> Self {
        if self.is_nan() {
            return self;
        }
        if self.is_infinite() {
            return Self::from(1.0_f64).copysign(self);
        }
        let x = self.abs();
        if x > Self::from(3.5_f64) {
            return Self::from(1.0_f64) - self.erfc();
        }
        let x2 = x * x;
        let mut term = x;
        let mut sum = x;
        let mut n = 1u32;
        while n < 160 {
            term = term * (-x2) / Self::from(n) * Self::from((2 * n - 1) as i32)
                / Self::from((2 * n + 1) as i32);
            let next = sum + term;
            if next == sum {
                break;
            }
            sum = next;
            n += 1;
        }
        (Self::FRAC_2_SQRT_PI * sum).copysign(self)
    }

    pub fn erfc(self) -> Self {
        if self.is_nan() {
            return self;
        }
        if self.is_infinite() {
            return if self.is_sign_negative() {
                Self::from(2.0_f64)
            } else {
                Self::ZERO
            };
        }
        let x = self.abs();
        let complement = if x <= Self::from(3.5_f64) {
            Self::from(1.0_f64) - x.erf()
        } else {
            let x2 = x * x;
            let mut term = Self::from(1.0_f64);
            let mut sum = term;
            let mut n = 1u32;
            while n <= 32 {
                term = term * Self::from(-((2 * n - 1) as i32)) / (Self::from(2.0_f64) * x2);
                if term.abs() > sum.abs() {
                    break;
                }
                sum += term;
                n += 1;
            }
            (-x2).exp() * Self::FRAC_1_SQRT_PI * sum / x
        };
        if self.is_sign_negative() {
            Self::from(2.0_f64) - complement
        } else {
            complement
        }
    }

    pub fn asin(self) -> Self {
        (self / (Self::from(1.0_f64) - self * self).sqrt()).atan()
    }
    pub fn acos(self) -> Self {
        Self::FRAC_PI_2 - self.asin()
    }
    pub fn acosh(self) -> Self {
        (self + (self - Self::from(1.0_f64)).sqrt() * (self + Self::from(1.0_f64)).sqrt()).ln()
    }
    pub fn asinh(self) -> Self {
        self.abs().asinh_positive().copysign(self)
    }
    fn asinh_positive(self) -> Self {
        (self + self.hypot(Self::from(1.0_f64))).ln()
    }
    pub fn atanh(self) -> Self {
        ((Self::from(1.0_f64) + self) / (Self::from(1.0_f64) - self)).ln() / Self::from(2.0_f64)
    }
    pub fn cbrt(self) -> Self {
        self.abs()
            .powf(Self::from(1.0_f64) / Self::from(3.0_f64))
            .copysign(self)
    }
    pub fn cosh(self) -> Self {
        (self.exp() + (-self).exp()) / Self::from(2.0_f64)
    }
    pub fn sinh(self) -> Self {
        (self.exp() - (-self).exp()) / Self::from(2.0_f64)
    }
    pub fn tanh(self) -> Self {
        let exponential = (Self::from(-2.0_f64) * self.abs()).exp();
        ((Self::from(1.0_f64) - exponential) / (Self::from(1.0_f64) + exponential))
            .copysign(self)
    }

    pub fn floor(self) -> Self {
        self.round_integral(1)
    }
    pub fn ceil(self) -> Self {
        self.round_integral(2)
    }
    pub fn trunc(self) -> Self {
        self.round_integral(3)
    }

    pub fn round(self) -> Self {
        if !self.is_finite() || self.is_zero() {
            return self;
        }
        let half = Self::from(0.5_f64);
        if self.is_sign_negative() {
            (self - half).ceil()
        } else {
            (self + half).floor()
        }
    }

    pub fn fract(self) -> Self {
        self - self.trunc()
    }

    pub fn round_ties_even(self) -> Self {
        self.round_integral(0)
    }

    pub fn abs_sub(self, other: Self) -> Self {
        if self.is_nan() || other.is_nan() {
            self + other
        } else if self > other {
            self - other
        } else {
            Self::ZERO
        }
    }

    pub fn algebraic_add(self, other: Self) -> Self {
        self + other
    }
    pub fn algebraic_sub(self, other: Self) -> Self {
        self - other
    }
    pub fn algebraic_mul(self, other: Self) -> Self {
        self * other
    }
    pub fn algebraic_div(self, other: Self) -> Self {
        self / other
    }
    pub fn algebraic_rem(self, other: Self) -> Self {
        self % other
    }

    pub fn signum(self) -> Self {
        if self.is_nan() {
            self
        } else if self.is_sign_negative() {
            Self::from(-1.0_f64)
        } else {
            Self::from(1.0_f64)
        }
    }

    pub fn to_degrees(self) -> Self {
        self * (Self::from(180.0_f64) / Self::PI)
    }
    pub fn to_radians(self) -> Self {
        self * (Self::PI / Self::from(180.0_f64))
    }

    pub fn div_euclid(self, rhs: Self) -> Self {
        let q = (self / rhs).trunc();
        let r = self - q * rhs;
        if r < Self::ZERO {
            if rhs < Self::ZERO {
                q + Self::from(1.0_f64)
            } else {
                q - Self::from(1.0_f64)
            }
        } else {
            q
        }
    }

    pub fn rem_euclid(self, rhs: Self) -> Self {
        let r = self % rhs;
        if r < Self::ZERO { r + rhs.abs() } else { r }
    }

    pub fn clamp_magnitude(self, min: Self, max: Self) -> Self {
        assert!(min >= Self::ZERO && min <= max);
        let magnitude = self.abs();
        if magnitude > max {
            max.copysign(self)
        } else if magnitude < min {
            min.copysign(self)
        } else {
            self
        }
    }

    pub fn midpoint(self, other: Self) -> Self {
        if self == other {
            return self;
        }
        if self.is_infinite() || other.is_infinite() {
            return self + other;
        }
        let two = Self::from(2.0_f64);
        if self.is_sign_negative() != other.is_sign_negative() {
            (self + other) / two
        } else {
            self + (other - self) / two
        }
    }

    pub fn hypot(self, other: Self) -> Self {
        let a = self.abs();
        let b = other.abs();
        if a.is_infinite() || b.is_infinite() {
            return Self::INFINITY;
        }
        if a.is_nan() || b.is_nan() {
            return Self::NAN;
        }
        let (large, small) = if a >= b { (a, b) } else { (b, a) };
        if large.is_zero() {
            Self::ZERO
        } else {
            large * (Self::from(1.0_f64) + (small / large) * (small / large)).sqrt()
        }
    }

    pub fn minimum(self, other: Self) -> Self {
        if self.is_nan() || other.is_nan() {
            Self::NAN
        } else {
            self.min(other)
        }
    }

    pub fn maximum(self, other: Self) -> Self {
        if self.is_nan() || other.is_nan() {
            Self::NAN
        } else {
            self.max(other)
        }
    }

    pub fn min(self, other: Self) -> Self {
        match (self.is_nan(), other.is_nan()) {
            (true, true) => self,
            (true, false) => other,
            (false, true) => self,
            (false, false) if self == other && self.is_zero() => {
                if self.is_sign_negative() {
                    self
                } else {
                    other
                }
            }
            (false, false) => {
                if self < other {
                    self
                } else {
                    other
                }
            }
        }
    }

    pub fn max(self, other: Self) -> Self {
        match (self.is_nan(), other.is_nan()) {
            (true, true) => self,
            (true, false) => other,
            (false, true) => self,
            (false, false) if self == other && self.is_zero() => {
                if self.is_sign_positive() {
                    self
                } else {
                    other
                }
            }
            (false, false) => {
                if self > other {
                    self
                } else {
                    other
                }
            }
        }
    }

    pub fn clamp(self, min: Self, max: Self) -> Self {
        assert!(min <= max);
        self.max(min).min(max)
    }

    fn round_integral(self, mode: u16) -> Self {
        let mut result = Self { bytes: [0; 10] };
        let mut old_control: u16 = 0;
        let new_control: u16;
        unsafe {
            asm!("fnstcw [{cw}]", cw = in(reg) &mut old_control, options(nostack));
        }
        new_control = (old_control & !0x0c00) | (mode << 10);
        unsafe {
            asm!(
                "fldcw [{new}]",
                "fld tbyte ptr [{src}]",
                "frndint",
                "fstp tbyte ptr [{out}]",
                "fldcw [{old}]",
                new = in(reg) &new_control,
                old = in(reg) &old_control,
                src = in(reg) self.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    #[inline]
    pub fn recip(self) -> Self {
        Self::from(1.0_f64) / self
    }

    pub fn powi(self, n: i32) -> Self {
        if n == 0 {
            return Self::from(1.0_f64);
        }
        let mut exponent = n.unsigned_abs();
        let mut base = self;
        let mut result = Self::from(1.0_f64);
        while exponent != 0 {
            if exponent & 1 != 0 {
                result *= base;
            }
            exponent >>= 1;
            if exponent != 0 {
                base = base * base;
            }
        }
        if n < 0 {
            Self::from(1.0_f64) / result
        } else {
            result
        }
    }

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

impl Default for f80 {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for f80 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_f64(), f)
    }
}

impl fmt::Debug for f80 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("f80").field(&self.to_f64()).finish()
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

impl_binary_op!(Add, add, "faddp st(1), st(0)");
impl_binary_op!(Sub, sub, "fsubp st(1), st(0)");
impl_binary_op!(Mul, mul, "fmulp st(1), st(0)");
impl_binary_op!(Div, div, "fdivp st(1), st(0)");
impl Rem for f80 {
    type Output = Self;

    #[inline]
    fn rem(self, rhs: Self) -> Self {
        let mut result = Self { bytes: [0; 10] };
        unsafe {
            asm!(
                "fld tbyte ptr [{rhs}]",
                "fld tbyte ptr [{lhs}]",
                "2:",
                "fprem",
                "fstsw ax",
                "sahf",
                "jp 2b",
                "fstp st(1)",
                "fstp tbyte ptr [{out}]",
                lhs = in(reg) self.bytes.as_ptr(),
                rhs = in(reg) rhs.bytes.as_ptr(),
                out = in(reg) result.bytes.as_mut_ptr(),
                lateout("ax") _,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }
}

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
impl_assign_op!(RemAssign, rem_assign, %);

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
    const fn integer_parts(self) -> (bool, u16, u64) {
        let significand = self.bytes[0] as u64
            | (self.bytes[1] as u64) << 8
            | (self.bytes[2] as u64) << 16
            | (self.bytes[3] as u64) << 24
            | (self.bytes[4] as u64) << 32
            | (self.bytes[5] as u64) << 40
            | (self.bytes[6] as u64) << 48
            | (self.bytes[7] as u64) << 56;
        let sign_exponent = self.bytes[8] as u16 | (self.bytes[9] as u16) << 8;
        (
            sign_exponent & 0x8000 != 0,
            sign_exponent & 0x7fff,
            significand,
        )
    }

    #[inline]
    fn integer_magnitude(self) -> Option<(bool, u128)> {
        let (negative, exponent, significand) = self.integer_parts();
        if exponent == 0x7fff {
            return None;
        }
        let unbiased = if exponent == 0 {
            1 - 16383
        } else {
            exponent as i32 - 16383
        };
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
                && self.integer_parts().2 != 0x8000_0000_0000_0000 =>
            {
                0
            }
            None => {
                if self.integer_parts().0 {
                    0
                } else {
                    max
                }
            }
            Some((true, _)) => 0,
            Some((false, value)) => value.min(max),
        }
    }

    #[inline]
    fn to_signed(self, min: i128, max: i128) -> i128 {
        match self.integer_magnitude() {
            None if self.integer_parts().1 == 0x7fff
                && self.integer_parts().2 != 0x8000_0000_0000_0000 =>
            {
                0
            }
            None => {
                if self.integer_parts().0 {
                    min
                } else {
                    max
                }
            }
            Some((negative, magnitude)) if negative => {
                if magnitude >= min.unsigned_abs() {
                    min
                } else {
                    -(magnitude as i128)
                }
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
    pub fn to_f32(self) -> f32 {
        let mut result = 0.0_f32;
        unsafe {
            asm!(
                "fld tbyte ptr [{src}]",
                "fstp dword ptr [{dst}]",
                src = in(reg) self.bytes.as_ptr(),
                dst = in(reg) &mut result,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            );
        }
        result
    }

    #[inline]
    pub fn to_i8(self) -> i8 {
        self.to_signed(i8::MIN as i128, i8::MAX as i128) as i8
    }
    #[inline]
    pub fn to_i16(self) -> i16 {
        self.to_signed(i16::MIN as i128, i16::MAX as i128) as i16
    }
    #[inline]
    pub fn to_i32(self) -> i32 {
        self.to_signed(i32::MIN as i128, i32::MAX as i128) as i32
    }
    #[inline]
    pub fn to_i64(self) -> i64 {
        self.to_signed(i64::MIN as i128, i64::MAX as i128) as i64
    }
    #[inline]
    pub fn to_i128(self) -> i128 {
        self.to_signed(i128::MIN, i128::MAX)
    }
    #[inline]
    pub fn to_isize(self) -> isize {
        self.to_signed(isize::MIN as i128, isize::MAX as i128) as isize
    }

    #[inline]
    pub fn to_u8(self) -> u8 {
        self.to_unsigned(u8::MAX as u128) as u8
    }
    #[inline]
    pub fn to_u16(self) -> u16 {
        self.to_unsigned(u16::MAX as u128) as u16
    }
    #[inline]
    pub fn to_u32(self) -> u32 {
        self.to_unsigned(u32::MAX as u128) as u32
    }
    #[inline]
    pub fn to_u64(self) -> u64 {
        self.to_unsigned(u64::MAX as u128) as u64
    }
    #[inline]
    pub fn to_u128(self) -> u128 {
        self.to_unsigned(u128::MAX)
    }
    #[inline]
    pub fn to_usize(self) -> usize {
        self.to_unsigned(usize::MAX as u128) as usize
    }
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
    fn from(value: f32) -> Self {
        Self::from(value as f64)
    }
}

impl From<f80> for f64 {
    #[inline]
    fn from(value: f80) -> Self {
        value.to_f64()
    }
}

impl From<f80> for f32 {
    #[inline]
    fn from(value: f80) -> Self {
        value.to_f32()
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
