use crate::addr::script_for_address;
use crate::config::Mode;
use crate::diff::difficulty_from_target;
use crate::live::unix_now;
use crate::pool::Pool;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

const INDEX: &str = include_str!("../ui/index.html");

pub async fn serve(pool: Arc<Pool>) {
    let app = Router::new()
        .route("/", get(|| async { ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], INDEX) }))
        .route("/api/pool", get(pool_view))
        .route("/api/engine", get(engine_view))
        .route("/api/history", get(history))
        .route("/api/miner/:addr", get(miner_view))
        .with_state(pool.clone());
    match tokio::net::TcpListener::bind(&pool.cfg.http.listen).await {
        Ok(l) => {
            tracing::info!("web page and API on http://{}", pool.cfg.http.listen);
            if let Err(e) = axum::serve(l, app).await {
                tracing::error!("http server: {e}");
            }
        }
        Err(e) => tracing::error!("cannot bind {}: {e}", pool.cfg.http.listen),
    }
}

fn mask(a: &str) -> String {
    if a == "pool" || a.len() <= 16 {
        a.to_string()
    } else {
        format!("{}...{}", &a[..8], &a[a.len() - 6..])
    }
}

fn net_hashrate(target: &[u8; 32]) -> f64 {
    difficulty_from_target(target) * 4_294_967_296.0 / 600.0
}

async fn pool_view(State(p): State<Arc<Pool>>) -> Json<Value> {
    let tpl = p.template.borrow().clone();
    let st = &p.stats;
    let total_hr = p.live.pool.lock().unwrap().hashrate();
    let top: Vec<Value> = p
        .live
        .top(20)
        .into_iter()
        .map(|(a, hr, w, acc)| json!({"address": mask(&a), "hashrate": hr, "workers": w, "accepted": acc, "share_pct": if total_hr > 0.0 { hr / total_hr * 100.0 } else { 0.0 }}))
        .collect();
    let ledger = p.ledger.clone();
    let blocks = tokio::task::spawn_blocking(move || ledger.recent_blocks(15)).await.unwrap_or_default();
    let blocks: Vec<Value> = blocks.into_iter().map(|(t, h, hash, a, r, res)| json!({"time": t, "height": h, "hash": hash, "address": mask(&a), "reward": r, "result": res})).collect();
    let plan: Vec<Value> = tpl
        .as_ref()
        .and_then(|t| t.plan.as_ref().map(|pl| pl.iter().zip(&t.plan_labels).map(|(o, l)| json!({"address": mask(l), "sats": o.value, "pct": o.value as f64 / t.coinbase_value.max(1) as f64 * 100.0})).collect()))
        .unwrap_or_default();
    Json(json!({
        "network": p.cfg.pool.network, "mode": if p.cfg.pool.mode == Mode::Solo { "solo" } else { "pplns" }, "fee_percent": if p.cfg.pool.mode == Mode::Solo { 0.0 } else { p.cfg.pplns.fee_percent },
        "version": env!("CARGO_PKG_VERSION"), "uptime_s": p.started.elapsed().as_secs(),
        "height": tpl.as_ref().map(|t| t.height.saturating_sub(1)), "reward_sats": tpl.as_ref().map(|t| t.coinbase_value), "transactions": tpl.as_ref().map(|t| t.txs.len()),
        "net_difficulty": tpl.as_ref().map(|t| difficulty_from_target(&t.target)), "net_hashrate": tpl.as_ref().map(|t| net_hashrate(&t.target)),
        "hashrate": total_hr, "connections": st.conns.load(Relaxed), "workers": p.live.workers_online(),
        "accepted": st.accepted.load(Relaxed), "rejected": st.rejected.load(Relaxed), "stale": st.stale.load(Relaxed), "blocks_found": st.blocks.load(Relaxed),
        "top": top, "blocks": blocks, "plan": plan,
    }))
}

async fn engine_view(State(p): State<Arc<Pool>>) -> Json<Value> {
    let tpl = p.template.borrow().clone();
    let t = &p.timings;
    let events: Vec<Value> = p.live.events.lock().unwrap().iter().rev().take(50).map(|e| json!({"time": e.t, "level": e.level, "message": e.msg})).collect();
    Json(json!({
        "tip": tpl.as_ref().map(|t| t.tip_hex.clone()), "height": tpl.as_ref().map(|t| t.height.saturating_sub(1)),
        "template": tpl.as_ref().map(|t| json!({"kind": if t.empty { "empty" } else { "full" }, "transactions": t.txs.len(), "reward_sats": t.coinbase_value, "job_id": t.job_id(), "outputs": t.plan.as_ref().map(|p| p.len())})),
        "template_age_s": unix_now().saturating_sub(t.published_unix.load(Relaxed)),
        "timings_ms": {"node_rpc": t.rpc_ms.load(Relaxed), "build": t.build_ms.load(Relaxed), "wake_to_publish": t.publish_ms.load(Relaxed)},
        "templates": t.templates.load(Relaxed), "empty_jobs": t.empty_jobs.load(Relaxed),
        "connections": p.stats.conns.load(Relaxed), "accepted": p.stats.accepted.load(Relaxed), "rejected": p.stats.rejected.load(Relaxed), "stale": p.stats.stale.load(Relaxed),
        "extra_nodes": p.extra_nodes.len(), "fast_empty_job": p.cfg.pool.fast_empty_job && p.net.fast_empty_job_supported(),
        "events": events,
    }))
}

async fn history(State(p): State<Arc<Pool>>) -> Json<Value> {
    let h: Vec<Value> = p.live.history.lock().unwrap().iter().map(|x| json!({"t": x.t, "hashrate": x.hashrate, "connections": x.conns})).collect();
    Json(json!(h))
}

async fn miner_view(State(p): State<Arc<Pool>>, Path(addr): Path<String>) -> Response {
    let Ok(script) = script_for_address(&addr, p.net) else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid address for this network"}))).into_response();
    };
    let tpl = p.template.borrow().clone();
    let (mut workers, mut hashrate, mut accepted, mut rejected, mut last_share) = (Vec::new(), 0.0, 0u64, 0u64, 0u64);
    if let Some(a) = p.live.addrs.lock().unwrap().get(&addr) {
        hashrate = a.meter.hashrate();
        (accepted, rejected, last_share) = (a.accepted, a.rejected, a.last_share);
        for (name, w) in &a.workers {
            workers.push(json!({"name": name, "online": w.conns > 0, "hashrate": w.meter.hashrate(), "accepted": w.accepted, "rejected": w.rejected, "last_share": w.last_share, "agent": w.agent}));
        }
    }
    let payout = tpl.as_ref().and_then(|t| t.plan.as_ref().map(|pl| pl.iter().filter(|o| o.script == script).map(|o| o.value).sum::<u64>()));
    let window_pct = tpl.as_ref().map(|t| {
        let limit = (p.cfg.pplns.window_factor * difficulty_from_target(&t.target)).max(1e-9);
        let w = p.ledger.window(limit);
        let total: f64 = w.iter().map(|(_, d)| d).sum();
        let mine: f64 = w.iter().filter(|(a, _)| *a == addr).map(|(_, d)| d).sum();
        if total > 0.0 { mine / total * 100.0 } else { 0.0 }
    });
    Json(json!({
        "address": addr, "mode": if p.cfg.pool.mode == Mode::Solo { "solo" } else { "pplns" }, "known": !workers.is_empty(),
        "hashrate": hashrate, "accepted": accepted, "rejected": rejected, "last_share": last_share,
        "window_pct": window_pct, "payout_in_next_block_sats": payout, "workers": workers,
    }))
    .into_response()
}
