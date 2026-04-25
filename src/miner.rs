use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc};

use crate::algorithm::kheavyhash::Target;
use crate::algorithm::JobContext;
use crate::config::Config;
use crate::stats::{format_hashrate, Stats};
use crate::stratum::{Event, Submission, Work};

/// Default number of nonces each thread tries before checking for new work.
///
/// This keeps the hot path tight while still checking for fresh 10 BPS-era work
/// frequently enough to limit stale scanning on CPU-class hashrates.
pub const DEFAULT_BATCH_SIZE: u64 = 4_096;

#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub threads: usize,
    pub batch_size: u64,
    pub hashes: u64,
    pub seconds: f64,
    pub hashrate: f64,
}

/// Spin up all worker threads and the stratum client; return when killed.
pub async fn run(config: Arc<Config>, stats: Arc<Stats>) -> anyhow::Result<()> {
    let (work_tx, _work_rx) = broadcast::channel::<Work>(16);
    let (sub_tx, sub_rx) = mpsc::channel::<Submission>(64);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);

    // ── stratum task ─────────────────────────────────────────────────────────
    let cfg2 = config.clone();
    let st2 = stats.clone();
    let wtx2 = work_tx.clone();
    let etx2 = event_tx.clone();
    tokio::spawn(async move {
        crate::stratum::run(cfg2, st2, wtx2, sub_rx, etx2).await;
    });

    // ── mining threads ───────────────────────────────────────────────────────
    let stop = Arc::new(AtomicBool::new(false));

    let mut handles = Vec::with_capacity(config.threads);
    for tid in 0..config.threads {
        let mut work_rx = work_tx.subscribe();
        let sub_tx_t = sub_tx.clone();
        let stats_t = stats.clone();
        let stop_t = stop.clone();
        let threads = config.threads;

        let batch_size = config.batch_size;
        let handle = std::thread::spawn(move || {
            mine_thread(
                tid,
                threads,
                batch_size,
                &mut work_rx,
                sub_tx_t,
                stats_t,
                stop_t,
            );
        });
        handles.push(handle);
    }

    // ── event logger (plain mode) ─────────────────────────────────────────────
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("[ctl] shutdown requested");
                break;
            }
            ev = event_rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    Event::Connected    => println!("[pool] connected"),
                    Event::Disconnected => println!("[pool] disconnected"),
                    Event::NewJob(id)   => println!("[job]  new job {id}"),
                    Event::Difficulty(d) => println!("[pool] difficulty {d}"),
                    Event::Extranonce(prefix) => println!("[pool] extranonce {prefix}"),
                    Event::ShareAccepted          => println!("[share] accepted"),
                    Event::ShareRejected(reason)  => println!("[share] rejected: {reason}"),
                    Event::Error(msg)             => println!("[err]  {msg}"),
                }
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    for handle in handles {
        let _ = handle.join();
    }
    Ok(())
}

