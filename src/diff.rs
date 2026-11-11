/// Share target for a pool difficulty (difficulty 1 = 0x00000000ffff << 208), big-endian.
pub fn target_from_difficulty(d: f64) -> [u8; 32] {
    let d = if d.is_finite() { d.clamp(1e-9, 1e15) } else { 1.0 };
    let divisor = ((d * 1_048_576.0).round() as u128).max(1);
    // numerator = 0xffff << 228 (difficulty-1 target shifted by 20 bits for fractional difficulties)
    let mut num = [0u8; 32];
    num[1] = 0x0f;
    num[2] = 0xff;
    num[3] = 0xf0;
    let mut out = [0u8; 32];
    let mut rem: u128 = 0;
    for i in 0..32 {
        rem = (rem << 8) | num[i] as u128;
        out[i] = (rem / divisor) as u8;
        rem %= divisor;
    }
    out
}

/// Network difficulty implied by a big-endian target (difficulty 1 = 0x00000000ffff << 208).
pub fn difficulty_from_target(target: &[u8; 32]) -> f64 {
    let t = target.iter().fold(0.0f64, |acc, b| acc * 256.0 + *b as f64);
    if t <= 0.0 {
        return f64::INFINITY;
    }
    65535.0 * 2f64.powi(208) / t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_one_is_the_standard_target() {
        let t = target_from_difficulty(1.0);
        let mut expect = [0u8; 32];
        expect[4] = 0xff;
        expect[5] = 0xff;
        assert_eq!(t, expect);
    }

    #[test]
    fn higher_difficulty_means_smaller_target() {
        assert!(target_from_difficulty(2.0) < target_from_difficulty(1.0));
        assert!(target_from_difficulty(0.5) > target_from_difficulty(1.0));
        let t = target_from_difficulty(65536.0);
        let mut expect = [0u8; 32];
        expect[6] = 0xff;
        expect[7] = 0xff;
        assert_eq!(t, expect);
    }

    #[test]
    fn difficulty_roundtrip() {
        for d in [1.0, 16.0, 1234.5, 1.0e6] {
            let back = difficulty_from_target(&target_from_difficulty(d));
            assert!((back / d - 1.0).abs() < 1e-3, "{d} -> {back}");
        }
    }

    #[test]
    fn extreme_values_do_not_panic() {
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-12, 1e18] {
            let _ = target_from_difficulty(d);
        }
    }
}
