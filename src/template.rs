use crate::tx::{merkle_branch, sha256d, CoinbaseParts, TxOutput};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Deserialize)]
struct Gbt {
    version: i32,
    previousblockhash: String,
    height: u32,
    coinbasevalue: u64,
    bits: String,
    target: String,
    curtime: u32,
    mintime: u32,
    #[serde(default)]
    transactions: Vec<GbtTx>,
    #[serde(default)]
    default_witness_commitment: Option<String>,
}

#[derive(Deserialize)]
struct GbtTx {
    data: String,
    txid: String,
}

/// One block template of the node, shared by all connections.
pub struct Template {
    pub tip_hex: String,
    pub prev: [u8; 32],
    pub version: u32,
    pub bits: u32,
    pub target: [u8; 32],
    pub curtime: u32,
    pub mintime: u32,
    pub height: u32,
    pub coinbase_value: u64,
    pub witness_commitment: Option<Vec<u8>>,
    pub txs: Vec<Vec<u8>>,
    pub branch: Vec<[u8; 32]>,
    /// Coinbase outputs shared by all connections (pplns mode); `None` in solo mode.
    pub plan: Option<Vec<TxOutput>>,
    /// Payout address of each plan output ("pool" for the pool output), for the pages.
    pub plan_labels: Vec<String>,
    pub empty: bool,
    /// Job id shared by all connections for this template.
    pub seq: u64,
    tag: Vec<u8>,
    shared: OnceLock<Arc<JobData>>,
    by_script: Mutex<HashMap<Vec<u8>, Arc<JobData>>>,
}

/// Coinbase and pre-rendered mining.notify lines of a template for one payout script. In pplns mode one instance serves every connection, in solo mode one per address,
/// so a tip change costs each connection only a socket write.
pub struct JobData {
    pub parts: CoinbaseParts,
    notify: [Arc<[u8]>; 2],
}

impl JobData {
    /// The notify line; `clean` tells the miner to drop its old work.
    pub fn notify(&self, clean: bool) -> Arc<[u8]> {
        self.notify[clean as usize].clone()
    }
}

pub fn rev32(hexstr: &str) -> Result<[u8; 32]> {
    let mut b: [u8; 32] = hex::decode(hexstr)?.try_into().map_err(|_| anyhow!("expected 32 bytes: {hexstr}"))?;
    b.reverse();
    Ok(b)
}

/// Stratum prevhash: internal byte order with every 4-byte word reversed.
pub fn stratum_prevhash(prev: &[u8; 32]) -> String {
    let mut out = [0u8; 32];
    for (i, w) in prev.chunks(4).enumerate() {
        for (j, b) in w.iter().rev().enumerate() {
            out[i * 4 + j] = *b;
        }
    }
    hex::encode(out)
}

impl Template {
    /// Parses a getblocktemplate JSON-RPC response body.
    pub fn from_body(body: &[u8], seq: u64, tag: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        struct Resp {
            result: Option<Gbt>,
            error: Option<serde_json::Value>,
        }
        let r: Resp = serde_json::from_slice(body).context("unparsable getblocktemplate response")?;
        if let Some(e) = r.error.filter(|e| !e.is_null()) {
            anyhow::bail!("getblocktemplate: {e}");
        }
        let g = r.result.ok_or_else(|| anyhow!("getblocktemplate returned no result"))?;
        let txids = g.transactions.iter().map(|t| rev32(&t.txid)).collect::<Result<Vec<_>>>()?;
        let mut target = [0u8; 32];
        target.copy_from_slice(&hex::decode(&g.target)?);
        Ok(Self {
            prev: rev32(&g.previousblockhash)?,
            tip_hex: g.previousblockhash,
            version: g.version as u32,
            bits: u32::from_str_radix(&g.bits, 16).context("bad bits")?,
            target,
            curtime: g.curtime,
            mintime: g.mintime,
            height: g.height,
            coinbase_value: g.coinbasevalue,
            witness_commitment: g.default_witness_commitment.map(|h| hex::decode(h)).transpose()?,
            txs: g.transactions.iter().map(|t| hex::decode(&t.data)).collect::<Result<Vec<_>, _>>()?,
            branch: merkle_branch(&txids),
            plan: None,
            plan_labels: Vec::new(),
            empty: false,
            seq,
            tag: tag.to_vec(),
            shared: OnceLock::new(),
            by_script: Mutex::new(HashMap::new()),
        })
    }

