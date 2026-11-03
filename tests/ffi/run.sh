#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
manifest="$root/tests/ffi/Cargo.toml"
target_dir="$root/target/ffi-tests"

for target in x86_64-unknown-linux-gnu i686-unknown-linux-gnu; do
    CARGO_TARGET_DIR="$target_dir" cargo build --manifest-path "$manifest" --release --target "$target"
    library="$target_dir/$target/release/libx87_ffi_test.a"
    executable="$target_dir/$target/ffi_test"
    if [ "$target" = i686-unknown-linux-gnu ]; then
        cc -m32 -std=c11 -O2 -fno-fast-math -ffp-contract=off -frounding-math \
            "$root/tests/ffi/ffi_test.c" "$library" -ldl -lpthread -lm -lrt -lutil -o "$executable"
    else
        cc -std=c11 -O2 -fno-fast-math -ffp-contract=off -frounding-math \
            "$root/tests/ffi/ffi_test.c" "$library" -ldl -lpthread -lm -lrt -lutil -o "$executable"
    fi
    "$executable"
done
