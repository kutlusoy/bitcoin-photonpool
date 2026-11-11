use crate::config::Mode;
use crate::diff::difficulty_from_target;
use crate::live::unix_now;
use crate::pool::Pool;
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; media-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const MATURITY: i64 = 100;

macro_rules! asset {
    ($path:expr, $ctype:expr, $file:expr) => {
        ($path, $ctype, include_bytes!(concat!("../ui/", $file)) as &'static [u8])
    };
}

const JS: &str = "text/javascript; charset=utf-8";
const ASSETS: &[(&str, &str, &[u8])] = &[
    asset!("/style.css", "text/css; charset=utf-8", "style.css"),
    asset!("/common.js", JS, "common.js"),
    asset!("/spa.js", JS, "spa.js"),
    asset!("/charts.js", JS, "charts.js"),
    asset!("/stats.js", JS, "stats.js"),
    asset!("/music.js", JS, "music.js"),
    asset!("/depth.js", JS, "depth.js"),
    asset!("/public.js", JS, "public.js"),
    asset!("/flow.js", JS, "flow.js"),
    asset!("/engine.js", JS, "engine.js"),
    asset!("/miner.js", JS, "miner.js"),
    asset!("/scene3d.js", JS, "scene3d.js"),
    asset!("/vendor/three.min.js", JS, "vendor/three.min.js"),
    asset!("/favicon.svg", "image/svg+xml", "favicon.svg"),
    asset!("/llms.txt", "text/plain; charset=utf-8", "llms.txt"),
    asset!("/robots.txt", "text/plain; charset=utf-8", "robots.txt"),
];
const INDEX: &[u8] = include_bytes!("../ui/index.html");

/// Token bucket per client address.
pub struct Limiter {
    buckets: Mutex<HashMap<IpAddr, (f64, Instant)>>,
    capacity: f64,
    per_sec: f64,
}

impl Limiter {
    pub fn new(per_min: u32) -> Self {
        let capacity = per_min.max(1) as f64;
        Self { buckets: Mutex::new(HashMap::new()), capacity, per_sec: capacity / 60.0 }
    }

    pub fn allow(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut m = self.buckets.lock().unwrap();
        if m.len() > 50_000 {
            m.retain(|_, (_, t)| now.duration_since(*t) < Duration::from_secs(600));
        }
        let e = m.entry(ip).or_insert((self.capacity, now));
        e.0 = (e.0 + now.duration_since(e.1).as_secs_f64() * self.per_sec).min(self.capacity);
        e.1 = now;
        if e.0 >= 1.0 {
            e.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

pub fn client_ip(peer: IpAddr, headers: &HeaderMap, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(ip) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).and_then(|v| v.trim().parse().ok()) {
            return ip;
        }
    }
    peer
}

pub fn mask(a: &str) -> String {
    if a.len() > 14 {
        format!("{}..{}", &a[..6], &a[a.len() - 4..])
    } else {
        a.to_string()
    }
}

#[derive(Clone)]
struct St {
    pool: Arc<Pool>,
    limiter: Arc<Limiter>,
}

#[derive(Deserialize)]
struct Limit {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Deserialize)]
struct RangeQ {
    range: Option<String>,
}

#[derive(Deserialize)]
struct SinceQ {
    since: Option<u64>,
}

#[derive(Deserialize)]
struct PageQ {
    offset: Option<i64>,
    limit: Option<i64>,
}

fn page(body: &'static [u8], ctype: &'static str) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, ctype)], body)
}

pub fn router(pool: Arc<Pool>) -> Router {
    let st = St { limiter: Arc::new(Limiter::new(pool.cfg.http.rate_per_min)), pool: pool.clone() };
    let mut app = Router::new()
        .route("/public/v1/pool", get(pool_view))
        .route("/public/v1/blocks", get(blocks))
        .route("/public/v1/payouts", get(|| async { Json(json!([])) }))
        .route("/public/v1/devices", get(devices))
        .route("/public/v1/history/devices", get(device_history))
        .route("/public/v1/history", get(history))
        .route("/public/v1/account/:address", get(account))
        .route("/public/v1/account/:address/blocks", get(|State(s): State<St>, Path(a): Path<String>, Query(q): Query<PageQ>| async move { account_page(s, a, "blocks", q).await }))
        .route("/public/v1/account/:address/credits", get(|State(s): State<St>, Path(a): Path<String>, Query(q): Query<PageQ>| async move { account_page(s, a, "credits", q).await }))
        .route("/public/v1/account/:address/payouts", get(|State(s): State<St>, Path(a): Path<String>, Query(q): Query<PageQ>| async move { account_page(s, a, "payouts", q).await }))
        .route("/public/v1/engine", get(engine))
        .route("/public/v1/events", get(events));
    for path in ["/", "/engine", "/ops", "/miner", "/miner.html", "/miner/*address"] {
        app = app.route(path, get(|| async { page(INDEX, "text/html; charset=utf-8") }));
    }
    for (path, ctype, body) in ASSETS {
        let (ctype, body) = (*ctype, *body);
        app = app.route(path, get(move || async move { page(body, ctype) }));
    }
    let music = crate::music::dir(&pool.cfg.http.music_dir, "ui");
    app.route("/music/playlist.json", get({
        let m = music.clone();
        move || crate::music::playlist_response(m.clone())
    }))
    .nest_service("/music", crate::music::files(music))
    .layer(middleware::from_fn_with_state(st.clone(), guard))
    .with_state(st)
}

