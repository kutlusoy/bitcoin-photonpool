use crate::addr::{script_for_address, Network};
use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Every miner is paid directly by the coinbase of the blocks it finds.
    Solo,
    /// Rewards are split in the coinbase between the miners of the share window (non-custodial).
    Pplns,
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub node: NodeCfg,
    pub pool: PoolCfg,
    #[serde(default)]
    pub difficulty: DiffCfg,
    #[serde(default)]
    pub pplns: PplnsCfg,
    #[serde(default)]
    pub ledger: LedgerCfg,
}

#[derive(Deserialize, Clone)]
pub struct NodeCfg {
    pub rpc_url: String,
    /// "cookie:<path>", "env:<VAR>" holding user:password, or "user:password".
    pub auth: String,
    #[serde(default = "d_poll")]
    pub poll_ms: u64,
}

#[derive(Deserialize, Clone)]
pub struct PoolCfg {
    pub network: String,
    pub mode: Mode,
    /// Fee and fallback address of the pool; in pplns mode also receives the rounding remainder and sub-dust parts.
    pub address: String,
    #[serde(default = "d_listen")]
    pub stratum_listen: String,
    #[serde(default = "d_tag")]
    pub coinbase_tag: String,
    /// A new job for the same tip is sent at this interval so that new transactions and the current payout plan are mined.
    #[serde(default = "d_refresh")]
    pub refresh_secs: u64,
    #[serde(default = "d_max_conns")]
    pub max_connections: usize,
}

#[derive(Deserialize, Clone)]
pub struct DiffCfg {
    #[serde(default = "d_start")]
    pub start: f64,
    #[serde(default = "d_min")]
    pub min: f64,
    #[serde(default = "d_max")]
    pub max: f64,
    /// Vardiff aims at one share per this many seconds and connection.
    #[serde(default = "d_share_secs")]
    pub target_share_secs: f64,
}

#[derive(Deserialize, Clone)]
pub struct PplnsCfg {
    /// Window size in units of the network difficulty.
    #[serde(default = "d_window")]
    pub window_factor: f64,
    #[serde(default = "d_fee")]
    pub fee_percent: f64,
    #[serde(default = "d_outputs")]
    pub max_outputs: usize,
    #[serde(default = "d_dust")]
    pub dust_sats: u64,
}

#[derive(Deserialize, Clone)]
pub struct LedgerCfg {
    #[serde(default = "d_db")]
    pub path: String,
}

fn d_poll() -> u64 { 100 }
fn d_listen() -> String { "0.0.0.0:3333".into() }
fn d_tag() -> String { "/photonpool/".into() }
fn d_refresh() -> u64 { 30 }
fn d_max_conns() -> usize { 20_000 }
fn d_start() -> f64 { 1024.0 }
fn d_min() -> f64 { 1.0 }
fn d_max() -> f64 { 1.0e12 }
fn d_share_secs() -> f64 { 10.0 }
fn d_window() -> f64 { 2.0 }
fn d_fee() -> f64 { 1.0 }
fn d_outputs() -> usize { 200 }
fn d_dust() -> u64 { 1000 }
fn d_db() -> String { "photond.db".into() }

impl Default for DiffCfg {
    fn default() -> Self {
        Self { start: d_start(), min: d_min(), max: d_max(), target_share_secs: d_share_secs() }
    }
}
impl Default for PplnsCfg {
    fn default() -> Self {
        Self { window_factor: d_window(), fee_percent: d_fee(), max_outputs: d_outputs(), dust_sats: d_dust() }
    }
}
impl Default for LedgerCfg {
    fn default() -> Self {
        Self { path: d_db() }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn network(&self) -> Result<Network> {
        Network::parse(&self.pool.network)
    }

    fn validate(&self) -> Result<()> {
        let net = self.network()?;
        script_for_address(&self.pool.address, net).context("pool.address")?;
        if self.pool.coinbase_tag.len() > 20 {
            bail!("pool.coinbase_tag is longer than 20 bytes");
        }
        let d = &self.difficulty;
        if !(d.min > 0.0 && d.min <= d.start && d.start <= d.max) {
            bail!("difficulty: need 0 < min <= start <= max");
        }
        if !(0.0..=100.0).contains(&self.pplns.fee_percent) || self.pplns.window_factor <= 0.0 || self.pplns.max_outputs == 0 {
            bail!("pplns: invalid fee_percent, window_factor or max_outputs");
        }
        Ok(())
    }
}
