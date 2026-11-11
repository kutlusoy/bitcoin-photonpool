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
    /// Further nodes that every found block is submitted to as well (faster propagation, resilience).
    #[serde(default)]
    pub extra_nodes: Vec<ExtraNode>,
    pub pool: PoolCfg,
    #[serde(default)]
    pub difficulty: DiffCfg,
    #[serde(default)]
    pub pplns: PplnsCfg,
    #[serde(default)]
    pub ledger: LedgerCfg,
    #[serde(default)]
    pub http: HttpCfg,
}

#[derive(Deserialize, Clone)]
pub struct HttpCfg {
    /// Address of the web page and API; empty disables it.
    #[serde(default = "d_http")]
    pub listen: String,
    /// Name shown on the page.
    #[serde(default = "d_name")]
    pub name: String,
    /// Host shown in the stratum URL; empty uses the host of the page.
    #[serde(default)]
    pub stratum_host: String,
    /// Block explorer used for links, for example https://mempool.space; empty for none.
    #[serde(default)]
    pub explorer_url: String,
    /// Folder with audio files for the player and an optional playlist.json.
    #[serde(default = "d_music")]
    pub music_dir: String,
    /// Requests per minute and client for the data API.
    #[serde(default = "d_rate")]
    pub rate_per_min: u32,
    /// Take the client address from X-Forwarded-For (only behind a reverse proxy you control).
    #[serde(default)]
    pub trust_proxy: bool,
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
pub struct ExtraNode {
    pub rpc_url: String,
    pub auth: String,
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
    /// Weight the node leaves free for the coinbase (bitcoind `-blockreservedweight`, default 8000 since Bitcoin Core 30). Limits the number of coinbase outputs.
    #[serde(default = "d_reserved")]
    pub coinbase_reserved_weight: u64,
    /// On a new tip, hand out a valid block without transactions at once and follow with the full template (mainnet only).
    #[serde(default = "d_true")]
    pub fast_empty_job: bool,
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
fn d_reserved() -> u64 { 8000 }
fn d_true() -> bool { true }
fn d_start() -> f64 { 1024.0 }
fn d_min() -> f64 { 1.0 }
fn d_max() -> f64 { 1.0e12 }
fn d_share_secs() -> f64 { 10.0 }
fn d_window() -> f64 { 2.0 }
fn d_fee() -> f64 { 1.0 }
fn d_outputs() -> usize { 200 }
fn d_dust() -> u64 { 1000 }
fn d_name() -> String { "photon".into() }
fn d_music() -> String { "music".into() }
fn d_rate() -> u32 { 300 }
fn d_http() -> String { "127.0.0.1:8080".into() }
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
impl Default for HttpCfg {
    fn default() -> Self {
        Self { listen: d_http(), name: d_name(), stratum_host: String::new(), explorer_url: String::new(), music_dir: d_music(), rate_per_min: d_rate(), trust_proxy: false }
    }
}
impl Default for LedgerCfg {
    fn default() -> Self {
        Self { path: d_db() }
    }
}

impl PplnsCfg {
    /// Miner outputs that fit into the reserved coinbase weight, assuming the largest common script (34 bytes) per output.
    pub fn outputs_for_weight(&self, reserved: u64) -> usize {
        const BASE: u64 = 1000;
        const PER_OUTPUT: u64 = 4 * (8 + 1 + 34);
        self.max_outputs.min((reserved.saturating_sub(BASE) / PER_OUTPUT).max(1) as usize)
    }
}

impl Config {
    /// Block explorer for links: the configured URL, `off` for none, or mempool.space for the network by default.
    pub fn explorer(&self) -> String {
        let e = self.http.explorer_url.trim();
        match e {
            "off" => String::new(),
            "" => match self.network() {
                Ok(Network::Mainnet) => "https://mempool.space".into(),
                Ok(Network::Testnet) => "https://mempool.space/testnet4".into(),
                _ => String::new(),
            },
            _ => e.trim_end_matches('/').to_string(),
        }
    }

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
        if self.pool.coinbase_reserved_weight < 2000 {
            bail!("pool.coinbase_reserved_weight must be at least 2000");
        }
        if !(0.0..=100.0).contains(&self.pplns.fee_percent) || self.pplns.window_factor <= 0.0 || self.pplns.max_outputs == 0 {
            bail!("pplns: invalid fee_percent, window_factor or max_outputs");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_count_follows_reserved_weight() {
        let c = PplnsCfg { window_factor: 2.0, fee_percent: 1.0, max_outputs: 200, dust_sats: 1000 };
        assert_eq!(c.outputs_for_weight(8000), 40);
        assert_eq!(c.outputs_for_weight(100_000), 200);
        assert_eq!(c.outputs_for_weight(2000), 5);
        let one = PplnsCfg { max_outputs: 1, ..c };
        assert_eq!(one.outputs_for_weight(8000), 1);
    }
}
