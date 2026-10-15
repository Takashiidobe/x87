# x87

`x87` provides an `f80` type backed by the x87 80-bit extended-precision
format. It supports conversion from `f64`, arithmetic, and conversion back to
`f64` on x86 and x86_64. Integer inputs are supported for all Rust scalar
integer types; integer outputs use `to_i*` and `to_u*` methods with truncating,
saturating float-to-integer conversion. `f32` conversions are also supported.

```rust
use x87::f80;

let sum = f80::from(1.25) + f80::from(2.5);
assert_eq!(sum.to_f64(), 3.75);
```

Arithmetic follows the active x87 floating-point environment, including its
rounding mode. Converting to `f64` rounds to the precision of `f64`.

Supported arithmetic operators are addition, subtraction, multiplication,
division, negation, and their assignment forms. Comparisons support equality
and partial ordering, so NaN values are unordered. The raw 80-bit
representation is not exposed.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.
