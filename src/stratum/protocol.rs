use anyhow::{bail, Result};
use num_bigint::BigUint;
use num_traits::{One, ToPrimitive};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::algorithm::kheavyhash::Target;

const DIFFICULTY_ONE_MANTISSA: u64 = 0xffff;
const DIFFICULTY_ONE_EXPONENT: usize = 208;

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Request {
    pub id: Option<u64>,
    pub method: String,
    pub params: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
}

// ── outgoing helpers ─────────────────────────────────────────────────────────

pub fn subscribe_msg(agent: &str) -> String {
    let r = Request {
        id: Some(1),
        method: "mining.subscribe".into(),
        params: vec![agent.into(), "EthereumStratum/1.0.0".into()],
    };
    serde_json::to_string(&r).unwrap() + "\n"
}

pub fn authorize_msg(wallet: &str, worker: &str) -> String {
    let login = format!("{wallet}.{worker}");
    let r = Request {
        id: Some(2),
        method: "mining.authorize".into(),
        params: vec![login.into(), "".into()],
    };
    serde_json::to_string(&r).unwrap() + "\n"
}

pub fn submit_msg(id: u64, login: &str, job_id: &str, nonce: u64) -> String {
    let nonce_hex = format!("{nonce:016x}");
    let r = Request {
        id: Some(id),
        method: "mining.submit".into(),
        params: vec![login.into(), job_id.into(), nonce_hex.into()],
    };
    serde_json::to_string(&r).unwrap() + "\n"
}

// ── incoming work ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Work {
    pub job_id: String,
    pub pre_pow_hash: [u8; 32],
    pub timestamp: u64,
    pub target: Target,
    pub nonce_fixed: u64,
    pub nonce_mask: u64,
}

impl Work {
    pub fn from_notify(
        params: &Value,
        target: Target,
        nonce_fixed: u64,
        nonce_mask: u64,
    ) -> Result<Self> {
        let arr = params
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("notify params not array"))?;
        if arr.len() < 2 {
            bail!("notify: expected at least 2 params, got {}", arr.len());
        }

        let job_id = arr[0]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("bad job_id"))?
            .to_string();

        if arr.len() >= 4 {
            let pre_pow_hash = decode32(arr[1].as_str().unwrap_or(""))?;
            let timestamp = parse_timestamp(arr[2].as_str().unwrap_or("0"))?;
            let target = decode_target(arr[3].as_str().unwrap_or(""))?;
            return Ok(Work {
                job_id,
                pre_pow_hash,
                timestamp,
                target,
                nonce_fixed,
                nonce_mask,
            });
        }

        let header = decode_hex(arr[1].as_str().unwrap_or(""))?;
        if header.len() != 40 {
            bail!("notify header hash must be 40 bytes, got {}", header.len());
        }

        let mut pre_pow_hash = [0u8; 32];
        pre_pow_hash.copy_from_slice(&header[..32]);
        let timestamp = u64::from_le_bytes(header[32..40].try_into()?);

        Ok(Work {
            job_id,
            pre_pow_hash,
            timestamp,
            target,
            nonce_fixed,
            nonce_mask,
        })
    }
}

pub fn target_from_difficulty(difficulty: f64) -> Result<Target> {
    if !difficulty.is_finite() || difficulty <= 0.0 {
        bail!("invalid stratum difficulty: {difficulty}");
    }

    let rounded = difficulty.round();
    if (difficulty - rounded).abs() > f64::EPSILON * difficulty.max(1.0) {
        bail!("fractional stratum difficulty is not supported yet: {difficulty}");
    }

    let diff = rounded
        .to_u64()
        .ok_or_else(|| anyhow::anyhow!("difficulty too large: {difficulty}"))?;
    if diff == 0 {
        bail!("difficulty cannot be zero");
    }

    let mut target = BigUint::from(DIFFICULTY_ONE_MANTISSA) << DIFFICULTY_ONE_EXPONENT;
    target /= diff;
    biguint_to_target(target)
}

pub fn extranonce_to_mask(prefix: &str, nonce_size: Option<u64>) -> Result<(u64, u64)> {
    let prefix = prefix.trim_start_matches("0x");
    if prefix.len() > 16 || !prefix.len().is_multiple_of(2) {
        bail!("invalid extranonce prefix length: {prefix}");
    }

    let prefix_bytes = prefix.len() as u64 / 2;
    let free_bytes = nonce_size.unwrap_or(8u64.saturating_sub(prefix_bytes));
    if prefix_bytes + free_bytes > 8 {
        bail!("extranonce prefix plus nonce size exceeds 8 bytes");
    }

    let fixed = if prefix.is_empty() {
        0
    } else {
        u64::from_str_radix(prefix, 16)?
    } << (free_bytes * 8);
    let mask = if free_bytes >= 8 {
        u64::MAX
    } else {
        (1u64 << (free_bytes * 8)) - 1
    };

    Ok((fixed, mask))
}

fn decode32(s: &str) -> Result<[u8; 32]> {
    let b = decode_hex(s)?;
    b.try_into()
        .map_err(|_| anyhow::anyhow!("expected 32-byte hex, got {} bytes", s.len() / 2))
}

fn decode_target(s: &str) -> Result<Target> {
    let bytes = decode32(s)?;
    let mut target = [0u64; 4];
    for i in 0..4 {
        target[i] = u64::from_le_bytes(bytes[i * 8..(i + 1) * 8].try_into()?);
    }
    Ok(target)
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    Ok(hex::decode(s.trim_start_matches("0x"))?)
}

fn parse_timestamp(s: &str) -> Result<u64> {
    let s = s.trim_start_matches("0x");
    Ok(u64::from_str_radix(s, 16).unwrap_or(0))
}

fn biguint_to_target(value: BigUint) -> Result<Target> {
    if value.bits() > 256 {
        bail!("target exceeds 256 bits");
    }

    let mut bytes = value.to_bytes_le();
    bytes.resize(32, 0);
    let mut target = [0u64; 4];
    for i in 0..4 {
        target[i] = u64::from_le_bytes(bytes[i * 8..(i + 1) * 8].try_into()?);
    }
    Ok(target)
}

pub fn max_target() -> Target {
    biguint_to_target((BigUint::one() << 255usize) - BigUint::one()).unwrap_or([u64::MAX; 4])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn difficulty_one_matches_expected_target_shape() {
        let target = target_from_difficulty(1.0).unwrap();
        assert_eq!(target[0], 0);
        assert_eq!(target[1], 0);
        assert_eq!(target[2], 0);
        assert_eq!(target[3], 0xffff << 16);
    }

    #[test]
    fn extranonce_prefix_is_high_part_of_nonce() {
        let (fixed, mask) = extranonce_to_mask("0001", Some(6)).unwrap();
        assert_eq!(fixed, 0x0001_0000_0000_0000);
        assert_eq!(mask, 0x0000_ffff_ffff_ffff);
    }

    #[test]
    fn parses_common_kaspa_notify_header() {
        let params = json!([
            "34",
            "2461684d90ef4e9fa55ca550ed4f9dd472d7bde502e01bf45ab7a3336d43cc9d4b9c7b4886010000"
        ]);
        let work = Work::from_notify(
            &params,
            target_from_difficulty(4096.0).unwrap(),
            0,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(work.job_id, "34");
        assert_eq!(work.pre_pow_hash[0], 0x24);
        assert_eq!(work.timestamp, 1_676_253_305_931);
    }
}
