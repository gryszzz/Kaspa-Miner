use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::miner::DEFAULT_BATCH_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolEndpoint {
    pub host: String,
    pub port: u16,
    pub tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub pool: String,
    pub wallet: String,
    pub worker: String,
    pub threads: usize,
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    #[serde(default = "default_reconnect_secs")]
    pub reconnect_secs: u64,
}

impl Config {
    pub fn load(
        path: &Path,
        pool: Option<String>,
        wallet: Option<String>,
        worker: Option<String>,
        threads: Option<usize>,
        batch_size: Option<u64>,
    ) -> Result<Self> {
        let mut cfg = if path.exists() {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("Reading {}", path.display()))?;
            toml::from_str::<Config>(&raw).context("Parsing config.toml")?
        } else {
            Config::default()
        };

        if let Some(p) = pool {
            cfg.pool = p;
        }
        if let Some(w) = wallet {
            cfg.wallet = w;
        }
        if let Some(w) = worker {
            cfg.worker = w;
        }
        if let Some(t) = threads {
            cfg.threads = t;
        }
        if let Some(b) = batch_size {
            cfg.batch_size = b;
        }

        if cfg.pool.is_empty() {
            anyhow::bail!(
                "Pool required.  Use --pool stratum+tcp://host:port  or set pool in config.toml"
            );
        }
        if cfg.wallet.is_empty() {
            anyhow::bail!(
                "Wallet required.  Use --wallet <kaspa:...>  or set wallet in config.toml"
            );
        }
        if !cfg.wallet.starts_with("kaspa:") && !cfg.wallet.starts_with("kaspatest:") {
            anyhow::bail!("Wallet must be a Kaspa address beginning with kaspa: or kaspatest:");
        }
        if cfg.worker.trim().is_empty() {
            anyhow::bail!("Worker name cannot be empty");
        }
        if cfg.threads == 0 {
            anyhow::bail!("Thread count must be at least 1");
        }
        if cfg.threads > 512 {
            anyhow::bail!(
                "Thread count {0} is too high; refusing to start",
                cfg.threads
            );
        }
        if cfg.batch_size < 64 || cfg.batch_size > 1_048_576 {
            anyhow::bail!("Batch size must be between 64 and 1048576");
        }
        if cfg.reconnect_secs == 0 {
            cfg.reconnect_secs = 5;
        }

        Ok(cfg)
    }

    /// Parse stratum pool URLs into a normalized endpoint.
    pub fn pool_endpoint(&self) -> Result<PoolEndpoint> {
        let pool = self.pool.trim();
        let (url, tls) = if let Some(rest) = pool.strip_prefix("stratum+ssl://") {
            (rest, true)
        } else if let Some(rest) = pool.strip_prefix("ssl://") {
            (rest, true)
        } else if let Some(rest) = pool.strip_prefix("stratum+tcp://") {
            (rest, false)
        } else if let Some(rest) = pool.strip_prefix("stratum://") {
            (rest, false)
        } else if let Some(rest) = pool.strip_prefix("tcp://") {
            (rest, false)
        } else {
            (pool, false)
        };

        let (host, port_str) = url
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Pool URL missing port: {}", self.pool))?;

        if host.trim().is_empty() {
            anyhow::bail!("Pool URL missing host: {}", self.pool);
        }

        let port = port_str
            .parse::<u16>()
            .with_context(|| format!("Invalid port: {port_str}"))?;

        Ok(PoolEndpoint {
            host: host.to_string(),
            port,
            tls,
        })
    }

    pub fn login(&self) -> String {
        format!("{}.{}", self.wallet, self.worker)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pool: String::new(),
            wallet: String::new(),
            worker: "worker1".into(),
            threads: num_cpus::get(),
            batch_size: DEFAULT_BATCH_SIZE,
            reconnect_secs: 5,
        }
    }
}

fn default_batch_size() -> u64 {
    DEFAULT_BATCH_SIZE
}

fn default_reconnect_secs() -> u64 {
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pool: &str) -> Config {
        Config {
            pool: pool.to_string(),
            wallet: "kaspa:test".to_string(),
            worker: "rig".to_string(),
            threads: 1,
            batch_size: DEFAULT_BATCH_SIZE,
            reconnect_secs: 5,
        }
    }

    #[test]
    fn parses_tcp_pool_urls() {
        let endpoint = cfg("stratum+tcp://pool.example.com:5555")
            .pool_endpoint()
            .unwrap();

        assert_eq!(
            endpoint,
            PoolEndpoint {
                host: "pool.example.com".to_string(),
                port: 5555,
                tls: false,
            }
        );
    }

    #[test]
    fn parses_tls_pool_urls() {
        let endpoint = cfg("stratum+ssl://pool.example.com:443")
            .pool_endpoint()
            .unwrap();

        assert_eq!(
            endpoint,
            PoolEndpoint {
                host: "pool.example.com".to_string(),
                port: 443,
                tls: true,
            }
        );
    }

    #[test]
    fn rejects_missing_pool_port() {
        let err = cfg("stratum+tcp://pool.example.com")
            .pool_endpoint()
            .unwrap_err()
            .to_string();

        assert!(err.contains("missing port"));
    }
}
