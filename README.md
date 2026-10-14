# x87

`x87` provides a small `F80` type backed by the x87 80-bit extended-precision
format. It currently supports conversion from `f64`, addition, and conversion
back to `f64` on x86 and x86_64.

```rust
use x87::F80;

let sum = F80::from(1.25) + F80::from(2.5);
assert_eq!(sum.to_f64(), 3.75);
```

Arithmetic follows the active x87 floating-point environment, including its
rounding mode. Converting to `f64` rounds to the precision of `f64`.

This crate currently provides only addition and conversion; it does not expose
the raw 80-bit representation or operations such as multiplication and
division.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.
