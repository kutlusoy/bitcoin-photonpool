use crate::tx::{merkle_branch, TxOutput};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

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
    pub fn from_gbt(v: serde_json::Value, plan: Option<Vec<TxOutput>>) -> Result<Self> {
        let g: Gbt = serde_json::from_value(v).context("unexpected getblocktemplate result")?;
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
            plan,
        })
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
