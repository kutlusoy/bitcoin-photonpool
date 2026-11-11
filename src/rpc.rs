use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Uri};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Instant;

pub struct NodeRpc {
    client: Client<HttpConnector, Full<Bytes>>,
    uri: Uri,
    auth: RwLock<String>,
    cookie: Option<String>,
    seq: AtomicU64,
    pub last_rtt_us: AtomicU64,
}

impl NodeRpc {
    pub fn new(url: &str, auth: &str) -> Result<Self> {
        let cookie = auth.strip_prefix("cookie:").map(str::to_string);
        Ok(Self {
            client: {
                let mut c = HttpConnector::new();
                c.set_nodelay(true);
                Client::builder(TokioExecutor::new()).pool_max_idle_per_host(4).build(c)
            },
            uri: url.parse().with_context(|| format!("bad rpc_url {url}"))?,
            auth: RwLock::new(Self::header(auth, cookie.as_deref())?),
            cookie,
            seq: AtomicU64::new(0),
            last_rtt_us: AtomicU64::new(0),
        })
    }

    fn header(auth: &str, cookie: Option<&str>) -> Result<String> {
        let creds = match (cookie, auth.strip_prefix("env:")) {
            (Some(path), _) => std::fs::read_to_string(path).with_context(|| {
                format!("reading cookie {path} (the node writes it when it starts: is the node running?)")
            })?.trim().to_string(),
            (None, Some(var)) => std::env::var(var).with_context(|| format!("environment variable {var} is not set"))?,
            (None, None) => auth.to_string(),
        };
        Ok(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(creds)))
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        match self.call_once(method, &params).await {
            Err(e) if self.cookie.is_some() && e.to_string().contains("HTTP 401") => {
                *self.auth.write().unwrap() = Self::header("", self.cookie.as_deref())?;
                self.call_once(method, &params).await
            }
            r => r,
        }
    }

    async fn call_raw(&self, method: &str, params: &Value) -> Result<Bytes> {
        match self.send_raw(method, params).await {
            Err(e) if self.cookie.is_some() && e.to_string().contains("HTTP 401") => {
                *self.auth.write().unwrap() = Self::header("", self.cookie.as_deref())?;
                self.send_raw(method, params).await
            }
            r => r,
        }
    }

    async fn send_raw(&self, method: &str, params: &Value) -> Result<Bytes> {
        let body = json!({"jsonrpc": "1.0", "id": self.seq.fetch_add(1, Ordering::Relaxed), "method": method, "params": params});
        let req = Request::post(self.uri.clone())
            .header("content-type", "application/json")
            .header("authorization", self.auth.read().unwrap().clone())
            .body(Full::new(Bytes::from(serde_json::to_vec(&body)?)))?;
        let t = Instant::now();
        let resp = self.client.request(req).await.with_context(|| format!("{method}: node unreachable"))?;
        let status = resp.status();
        let bytes = resp.into_body().collect().await?.to_bytes();
        self.last_rtt_us.store(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        if status == hyper::StatusCode::UNAUTHORIZED {
            bail!("{method}: HTTP 401, node refused the credentials");
        }
        Ok(bytes)
    }

    async fn call_once(&self, method: &str, params: &Value) -> Result<Value> {
        let bytes = self.send_raw(method, params).await?;
        let mut v: Value = serde_json::from_slice(&bytes).map_err(|e| anyhow!("{method}: unparsable body: {e}"))?;
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            bail!("{method}: {err}");
        }
        Ok(v["result"].take())
    }

    pub async fn best_block_hash(&self) -> Result<String> {
        Ok(self.call("getbestblockhash", json!([])).await?.as_str().ok_or_else(|| anyhow!("bad getbestblockhash"))?.to_string())
    }

    pub async fn chain_info(&self) -> Result<(String, u32, bool)> {
        let v = self.call("getblockchaininfo", json!([])).await?;
        Ok((v["chain"].as_str().unwrap_or("").to_string(), v["blocks"].as_u64().unwrap_or(0) as u32, v["initialblockdownload"].as_bool().unwrap_or(false)))
    }

    pub async fn submit_block(&self, block_hex: &str) -> Result<Option<String>> {
        let v = self.call("submitblock", json!([block_hex])).await?;
        Ok(v.as_str().map(str::to_string))
    }

    /// Raw JSON-RPC response body of getblocktemplate, parsed once by the caller (the body is megabytes on a full block).
    pub async fn template_raw(&self) -> Result<Bytes> {
        self.call_raw("getblocktemplate", &json!([{"rules": ["segwit"]}])).await
    }

    /// Blocks until the tip differs from `current_tip` or `timeout_ms` has passed; returns the tip hash (node RPC `waitfornewblock`).
    pub async fn wait_for_new_block(&self, timeout_ms: u64, current_tip: &str) -> Result<(String, u32)> {
        let v = self.call("waitfornewblock", json!([timeout_ms.max(1), current_tip])).await?;
        Ok((v["hash"].as_str().ok_or_else(|| anyhow!("bad waitfornewblock"))?.to_string(), v["height"].as_u64().unwrap_or(0) as u32))
    }
}
