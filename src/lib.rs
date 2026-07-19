use core::arch::asm;
use core::ops::Add;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct F80 {
    bytes: [u8; 10],
}

impl Add for F80 {
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

impl From<f64> for F80 {
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