pub fn mine_thread_pub(
    tid: usize,
    threads: usize,
    batch_size: u64,
    work_rx: &mut broadcast::Receiver<Work>,
    sub_tx: mpsc::Sender<Submission>,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
) {
    let mut job: Option<JobContext> = None;
    let mut current_job_id = String::new();
    let mut nonce_fixed = 0u64;
    let mut nonce_mask = u64::MAX;

    // start nonce spread across threads to avoid overlap
    let mut cursor: u64 = tid as u64;
    let stride = threads.max(1) as u64;

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        // non-blocking check for new work
        match work_rx.try_recv() {
            Ok(work) => {
                current_job_id = work.job_id.clone();
                job = Some(JobContext::new(
                    work.pre_pow_hash,
                    work.timestamp,
                    work.target,
                ));
                nonce_fixed = work.nonce_fixed;
                nonce_mask = work.nonce_mask;
                cursor = tid as u64;
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => {
                // missed messages; drain to latest
                while let Ok(w) = work_rx.try_recv() {
                    current_job_id = w.job_id.clone();
                    job = Some(JobContext::new(w.pre_pow_hash, w.timestamp, w.target));
                    nonce_fixed = w.nonce_fixed;
                    nonce_mask = w.nonce_mask;
                    cursor = tid as u64;
                }
            }
            Err(_) => {}
        }

        if let Some(ref ctx) = job {
            // mine a batch
            for _ in 0..batch_size {
                let nonce = nonce_fixed | (cursor & nonce_mask);
                cursor = cursor.wrapping_add(stride);
                if let Some(found) = ctx.try_nonce(nonce) {
                    let _ = sub_tx.try_send(Submission {
                        job_id: current_job_id.clone(),
                        nonce: found,
                    });
                }
            }
            stats.add_hashes(tid, batch_size);
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn mine_thread(
    tid: usize,
    threads: usize,
    batch_size: u64,
    work_rx: &mut broadcast::Receiver<Work>,
    sub_tx: mpsc::Sender<Submission>,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
) {
    mine_thread_pub(tid, threads, batch_size, work_rx, sub_tx, stats, stop);
}

pub async fn benchmark(threads: usize, batch_size: u64, duration: Duration) -> anyhow::Result<()> {
    let result = benchmark_once(threads, batch_size, duration).await?;

    println!(
        "KASPilot benchmark: {} across {} threads, batch {}, {} hashes in {:.1}s",
        format_hashrate(result.hashrate),
        result.threads,
        result.batch_size,
        result.hashes,
        result.seconds
    );
    Ok(())
}

pub async fn tune(
    max_threads: usize,
    batch_sizes: &[u64],
    duration: Duration,
) -> anyhow::Result<()> {
    if max_threads == 0 {
        anyhow::bail!("Tune max threads must be at least 1");
    }
    if batch_sizes.is_empty() {
        anyhow::bail!("Tune requires at least one batch size");
    }

    let mut results = Vec::new();
    let thread_counts = tune_thread_counts(max_threads);

    println!(
        "Autotune matrix: threads {:?}, batches {:?}, {}s each\n",
        thread_counts,
        batch_sizes,
        duration.as_secs().max(1)
    );

    for threads in thread_counts {
        for &batch_size in batch_sizes {
            let result = benchmark_once(threads, batch_size, duration).await?;
            println!(
                "  {:>2} threads | batch {:>6} | {:>12}",
                result.threads,
                result.batch_size,
                format_hashrate(result.hashrate)
            );
            results.push(result);
        }
    }

    results.sort_by(|a, b| b.hashrate.total_cmp(&a.hashrate));

    println!("\nTop settings");
    println!("{:<8} {:<10} {:>14}", "threads", "batch", "hashrate");
    println!("{}", "-".repeat(36));
    for result in results.iter().take(8) {
        println!(
            "{:<8} {:<10} {:>14}",
            result.threads,
            result.batch_size,
            format_hashrate(result.hashrate)
        );
    }

    if let Some(best) = results.first() {
        println!(
            "\nRecommended config: threads = {}, batch_size = {}",
            best.threads, best.batch_size
        );
    }

    Ok(())
}

async fn benchmark_once(
    threads: usize,
    batch_size: u64,
    duration: Duration,
) -> anyhow::Result<BenchmarkResult> {
    if threads == 0 {
        anyhow::bail!("Thread count must be at least 1");
    }
    if !(64..=1_048_576).contains(&batch_size) {
        anyhow::bail!("Batch size must be between 64 and 1048576");
    }

    let stats = Arc::new(Stats::new(threads));
    let stop = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let mut handles = Vec::with_capacity(threads);
    let target: Target = [u64::MAX; 4];

    for tid in 0..threads {
        let stats_t = stats.clone();
        let stop_t = stop.clone();
        handles.push(std::thread::spawn(move || {
            let seed = benchmark_seed(tid);
            let ctx = JobContext::new(seed, 1_714_000_000, target);
            let mut nonce = tid as u64;
            let stride = threads.max(1) as u64;
            while !stop_t.load(Ordering::Relaxed) {
                for _ in 0..batch_size {
                    let _ = ctx.try_nonce(nonce);
                    nonce = nonce.wrapping_add(stride);
                }
                stats_t.add_hashes(tid, batch_size);
            }
        }));
    }

    tokio::time::sleep(duration).await;
    stop.store(true, Ordering::Relaxed);
    for handle in handles {
        let _ = handle.join();
    }

    let seconds = start.elapsed().as_secs_f64().max(0.001);
    let hashes = stats.total_hashes();
    let hashrate = hashes as f64 / seconds;

    Ok(BenchmarkResult {
        threads,
        batch_size,
        hashes,
        seconds,
        hashrate,
    })
}

fn benchmark_seed(tid: usize) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&(tid as u64 + 1).to_le_bytes());
    seed[8..16].copy_from_slice(&0x9e37_79b9_7f4a_7c15u64.to_le_bytes());
    seed[16..24].copy_from_slice(&0xd1b5_4a32_d192_ed03u64.to_le_bytes());
    seed[24..32].copy_from_slice(&0x94d0_49bb_1331_11ebu64.to_le_bytes());
    seed
}

fn tune_thread_counts(max_threads: usize) -> Vec<usize> {
    let mut counts = vec![1];
    let mut value = 2;
    while value < max_threads {
        counts.push(value);
        value *= 2;
    }
    if !counts.contains(&max_threads) {
        counts.push(max_threads);
    }
    counts
}
