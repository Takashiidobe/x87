#include <math.h>
#include <float.h>
#include <fenv.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__x86_64__)
typedef struct __attribute__((aligned(16))) {
    unsigned char bytes[10];
} rust_f80;
_Static_assert(sizeof(rust_f80) == 16, "x86_64 f80 size");
_Static_assert(_Alignof(rust_f80) == 16, "x86_64 f80 alignment");
#elif defined(__i386__)
typedef struct __attribute__((aligned(4))) {
    unsigned char bytes[10];
} rust_f80;
_Static_assert(sizeof(rust_f80) == 12, "i686 f80 size");
_Static_assert(_Alignof(rust_f80) == 4, "i686 f80 alignment");
#else
#error unsupported test architecture
#endif

extern size_t rust_f80_size(void);
extern size_t rust_f80_align(void);
extern rust_f80 rust_add(rust_f80, rust_f80);
extern rust_f80 rust_sub(rust_f80, rust_f80);
extern rust_f80 rust_mul(rust_f80, rust_f80);
extern rust_f80 rust_div(rust_f80, rust_f80);
extern rust_f80 rust_mul_add(rust_f80, rust_f80, rust_f80);
extern rust_f80 rust_tanh(rust_f80);
extern rust_f80 rust_gamma(rust_f80);
extern float rust_to_f32(rust_f80);

static rust_f80 from_long_double(long double value) {
    rust_f80 result = {0};
    memcpy(&result, &value, 10);
    return result;
}

static long double to_long_double(rust_f80 value) {
    long double result = 0.0L;
    memcpy(&result, &value, 10);
    return result;
}

static uint64_t random_u64(void) {
    uint64_t value = 0;
    for (unsigned i = 0; i < 4; ++i)
        value = (value << 16) ^ (uint64_t)(rand() & 0xffff);
    return value;
}

static long double random_finite_long_double(void) {
    long double value = 0.0L;
    uint64_t significand = random_u64();
    uint16_t exponent = (uint16_t)(rand() % 0x7fff);
    if (exponent == 0)
        significand &= 0x7fffffffffffffffULL;
    else
        significand |= 0x8000000000000000ULL;
    if (rand() & 1)
        exponent |= 0x8000;
    memcpy(&value, &significand, sizeof(significand));
    memcpy((unsigned char *)&value + 8, &exponent, sizeof(exponent));
    return value;
}

static void check_equal(const char *operation, long double expected, long double actual) {
    if ((expected != actual && !(isnan(expected) && isnan(actual))) ||
        (expected == 0.0L && actual == 0.0L && signbit(expected) != signbit(actual))) {
        fprintf(stderr, "%s mismatch: expected=%La actual=%La\n", operation, expected, actual);
        abort();
    }
}

static void check_case_equal(const char *test_case, long double expected, long double actual) {
    if ((expected != actual && !(isnan(expected) && isnan(actual))) ||
        (expected == 0.0L && actual == 0.0L && signbit(expected) != signbit(actual))) {
        fprintf(stderr, "%s: expected=%La actual=%La\n", test_case, expected, actual);
        abort();
    }
}

int main(void) {
    if (rust_f80_size() != sizeof(long double) || rust_f80_align() != _Alignof(long double)) {
        fprintf(stderr, "layout mismatch: Rust %zu/%zu, C long double %zu/%zu\n",
                rust_f80_size(), rust_f80_align(), sizeof(long double), _Alignof(long double));
        return 1;
    }

    const long double samples[][2] = {
        {1.25L, 2.5L}, {-3.0L, 7.0L}, {0x1.000000000000001p+40L, 0x1.8p-3L},
        {LDBL_MIN, 2.0L}, {LDBL_MAX / 4.0L, 2.0L}, {-0.0L, 3.0L}
    };
    for (size_t i = 0; i < sizeof(samples) / sizeof(samples[0]); ++i) {
        long double a = samples[i][0], b = samples[i][1];
        rust_f80 ra = from_long_double(a), rb = from_long_double(b);
        check_equal("add", a + b, to_long_double(rust_add(ra, rb)));
        check_equal("sub", a - b, to_long_double(rust_sub(ra, rb)));
        check_equal("mul", a * b, to_long_double(rust_mul(ra, rb)));
        if (b != 0.0L)
            check_equal("div", a / b, to_long_double(rust_div(ra, rb)));
    }

    check_case_equal("tanh(+10000)", tanhl(10000.0L),
                     to_long_double(rust_tanh(from_long_double(10000.0L))));
    check_case_equal("tanh(-10000)", tanhl(-10000.0L),
                     to_long_double(rust_tanh(from_long_double(-10000.0L))));
    check_case_equal("tanh(+infinity)", tanhl(INFINITY),
                     to_long_double(rust_tanh(from_long_double(INFINITY))));
    check_case_equal("tanh(-infinity)", tanhl(-INFINITY),
                     to_long_double(rust_tanh(from_long_double(-INFINITY))));

    check_case_equal("gamma(-infinity)", tgammal(-INFINITY),
                     to_long_double(rust_gamma(from_long_double(-INFINITY))));

    volatile long double midpoint = 1.0L + ldexpl(1.0L, -24);
    volatile long double above_midpoint = midpoint + ldexpl(1.0L, -63);
    long double conversion_input = above_midpoint;
    float expected_f32 = (float)conversion_input;
    float actual_f32 = rust_to_f32(from_long_double(conversion_input));
    if (memcmp(&expected_f32, &actual_f32, sizeof(expected_f32)) != 0) {
        fprintf(stderr, "f80_to_f32(1 + 2^-24 + 2^-63): expected=%a actual=%a input=%La\n",
                expected_f32, actual_f32, conversion_input);
        abort();
    }

    const int rounding_modes[] = {FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO};
    long double min_subnormal = nextafterl(0.0L, 1.0L);
    for (size_t mode = 0; mode < sizeof(rounding_modes) / sizeof(rounding_modes[0]); ++mode) {
        if (fesetround(rounding_modes[mode]) != 0)
            return 2;
        long double fma_inputs[][3] = {
            {1.0L + ldexpl(1.0L, -32), 1.0L - ldexpl(1.0L, -32), -1.0L},
            {min_subnormal, 0.5L, 0.0L},
            {-min_subnormal, 0.5L, 0.0L},
            {LDBL_MAX, 2.0L, -LDBL_MAX},
            {INFINITY, 0.0L, 1.0L},
            {INFINITY, 2.0L, -INFINITY}
        };
        for (size_t i = 0; i < sizeof(fma_inputs) / sizeof(fma_inputs[0]); ++i) {
            long double a = fma_inputs[i][0], b = fma_inputs[i][1], c = fma_inputs[i][2];
            rust_f80 result = rust_mul_add(from_long_double(a), from_long_double(b), from_long_double(c));
            check_equal("fma edge", fmal(a, b, c), to_long_double(result));
        }
        for (unsigned i = 0; i < 10000; ++i) {
            long double a = random_finite_long_double();
            long double b = random_finite_long_double();
            long double c = random_finite_long_double();
            rust_f80 result = rust_mul_add(from_long_double(a), from_long_double(b), from_long_double(c));
            check_equal("fma", fmal(a, b, c), to_long_double(result));
        }
    }

    puts("C/Rust f80 ABI, arithmetic, and FMA checks passed");
    return 0;
}
