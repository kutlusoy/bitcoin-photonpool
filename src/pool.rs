use crate::addr::{script_for_address, Network};
use crate::config::{Config, Mode};
use crate::diff::difficulty_from_target;
use crate::ledger::Ledger;
use crate::plan::build_plan;
use crate::rpc::NodeRpc;
use crate::template::Template;
use anyhow::{bail, Context, Result};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[derive(Default)]
pub struct Stats {
    pub conns: AtomicU64,
    pub accepted: AtomicU64,
    pub rejected: AtomicU64,
    pub stale: AtomicU64,
    pub blocks: AtomicU64,
    pub accepted_diff: Mutex<f64>,
}

pub struct Pool {
    pub cfg: Config,
    pub net: Network,
    pub node: NodeRpc,
    pub ledger: Ledger,
    pub pool_script: Vec<u8>,
    pub stats: Stats,
    pub template: watch::Sender<Option<Arc<Template>>>,
    en1_seq: AtomicU32,
    job_seq: AtomicU64,
}

impl Pool {
    pub async fn connect(cfg: Config) -> Result<Arc<Pool>> {
        let net = cfg.network()?;
        let node = NodeRpc::new(&cfg.node.rpc_url, &cfg.node.auth)?;
        let (chain, height, syncing) = node.chain_info().await.context("cannot reach the node")?;
        if !net.chain().contains(&chain.as_str()) {
            bail!("node is on chain {chain}, config says {}", cfg.pool.network);
        }
        if syncing {
            bail!("node is still syncing (height {height})");
        }
        let pool_script = script_for_address(&cfg.pool.address, net)?;
        let ledger = Ledger::open(&cfg.ledger.path).with_context(|| format!("opening ledger {}", cfg.ledger.path))?;
        let (template, _) = watch::channel(None);
        Ok(Arc::new(Pool { cfg, net, node, ledger, pool_script, stats: Stats::default(), template, en1_seq: AtomicU32::new(rand_start()), job_seq: AtomicU64::new(0) }))
    }

    pub fn next_en1(&self) -> [u8; 4] {
        self.en1_seq.fetch_add(1, Ordering::Relaxed).to_be_bytes()
    }

    pub fn next_job_id(&self) -> String {
        format!("{:x}", self.job_seq.fetch_add(1, Ordering::Relaxed) + 1)
    }

    pub async fn build_template(&self) -> Result<Template> {
        let raw = self.node.template().await?;
        if self.cfg.pool.mode == Mode::Solo {
            return Template::from_gbt(raw, None);
        }
        let probe = Template::from_gbt(raw.clone(), None)?;
        let limit = (self.cfg.pplns.window_factor * difficulty_from_target(&probe.target)).max(1e-9);
        let ledger = self.ledger.clone();
        let window = tokio::task::spawn_blocking(move || ledger.window(limit)).await??;
        let window: Vec<(Vec<u8>, f64)> = window.into_iter().filter_map(|(a, w)| script_for_address(&a, self.net).ok().map(|s| (s, w))).collect();
        let plan = build_plan(probe.coinbase_value, &window, &self.pool_script, &self.cfg.pplns);
        Template::from_gbt(raw, Some(plan))
    }

    /// Publishes a new template when the tip changes and every `refresh_secs`.
    pub async fn run_template_loop(self: Arc<Self>) {
        let poll = Duration::from_millis(self.cfg.node.poll_ms);
        let refresh = Duration::from_secs(self.cfg.pool.refresh_secs);
        let (mut last_tip, mut last_build, mut down) = (String::new(), Instant::now(), false);
        let mut first = true;
        loop {
            if !first {
                tokio::time::sleep(poll).await;
            }
            let best = match self.node.best_block_hash().await {
                Ok(b) => b,
                Err(e) => {
                    if !down {
                        tracing::error!("node link lost: {e}");
                    }
                    down = true;
                    continue;
                }
            };
            if down {
                tracing::info!("node link restored");
                down = false;
            }
            if !first && best == last_tip && last_build.elapsed() < refresh {
                continue;
            }
            first = false;
            match self.build_template().await {
                Ok(t) => {
                    if t.tip_hex != last_tip {
                        tracing::info!("new tip {} at height {}, {} transactions, reward {} sats", t.tip_hex, t.height - 1, t.txs.len(), t.coinbase_value);
                    }
                    last_tip = t.tip_hex.clone();
                    last_build = Instant::now();
                    self.template.send_replace(Some(Arc::new(t)));
                }
                Err(e) => {
                    tracing::warn!("template: {e:#}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    pub async fn run_stats_loop(self: Arc<Self>) {
        let mut last = (0u64, 0u64, 0u64, Instant::now(), 0.0f64);
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let (acc, rej, stale) = (self.stats.accepted.load(Ordering::Relaxed), self.stats.rejected.load(Ordering::Relaxed), self.stats.stale.load(Ordering::Relaxed));
            let diff = *self.stats.accepted_diff.lock().unwrap();
            let secs = last.3.elapsed().as_secs_f64();
            let hs = (diff - last.4) * 4_294_967_296.0 / secs;
            tracing::info!(
                "connections {} | shares +{} (rejected +{}, stale +{}) | hashrate ~{} | blocks {}",
                self.stats.conns.load(Ordering::Relaxed), acc - last.0, rej - last.1, stale - last.2, fmt_hashrate(hs), self.stats.blocks.load(Ordering::Relaxed)
            );
            last = (acc, rej, stale, Instant::now(), diff);
        }
    }
}

fn rand_start() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1)
}

fn fmt_hashrate(h: f64) -> String {
    let units = ["H/s", "kH/s", "MH/s", "GH/s", "TH/s", "PH/s", "EH/s"];
    let (mut v, mut i) = (h, 0);
    while v >= 1000.0 && i < units.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{v:.2} {}", units[i])
}
