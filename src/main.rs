use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

mod algorithm;
mod branding;
mod config;
mod fleet;
mod gpu;
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
    about = "KASPilot: Kaspa ASIC fleet controller plus GPU supervisor and CPU benchmark/dev miner",
    long_about = "KASPilot is a Kaspa-only operations terminal for ASIC fleet telemetry, managed GPU engine supervision, pool validation, CPU benchmarking, and Common Stratum development mining.",
    after_help = "Modes: --fleet for ASIC ops, --gpu for managed GPU engines, --benchmark for local kHeavyHash speed, --tune for CPU settings, or provide pool/wallet config for dev mining."
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

    /// Sweep CPU thread/batch combinations and rank local hashrate.
    #[arg(long)]
    tune: bool,

    /// Benchmark duration in seconds.
    #[arg(long, default_value_t = 10)]
    bench_seconds: u64,

    /// Per-test duration in seconds for --tune.
    #[arg(long, default_value_t = 5)]
    tune_seconds: u64,

    /// Maximum thread count to test during --tune.
    #[arg(long)]
    tune_max_threads: Option<usize>,

    /// Comma-separated batch sizes to test during --tune.
    #[arg(long, value_delimiter = ',', default_value = "1024,4096,16384,65536")]
    tune_batches: Vec<u64>,

    /// Run managed Kaspa GPU mining through gpu.toml.
    #[arg(long)]
    gpu: bool,

    /// Path to the GPU engine TOML config.
    #[arg(long, default_value = "gpu.toml")]
    gpu_config: PathBuf,

    /// Run the GPU engine once without watchdog restart.
    #[arg(long)]
    gpu_once: bool,

    /// Print local GPU/runtime discovery and exit.
    #[arg(long)]
    gpu_info: bool,

    /// Run ASIC fleet controller mode instead of CPU mining.
    #[arg(long)]
    fleet: bool,

    /// Path to the ASIC fleet TOML config.
    #[arg(long, default_value = "fleet.toml")]
    fleet_config: PathBuf,

    /// Poll fleet once and exit.
    #[arg(long)]
    fleet_once: bool,

    /// Render the mining cockpit preview SVG and exit.
    #[arg(long, hide = true)]
    render_ui_preview: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.gpu_info {
        gpu::print_info();
        return Ok(());
    }

    if cli.fleet {
        branding::print_banner("ASIC FLEET CONTROL");
        fleet::run(&cli.fleet_config, cli.fleet_once).await?;
        return Ok(());
    }

    if cli.tune {
        branding::print_banner("CPU AUTOTUNE");
        let max_threads = cli.tune_max_threads.unwrap_or_else(num_cpus::get);
        miner::tune(
            max_threads,
            &cli.tune_batches,
            Duration::from_secs(cli.tune_seconds),
        )
        .await?;
        return Ok(());
    }

    if cli.benchmark {
        branding::print_banner("CPU BENCHMARK");
        let threads = cli.threads.unwrap_or_else(num_cpus::get);
        let batch_size = cli.batch_size.unwrap_or(miner::DEFAULT_BATCH_SIZE);
        miner::benchmark(threads, batch_size, Duration::from_secs(cli.bench_seconds)).await?;
        return Ok(());
    }

    if let Some(path) = cli.render_ui_preview {
        tui::write_preview_svg(&path)?;
        println!("Rendered mining cockpit preview to {}", path.display());
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

    if cli.gpu {
        branding::print_banner("GPU MINER CONTROL");
        gpu::run(config, &cli.gpu_config, cli.gpu_once).await?;
        return Ok(());
    }

    let stats = Arc::new(Stats::new(config.threads));
    if cli.no_tui {
        branding::print_banner("CPU MINER");
        miner::run(config, stats).await
    } else {
        tui::run(config, stats).await
    }
}
