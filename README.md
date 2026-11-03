# x87

`x87` provides an `f80` type for x86 and x86_64.

```rust
use x87::f80;

let sum = f80::from(1.25) + f80::from(2.5);
assert_eq!(sum.to_f64(), 3.75);
```

Arithmetic follows the active x87 floating-point environment, including its
rounding mode. Converting to `f64` rounds to the precision of `f64`.

## Testing

On Linux, `tests/ffi/run.sh` builds a C caller and checks layout, basic
arithmetic, and FMA results against glibc on x86_64 and i686. It requires the
i686 Rust target and 32-bit C toolchain support.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.