pub async fn serve(pool: Arc<Pool>) {
    let listener = match tokio::net::TcpListener::bind(&pool.cfg.http.listen).await {
        Ok(l) => l,
        Err(e) => return tracing::error!("cannot bind {}: {e}", pool.cfg.http.listen),
    };
    tracing::info!("web page and API on http://{}", pool.cfg.http.listen);
    if let Err(e) = axum::serve(listener, router(pool).into_make_service_with_connect_info::<SocketAddr>()).await {
        tracing::error!("http server: {e}");
    }
}

async fn guard(State(st): State<St>, ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    let ip = client_ip(peer.ip(), req.headers(), st.pool.cfg.http.trust_proxy);
    // only the data API is limited: a page view loads about ten static files and the music streams in ranges
    let limited = req.uri().path().starts_with("/public/v1/");
    let mut resp = if !limited || st.limiter.allow(ip) {
        next.run(req).await
    } else {
        let mut r = (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
        r.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
        r
    };
    let h = resp.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("public, max-age=5"));
    resp
}

fn mode_name(p: &Pool) -> &'static str {
    if p.cfg.pool.mode == Mode::Solo { "solo" } else { "pplns" }
}

fn fee(p: &Pool) -> f64 {
    if p.cfg.pool.mode == Mode::Solo { 0.0 } else { p.cfg.pplns.fee_percent }
}

fn url_of(base: &str, hash: &str) -> Option<String> {
    (!base.is_empty() && hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())).then(|| format!("{}/block/{hash}", base.trim_end_matches('/')))
}

fn port_of(listen: &str) -> u64 {
    listen.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0)
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() as f64 * q) as usize).min(sorted.len() - 1)]
}

fn hashrate_obj(p: &Pool) -> Value {
    let hist: Vec<f64> = p.live.hist.lock().unwrap().iter().copied().collect();
    json!({"now": p.live.pool.lock().unwrap().hashrate(), "m1": p.live.m1.lock().unwrap().hashrate(), "h1": p.live.h1.lock().unwrap().hashrate(), "hist": if hist.is_empty() { vec![0.0] } else { hist }})
}

async fn pool_view(State(st): State<St>) -> Json<Value> {
    let p = &st.pool;
    let tpl = p.template.borrow().clone();
    let (height, tip, net_diff) = tpl.as_ref().map(|t| (t.height.saturating_sub(1), t.tip_hex.clone(), difficulty_from_target(&t.target))).unwrap_or((0, String::new(), 0.0));
    let ledger = p.ledger.clone();
    let day_ago = unix_now() as i64 - 86_400;
    let (found24h, by_status, total) = tokio::task::spawn_blocking(move || (ledger.blocks_since(day_ago), ledger.blocks_by_status(), ledger.block_count())).await.unwrap_or_default();
    let c = &p.cfg.http;
    Json(json!({
        "name": c.name, "version": env!("CARGO_PKG_VERSION"), "release": env!("CARGO_PKG_VERSION"), "network": p.cfg.pool.network, "explorerUrl": c.explorer_url,
        "uptime": p.started.elapsed().as_secs_f64(),
        "stratum": {"host": c.stratum_host, "ports": [{"port": port_of(&p.cfg.pool.stratum_listen), "mode": mode_name(p), "override": false}]},
        "blocksTotal": total, "payoutsTotal": 0,
        "payout": {"dryRun": false, "wallet": false, "intervalSecs": 0, "nextRunAt": 0, "lastRunAt": 0},
        "modes": [{"mode": mode_name(p), "fee": fee(p), "available": true}], "minPayoutLeptons": 0, "pplnsWindowFactor": p.cfg.pplns.window_factor,
        "hashrate": hashrate_obj(p), "miners": {"online": p.live.workers_online(), "workers": p.stats.conns.load(Relaxed)},
        "found24h": found24h, "blocks": by_status.into_iter().collect::<HashMap<_, _>>(),
        "node": {"height": height, "tip": tip, "tipTime": p.timings.tip_unix.load(Relaxed)}, "networkDifficulty": net_diff,
        "connected": p.timings.node_up.load(Relaxed),
    }))
}

