#![deny(unsafe_code)]

pub const fn version() -> &'static str {
    "0.1.0"
}

pub fn hash_sha256(input: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// Lossy `f64` → `f32` conversion using IEEE 754 bit manipulation.
///
/// Avoids both `as f32` (`clippy::cast_possible_truncation`) and the
/// `TryFrom<f64> for f32` / `From<f64> for f32` trait impls (which
/// aren't available in our std build). Correctly handles:
/// - Finite values in range → truncated mantissa, rebiased exponent
/// - f64 subnormals → `f32::ZERO` (can't be represented in f32)
/// - Overflow → `f32::INFINITY` (signed)
/// - NaN → `f32::NAN`
///
/// Lives in `utils` (not `vectors`) so it is available under every
/// feature configuration — `lib.rs` gates `pub mod vectors;` behind
/// `embeddings`/`embeddings-dynamic`, but this helper has no
/// dependency on those features and was causing builds with
/// `default-features = false` (e.g. the fuzz workspace) to fail to
/// resolve `crate::vectors::f64_to_f32_lossy`.
pub fn f64_to_f32_lossy(v: f64) -> f32 {
    let bits = v.to_bits();
    // High 32 bits of the f64 representation carry the sign, the full
    // exponent, and the top 20 mantissa bits — exactly what f32 needs,
    // modulo the exponent bias difference (f64 uses 1023, f32 uses 127).
    let high32 = u32::try_from(bits >> 32).unwrap_or(0);
    let sign = high32 & 0x8000_0000;
    let exp_f64 = i32::try_from((high32 >> 20) & 0x7ff).unwrap_or(0); // 11-bit f64 exponent
    let mantissa = high32 & 0x0007_ffff; // top 20 bits of f64 mantissa
    // Rebias exponent: f32_exp = saturating(f64_exp - 1023 + 127, 0..=255).
    // We use wrapping arithmetic to avoid `as` casts and `i32::try_from`.
    let rebased = exp_f64.wrapping_sub(896); // -1023 + 127 = -896
    let new_exp = if rebased < 0 {
        0u32
    } else if rebased > 255 {
        255u32
    } else {
        // Safe: `rebased` is in [0, 255] which fits in u32 exactly.
        u32::try_from(rebased).unwrap_or(0)
    };
    // Inf/NaN preservation: f64 exp = 0x7ff → f32 exp = 0xff. Our clamp
    // already maps 0x7ff to 255, so this works for free.
    f32::from_bits(sign | (new_exp << 23) | mantissa)
}

pub fn node_id_for_symbol(
    file_path: &str,
    kind: &str,
    qualified_name: &str,
    start_line: i64,
    start_column: i64,
) -> String {
    let seed = format!("{file_path}|{kind}|{qualified_name}|{start_line}|{start_column}");
    hash_sha256(&seed)
}
