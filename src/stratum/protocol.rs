use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Request {
    pub id:     Option<u64>,
    pub method: String,
    pub params: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id:     Option<u64>,
    pub result: Option<Value>,
    pub error:  Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
}

// ── outgoing helpers ─────────────────────────────────────────────────────────

pub fn subscribe_msg(agent: &str) -> String {
    let r = Request {
        id:     Some(1),
        method: "mining.subscribe".into(),
        params: vec![agent.into()],
    };
    serde_json::to_string(&r).unwrap() + "\n"
}

pub fn authorize_msg(wallet: &str, worker: &str) -> String {
    let login = format!("{wallet}.{worker}");
    let r = Request {
        id:     Some(2),
        method: "mining.authorize".into(),
        params: vec![login.into(), "x".into()],
    };
    serde_json::to_string(&r).unwrap() + "\n"
}

pub fn submit_msg(id: u64, worker: &str, job_id: &str, nonce: u64) -> String {
    let nonce_hex = format!("{nonce:016x}");
    let r = Request {
        id:     Some(id),
        method: "mining.submit".into(),
        params: vec![worker.into(), job_id.into(), nonce_hex.into()],
    };
    serde_json::to_string(&r).unwrap() + "\n"
}

// ── incoming work ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Work {
    pub job_id:       String,
    pub pre_pow_hash: [u8; 32],
    pub timestamp:    u64,
    pub target:       [u8; 32],
}

impl Work {
    pub fn from_notify(params: &Value) -> Result<Self> {
        let arr = params.as_array().ok_or_else(|| anyhow::anyhow!("notify params not array"))?;
        if arr.len() < 4 {
            bail!("notify: expected ≥4 params, got {}", arr.len());
        }

        let job_id = arr[0].as_str().ok_or_else(|| anyhow::anyhow!("bad job_id"))?.to_string();

        let pre_pow_hash = decode32(arr[1].as_str().unwrap_or(""))?;

        let timestamp = parse_timestamp(arr[2].as_str().unwrap_or("0"))?;

        let target = decode32(arr[3].as_str().unwrap_or(""))?;

        Ok(Work { job_id, pre_pow_hash, timestamp, target })
    }
}

fn decode32(s: &str) -> Result<[u8; 32]> {
    let b = hex::decode(s.trim_start_matches("0x"))?;
    b.try_into().map_err(|_| anyhow::anyhow!("expected 32-byte hex, got {} bytes", s.len() / 2))
}

fn parse_timestamp(s: &str) -> Result<u64> {
    let s = s.trim_start_matches("0x");
    Ok(u64::from_str_radix(s, 16).unwrap_or(0))
}