async fn blocks(State(st): State<St>, Query(q): Query<Limit>) -> Json<Value> {
    let p = &st.pool;
    let (n, off) = (q.limit.unwrap_or(12).min(50) as i64, q.offset.unwrap_or(0).min(10_000_000) as i64);
    let ledger = p.ledger.clone();
    let rows = tokio::task::spawn_blocking(move || ledger.blocks_page(off, n)).await.unwrap_or_default();
    let base = &p.cfg.http.explorer_url;
    Json(Value::Array(rows.into_iter().map(|b| block_json(base, &b, true)).collect()))
}

fn block_json(base: &str, b: &crate::ledger::BlockRow, with_finder: bool) -> Value {
    let mut v = json!({"height": b.height, "hash": b.hash, "time": b.ts, "mode": b.mode, "status": b.status});
    if with_finder {
        v["finder"] = json!(mask(&b.addr));
    }
    if let Some(u) = url_of(base, &b.hash) {
        v["url"] = json!(u);
    }
    v
}

async fn devices(State(st): State<St>) -> Json<Value> {
    let rows: Vec<Value> = st.pool.live.devices().into_iter().map(|d| json!({"key": d.key, "name": d.name, "url": d.url, "openSource": d.open_source, "working": d.working, "hashrate": d.hashrate, "best": d.best})).collect();
    let workers: u64 = rows.iter().map(|r| r["working"].as_u64().unwrap_or(0)).sum();
    Json(json!({"devices": rows, "workers": workers}))
}

fn range_of(range: &str) -> (i64, i64) {
    match range {
        "1h" => (60, 1),
        "7d" => (7 * 1440, 30),
        "30d" => (30 * 1440, 120),
        "all" => (60 * 1440, 150),
        _ => (1440, 5),
    }
}

async fn history(State(st): State<St>, Query(q): Query<RangeQ>) -> Json<Value> {
    let range = q.range.unwrap_or_else(|| "24h".into());
    let (minutes, slot) = range_of(&range);
    let since = unix_now() as i64 / 60 - minutes;
    let ledger = st.pool.ledger.clone();
    let rows = tokio::task::spawn_blocking(move || ledger.history(since, slot)).await.unwrap_or_default();
    let points: Vec<Value> = rows.into_iter().map(|(m, hr, miners, ok, stale, dup, nd)| json!([m * 60, hr, miners, ok, stale, dup, nd])).collect();
    Json(json!({"range": range, "minutes": minutes, "stepMinutes": slot, "keepDays": 60, "points": points}))
}

async fn device_history(State(st): State<St>, Query(q): Query<RangeQ>) -> Json<Value> {
    let range = q.range.unwrap_or_else(|| "24h".into());
    let (minutes, slot) = range_of(&range);
    let since = unix_now() as i64 / 60 - minutes;
    let ledger = st.pool.ledger.clone();
    let rows = tokio::task::spawn_blocking(move || ledger.device_history(since, slot)).await.unwrap_or_default();
    let mut by: HashMap<String, (String, f64, Vec<Value>)> = HashMap::new();
    for (m, key, name, hr, workers) in rows {
        let e = by.entry(key).or_insert((name, 0.0, Vec::new()));
        e.1 += hr;
        e.2.push(json!([m * 60, hr, workers]));
    }
    let mut list: Vec<_> = by.into_iter().collect();
    list.sort_by(|a, b| b.1 .1.partial_cmp(&a.1 .1).unwrap_or(std::cmp::Ordering::Equal));
    list.truncate(8);
    let devices: Vec<Value> = list.into_iter().map(|(k, (name, _, pts))| json!({"key": k, "name": name, "points": pts})).collect();
    Json(json!({"range": range, "minutes": minutes, "stepMinutes": slot, "keepDays": 60, "devices": devices}))
}

