mod addr;
mod api;
mod config;
mod diff;
mod ledger;
mod live;
mod plan;
mod pool;
mod rpc;
mod stratum;
mod template;
mod tx;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "photond", version, about = "Headless Stratum V1 mining pool daemon for Bitcoin Core")]
struct Cli {
    #[arg(short, long, default_value = "photond.toml")]
    config: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the pool.
    Run,
    /// Validate the configuration and the node connection, then exit.
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    let cli = Cli::parse();
    let cfg = config::Config::load(&cli.config)?;
    let pool = pool::Pool::connect(cfg).await?;
    match cli.cmd {
        Cmd::Check => {
            let t = pool.build_template().await?;
            println!(
                "ok: chain {}, next block height {}, {} transactions, reward {} sats, mode {:?}",
                pool.cfg.pool.network, t.height, t.txs.len(), t.coinbase_value, pool.cfg.pool.mode
            );
            Ok(())
        }
        Cmd::Run => {
            tokio::spawn(pool.clone().run_template_loop());
            tokio::spawn(pool.clone().run_stats_loop());
            if !pool.cfg.http.listen.is_empty() {
                tokio::spawn(api::serve(pool.clone()));
            }
            tokio::select! {
                r = stratum::serve(pool.clone()) => r,
                _ = tokio::signal::ctrl_c() => { tracing::info!("shutting down"); pool.ledger.flush_now(); Ok(()) }
            }
        }
    }
}
