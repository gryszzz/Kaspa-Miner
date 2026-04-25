use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::miner::DEFAULT_BATCH_SIZE;

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

    /// Parse "stratum+tcp://host:port" → (host, port).
    pub fn pool_host_port(&self) -> Result<(String, u16)> {
        let url = self
            .pool
            .trim_start_matches("stratum+tcp://")
            .trim_start_matches("stratum://")
            .trim_start_matches("tcp://");

        if self.pool.starts_with("stratum+ssl://") || self.pool.starts_with("ssl://") {
            anyhow::bail!("TLS stratum URLs are not implemented yet; use stratum+tcp://host:port");
        }

        let (host, port_str) = url
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Pool URL missing port: {}", self.pool))?;

        let port = port_str
            .parse::<u16>()
            .with_context(|| format!("Invalid port: {port_str}"))?;

        Ok((host.to_string(), port))
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