async fn engine(State(st): State<St>) -> Json<Value> {
    let p = &st.pool;
    let tpl = p.template.borrow().clone();
    let s = &p.stats;
    let mut lat: Vec<f64> = p.live.tip_ready_ms.lock().unwrap().iter().copied().collect();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (ok, stale, dup, inv) = (s.accepted.load(Relaxed), s.stale.load(Relaxed), s.dup.load(Relaxed), s.inv.load(Relaxed));
    let total = ok + stale + dup + inv;
    let frac = |n: u64| if total == 0 { 0.0 } else { n as f64 / total as f64 };
    let rates = *p.live.rates.lock().unwrap();
    let t = &p.timings;
    let (height, tip) = tpl.as_ref().map(|t| (t.height.saturating_sub(1), t.tip_hex.clone())).unwrap_or((0, String::new()));
    let hr = hashrate_obj(p);
    let online = p.live.workers_online();
    Json(json!({
        "pool": {"name": p.cfg.http.name, "version": env!("CARGO_PKG_VERSION"), "release": env!("CARGO_PKG_VERSION"), "uptime": p.started.elapsed().as_secs_f64(), "mode": "core", "explorerUrl": p.cfg.http.explorer_url},
        "node": {
            "height": height, "tip": tip, "tipTime": t.tip_unix.load(Relaxed), "connected": t.node_up.load(Relaxed), "transport": "http-rpc",
            "rttMs": t.rpc_ms.load(Relaxed) as f64, "p50": quantile(&lat, 0.5), "p95": quantile(&lat, 0.95), "samples": lat.len(),
            "batchMs": t.build_ms.load(Relaxed), "backend": "gbt", "candidatesAvailable": 1, "demandUnitsPerSec": rates.shares_per_sec,
        },
        "hashrate": hr,
        "miners": {"online": online, "total": s.conns.load(Relaxed), "v1": s.conns.load(Relaxed), "v2": 0, "edge": 0},
        "shares": {"perSec": rates.shares_per_sec, "ok": frac(ok), "stale": frac(stale), "dup": frac(dup), "inv": frac(inv), "total": total, "okN": ok, "staleN": stale, "dupN": dup, "invN": inv},
        "alloc": {"candidates": tpl.as_ref().map(|t| t.variants()).unwrap_or(1), "unitsPerSec": rates.shares_per_sec, "epochUse": 0.0, "overlap": 0},
        "net": {"connections": s.conns.load(Relaxed), "inBps": rates.in_bps, "outBps": rates.out_bps, "jobsPerSec": rates.jobs_per_sec, "connectsPerSec": rates.connects_per_sec,
                "accepted": s.connects.load(Relaxed), "closed": s.closed.load(Relaxed), "refused": s.refused.load(Relaxed), "bytesIn": s.bytes_in.load(Relaxed), "bytesOut": s.bytes_out.load(Relaxed)},
        "found": s.blocks.load(Relaxed),
        "modes": [{"mode": mode_name(p), "fee": fee(p), "available": true, "online": online, "hashrate": hr["now"]}],
        "photon": {"enabled": false},
    }))
}

async fn events(State(st): State<St>, Query(q): Query<SinceQ>) -> Json<Value> {
    let since = q.since.unwrap_or(0);
    let ev = st.pool.live.events.lock().unwrap();
    let mut out: Vec<Value> = ev.iter().filter(|e| e.seq > since).rev().take(100).map(|e| json!({"seq": e.seq, "t": e.t, "lvl": e.level, "msg": e.msg})).collect();
    out.reverse();
    Json(json!({"events": out, "last": ev.back().map(|e| e.seq).unwrap_or(0)}))
}

fn valid(p: &Pool, address: &str) -> bool {
    crate::addr::script_for_address(address, p.net).is_ok()
}

fn credit_json(base: &str, tip: i64, c: &(u32, String, i64, String)) -> Value {
    let (status, remaining) = match c.3.as_str() {
        "mature" => ("confirmed", 0),
        "orphaned" => ("orphaned", 0),
        _ => ("pending", (MATURITY - (tip - c.0 as i64 + 1)).max(0)),
    };
    let mut v = json!({"height": c.0, "hash": c.1, "amount": c.2, "status": status, "blockStatus": c.3, "maturesIn": remaining});
    if let Some(u) = url_of(base, &c.1) {
        v["url"] = json!(u);
    }
    v
}

fn tip_height(p: &Pool) -> i64 {
    p.template.borrow().as_ref().map(|t| t.height as i64 - 1).unwrap_or(0)
}