    /// A block without transactions on top of `tip` (height `tip_height`), derived from the previous template; `None` where the difficulty could change (retarget block).
    pub fn empty_on_top_of(prev: &Template, tip_hex: &str, tip_height: u32, now: u32, halving: u32, retargets: bool, seq: u64) -> Option<Self> {
        let height = tip_height.checked_add(1)?;
        if tip_height == 0 || (retargets && (height % 2016 == 0 || height / 2016 != prev.height / 2016)) {
            return None;
        }
        let mut commitment = vec![0x6a, 0x24, 0xaa, 0x21, 0xa9, 0xed];
        commitment.extend_from_slice(&sha256d(&[0u8; 64]));
        Some(Self {
            tip_hex: tip_hex.to_string(),
            prev: rev32(tip_hex).ok()?,
            version: prev.version,
            bits: prev.bits,
            target: prev.target,
            curtime: now.max(prev.curtime),
            mintime: prev.mintime,
            height,
            coinbase_value: 5_000_000_000u64.checked_shr(height / halving).unwrap_or(0),
            witness_commitment: Some(commitment),
            txs: Vec::new(),
            branch: Vec::new(),
            plan: None,
            plan_labels: Vec::new(),
            empty: true,
            seq,
            tag: prev.tag.clone(),
            shared: OnceLock::new(),
            by_script: Mutex::new(HashMap::new()),
        })
    }

    pub fn job_id(&self) -> String {
        format!("{:x}", self.seq)
    }

    /// The job for a miner paying to `payout`, built on first use.
    pub fn job_for(&self, payout: &[u8]) -> Arc<JobData> {
        if self.plan.is_some() {
            return self.shared.get_or_init(|| Arc::new(self.build_job(payout))).clone();
        }
        let mut m = self.by_script.lock().unwrap();
        m.entry(payout.to_vec()).or_insert_with(|| Arc::new(self.build_job(payout))).clone()
    }

    fn build_job(&self, payout: &[u8]) -> JobData {
        let parts = CoinbaseParts::new(self.height, &self.tag, &self.coinbase_outputs(payout));
        let branch: Vec<String> = self.branch.iter().map(hex::encode).collect();
        let line = |clean: bool| -> Arc<[u8]> {
            let v = serde_json::json!({"id": null, "method": "mining.notify", "params": [
                self.job_id(), stratum_prevhash(&self.prev), hex::encode(&parts.coinb1), hex::encode(&parts.coinb2), branch,
                format!("{:08x}", self.version), format!("{:08x}", self.bits), format!("{:08x}", self.curtime), clean
            ]});
            let mut b = serde_json::to_vec(&v).unwrap();
            b.push(b'\n');
            b.into()
        };
        JobData { notify: [line(false), line(true)], parts }
    }

    /// Coinbase outputs for `payout`: the miner's own output in solo mode, the shared plan otherwise; then the witness commitment.
    pub fn coinbase_outputs(&self, payout: &[u8]) -> Vec<TxOutput> {
        let mut outs = match &self.plan {
            Some(p) => p.clone(),
            None => vec![TxOutput { value: self.coinbase_value, script: payout.to_vec() }],
        };
        if let Some(c) = &self.witness_commitment {
            outs.push(TxOutput { value: 0, script: c.clone() });
        }
        outs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(height: u32) -> Template {
        let body = serde_json::json!({"result": {
            "version": 0x2000_0000, "previousblockhash": "00".repeat(32), "height": height, "coinbasevalue": 1u64,
            "bits": "1d00ffff", "target": "00".repeat(32), "curtime": 1_700_000_000u32, "mintime": 1_699_990_000u32, "transactions": []
        }, "error": null});
        Template::from_body(&serde_json::to_vec(&body).unwrap(), 1, b"/t/").unwrap()
    }

    #[test]
    fn empty_template_has_the_subsidy_and_a_valid_single_tx_commitment() {
        let t = Template::empty_on_top_of(&base(100), &"11".repeat(32), 100, 1_700_000_100, 210_000, true, 2).unwrap();
        assert_eq!((t.height, t.coinbase_value, t.txs.len(), t.branch.len()), (101, 5_000_000_000, 0, 0));
        assert_eq!(t.curtime, 1_700_000_100);
        let c = t.witness_commitment.unwrap();
        assert_eq!(&c[..6], &[0x6a, 0x24, 0xaa, 0x21, 0xa9, 0xed]);
        assert_eq!(hex::encode(&c[6..]), "e2f61c3f71d1defd3fa999dfa36953755c690689799962b48bebd836974e8cf9");
    }

    #[test]
    fn subsidy_halves_and_retarget_blocks_are_skipped() {
        let before = Template::empty_on_top_of(&base(839_990), &"11".repeat(32), 839_998, 1_700_000_100, 210_000, true, 2).unwrap();
        assert_eq!((before.height, before.coinbase_value), (839_999, 625_000_000));
        let at_halving = Template::empty_on_top_of(&base(839_990), &"11".repeat(32), 839_999, 1, 210_000, true, 2).unwrap();
        assert_eq!((at_halving.height, at_halving.coinbase_value), (840_000, 312_500_000));
        assert!(Template::empty_on_top_of(&base(100), &"11".repeat(32), 2015, 1, 210_000, true, 2).is_none());
        assert!(Template::empty_on_top_of(&base(2000), &"11".repeat(32), 2100, 1, 210_000, true, 2).is_none());
        assert!(Template::empty_on_top_of(&base(2000), &"11".repeat(32), 2100, 1, 150, false, 2).is_some());
    }
}
