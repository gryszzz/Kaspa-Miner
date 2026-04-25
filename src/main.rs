use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

mod algorithm;
mod config;
mod fleet;
mod miner;
mod stats;
mod stratum;
mod tui;

use config::Config;
use stats::Stats;

#[derive(Debug, Parser)]
#[command(
    name = "kaspa-miner",
    version,
    about = "KASPilot: a Kaspa-only CPU miner with Common Stratum support"
)]
struct Cli {
    /// Path to the TOML config file.
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Pool URL, for example stratum+tcp://pool.example.com:5555.
    #[arg(long)]
    pool: Option<String>,

    /// Kaspa payout address, for example kaspa:....
    #[arg(long)]
    wallet: Option<String>,

    /// Worker/rig name appended to the wallet for pool auth.
    #[arg(long)]
    worker: Option<String>,

    /// Number of CPU mining threads.
    #[arg(short, long)]
    threads: Option<usize>,

    /// Nonces per worker batch before checking for fresh pool work.
    #[arg(long)]
    batch_size: Option<u64>,

    /// Disable the terminal dashboard and print logs instead.
    #[arg(long)]
    no_tui: bool,

    /// Run an offline hash benchmark instead of connecting to a pool.
    #[arg(long)]
    benchmark: bool,

    /// Benchmark duration in seconds.
    #[arg(long, default_value_t = 10)]
    bench_seconds: u64,

    /// Run ASIC fleet controller mode instead of CPU mining.
    #[arg(long)]
    fleet: bool,

    /// Path to the ASIC fleet TOML config.
    #[arg(long, default_value = "fleet.toml")]
    fleet_config: PathBuf,

    /// Poll fleet once and exit.
    #[arg(long)]
    fleet_once: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.fleet {
        fleet::run(&cli.fleet_config, cli.fleet_once).await?;
        return Ok(());
    }

    if cli.benchmark {
        let threads = cli.threads.unwrap_or_else(num_cpus::get);
        let batch_size = cli.batch_size.unwrap_or(miner::DEFAULT_BATCH_SIZE);
        miner::benchmark(threads, batch_size, Duration::from_secs(cli.bench_seconds)).await?;
        return Ok(());
    }

    let config = Arc::new(Config::load(
        &cli.config,
        cli.pool,
        cli.wallet,
        cli.worker,
        cli.threads,
        cli.batch_size,
    )?);

    let stats = Arc::new(Stats::new(config.threads));
    if cli.no_tui {
        miner::run(config, stats).await
    } else {
        tui::run(config, stats).await
    }
}