async fn account(State(st): State<St>, Path(address): Path<String>) -> Response {
    let p = st.pool.clone();
    if !valid(&p, &address) {
        return (StatusCode::BAD_REQUEST, "invalid address").into_response();
    }
    let now = unix_now() as i64;
    let (ledger, addr) = (p.ledger.clone(), address.clone());
    let (h1, h24, hist, (pending, received), (blocks, bt), (credits, ct)) = tokio::task::spawn_blocking(move || {
        (
            ledger.account_shares(&addr, now - 3600),
            ledger.account_shares(&addr, now - 86_400),
            ledger.account_history(&addr, now - 86_400),
            ledger.account_balance(&addr),
            ledger.account_blocks_page(&addr, 0, 20),
            ledger.account_credits_page(&addr, 0, 20),
        )
    })
    .await
    .unwrap_or_default();
    let (mut hashrate, mut workers) = (0.0, Vec::new());
    if let Some(a) = p.live.addrs.lock().unwrap().get(&address) {
        hashrate = a.meter.hashrate();
        for (name, w) in &a.workers {
            workers.push(json!({
                "name": name, "mode": mode_name(&p), "hashrate": w.meter.hashrate(), "difficulty": w.diff, "accepted": w.accepted, "stale": w.stale, "invalid": w.invalid,
                "seen": if w.conns > 0 { (now as u64).saturating_sub(w.last_seen).min(60) } else { (now as u64).saturating_sub(w.last_seen) },
                "device": crate::devices::classify(&w.agent).name, "best": w.best,
            }));
        }
    }
    workers.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let shares = |(d, n): (f64, i64)| if n > 0 { json!({mode_name(&p): {"difficulty": d, "count": n}}) } else { json!({}) };
    let base = &p.cfg.http.explorer_url;
    let tip = tip_height(&p);
    Json(json!({
        "address": address, "hashrate": hashrate, "workers": workers.len(), "workerList": workers,
        "shares1h": shares(h1), "shares24h": shares(h24),
        "balance": {"pending": pending, "confirmed": 0, "paid": received}, "minPayoutLeptons": 0,
        "creditsTotal": ct, "payoutsTotal": 0, "payoutsSum": 0, "blocksTotal": bt,
        "payouts": [], "credits": credits.iter().map(|c| credit_json(base, tip, c)).collect::<Vec<_>>(),
        "blocksFound": blocks.iter().map(|b| block_json(base, b, false)).collect::<Vec<_>>(),
        "history": hist.into_iter().map(|(t, d)| json!({"time": t, "hashrate": d * 4_294_967_296.0 / 600.0})).collect::<Vec<_>>(),
    }))
    .into_response()
}

async fn account_page(st: St, address: String, kind: &str, q: PageQ) -> Response {
    let p = st.pool.clone();
    if !valid(&p, &address) {
        return (StatusCode::BAD_REQUEST, "invalid address").into_response();
    }
    let (offset, limit) = (q.offset.unwrap_or(0).clamp(0, 10_000_000), q.limit.unwrap_or(20).clamp(1, 100));
    let (base, tip, ledger, kind) = (p.cfg.http.explorer_url.clone(), tip_height(&p), p.ledger.clone(), kind.to_string());
    let v = tokio::task::spawn_blocking(move || match kind.as_str() {
        "credits" => {
            let (rows, total) = ledger.account_credits_page(&address, offset, limit);
            json!({"total": total, "offset": offset, "items": rows.iter().map(|c| credit_json(&base, tip, c)).collect::<Vec<_>>()})
        }
        "blocks" => {
            let (rows, total) = ledger.account_blocks_page(&address, offset, limit);
            json!({"total": total, "offset": offset, "items": rows.iter().map(|b| block_json(&base, b, false)).collect::<Vec<_>>()})
        }
        _ => json!({"total": 0, "offset": offset, "items": []}),
    })
    .await
    .unwrap_or(Value::Null);
    Json(v).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_the_burst_then_blocks() {
        let l = Limiter::new(5);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!((0..8).filter(|_| l.allow(ip)).count(), 5);
        assert!(l.allow("10.0.0.2".parse().unwrap()));
    }

    #[test]
    fn forwarded_address_is_only_used_behind_a_trusted_proxy() {
        let peer: IpAddr = "10.0.0.9".parse().unwrap();
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7, 10.0.0.9"));
        assert_eq!(client_ip(peer, &h, false), peer);
        assert_eq!(client_ip(peer, &h, true), "203.0.113.7".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn addresses_are_masked_and_block_links_need_a_hash() {
        assert_eq!(mask("bc1qabcdefghijklmnopqrstuvwxyz"), "bc1qab..wxyz");
        assert_eq!(mask("short"), "short");
        let h = "ab".repeat(32);
        assert_eq!(url_of("https://mempool.space/", &h), Some(format!("https://mempool.space/block/{h}")));
        assert_eq!(url_of("", &h), None);
    }
}
