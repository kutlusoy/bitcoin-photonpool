use crate::config::PplnsCfg;
use crate::tx::TxOutput;

/// Splits `reward` between the miners of the share window. Returns miner outputs in descending weight order followed by one pool output with the fee,
/// the rounding remainder and every part below the dust limit or beyond `max_outputs`; the values always add up to `reward`.
pub fn build_plan(reward: u64, window: &[(Vec<u8>, f64)], pool_script: &[u8], cfg: &PplnsCfg) -> Vec<TxOutput> {
    let fee = (reward as f64 * cfg.fee_percent / 100.0) as u64;
    let distributable = reward.saturating_sub(fee.min(reward));
    let max = window.iter().map(|(_, w)| *w).filter(|w| w.is_finite()).fold(0.0f64, f64::max);
    let mut ranked: Vec<(&Vec<u8>, u128)> = if max > 0.0 {
        window.iter().filter(|(_, w)| w.is_finite() && *w > 0.0).map(|(s, w)| (s, (w / max * 1e12) as u128)).filter(|(_, w)| *w > 0).collect()
    } else {
        Vec::new()
    };
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let total: u128 = ranked.iter().map(|(_, w)| *w).sum();

    let mut outs: Vec<TxOutput> = Vec::new();
    let mut paid = 0u64;
    if total > 0 {
        for (script, w) in ranked.into_iter().take(cfg.max_outputs) {
            let v = (distributable as u128 * w / total) as u64;
            if v >= cfg.dust_sats.max(1) {
                paid += v;
                outs.push(TxOutput { value: v, script: script.clone() });
            }
        }
    }
    let rest = reward - paid;
    if rest > 0 {
        outs.push(TxOutput { value: rest, script: pool_script.to_vec() });
    }
    outs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PplnsCfg {
        PplnsCfg { window_factor: 2.0, fee_percent: 1.0, max_outputs: 3, dust_sats: 1000 }
    }

    #[test]
    fn values_add_up_and_follow_weights() {
        let w = vec![(vec![1], 300.0), (vec![2], 100.0), (vec![3], 600.0)];
        let outs = build_plan(625_000_000, &w, &[9], &cfg());
        assert_eq!(outs.iter().map(|o| o.value).sum::<u64>(), 625_000_000);
        assert_eq!(outs[0].script, vec![3]);
        assert_eq!(outs[0].value, 371_250_000);
        assert_eq!(outs.last().unwrap().script, vec![9]);
        assert!(outs.last().unwrap().value >= 6_250_000);
    }

    #[test]
    fn empty_window_pays_the_pool() {
        let outs = build_plan(100, &[], &[9], &cfg());
        assert_eq!(outs, vec![TxOutput { value: 100, script: vec![9] }]);
    }

    #[test]
    fn dust_and_output_limit_go_to_the_pool() {
        let mut w: Vec<(Vec<u8>, f64)> = (0..10u8).map(|i| (vec![i], 100.0)).collect();
        w.push((vec![99], 1e-9));
        let outs = build_plan(10_000_000, &w, &[200], &cfg());
        assert_eq!(outs.len(), 4);
        assert_eq!(outs.iter().map(|o| o.value).sum::<u64>(), 10_000_000);
        assert!(outs.iter().all(|o| o.script != vec![99]));
    }

    #[test]
    fn tiny_weights_are_scaled_relative_to_the_largest() {
        let w = vec![(vec![1], 3e-9), (vec![2], 1e-9)];
        let outs = build_plan(1_000_000, &w, &[9], &cfg());
        assert_eq!(outs[0].script, vec![1]);
        assert!(outs[0].value.abs_diff(742_500) <= 1 && outs[1].value.abs_diff(247_500) <= 1);
        assert_eq!(outs.iter().map(|o| o.value).sum::<u64>(), 1_000_000);
    }

    #[test]
    fn zero_fee_and_single_miner() {
        let c = PplnsCfg { fee_percent: 0.0, ..cfg() };
        let outs = build_plan(5000, &[(vec![1], 7.0)], &[9], &c);
        assert_eq!(outs, vec![TxOutput { value: 5000, script: vec![1] }]);
    }
}
