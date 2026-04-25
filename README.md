# KASPilot

```text
 _  __    _    ____  ____  _ _       _
| |/ /   / \  / ___||  _ \(_) | ___ | |_
| ' /   / _ \ \___ \| |_) | | |/ _ \| __|
| . \  / ___ \ ___) |  __/| | | (_) | |_
|_|\_\/_/   \_\____/|_|   |_|_|\___/ \__|

Kaspa ASIC fleet controller / CPU benchmark miner / Rust
```

KASPilot is a Kaspa operations console with two lanes: a vendor-neutral ASIC fleet controller for production rigs, and a Kaspa-only CPU miner/benchmark for development, pool validation, and protocol testing. The CPU miner is built around the current Kaspa Common Stratum flow: `mining.set_difficulty`, `mining.set_extranonce`, compact `mining.notify` jobs, and 8-byte nonce submission.

> Reality check: profitable Kaspa mining is ASIC territory. The CPU path stays useful as a dev miner and benchmark harness; the production direction is fleet control, monitoring, and ASIC operations.

## Feature Grid

| Layer | Status |
| --- | --- |
| Coin target | Kaspa only |
| ASIC fleet | Vendor-neutral reachability and expected TH/s monitor |
| Algorithm | kHeavyHash PoW path using `kaspa-hashes` primitives |
| Pool protocol | Kaspa Common Stratum over TCP |
| Difficulty | Pool share target from `mining.set_difficulty` |
| Nonce space | Extranonce prefix plus per-thread nonce scanning |
| Runtime | Multi-threaded CPU workers with non-overlapping nonce stride |
| UI | Ratatui terminal dashboard or plain log mode |
| Ops | Reconnect loop, share accept/reject counters, benchmark mode |

## Quick Start

```sh
cargo build --release
cp config.example.toml config.toml
cp fleet.example.toml fleet.toml
```

Edit `config.toml`:

```toml
pool = "stratum+tcp://pool.example.com:5555"
wallet = "kaspa:your_wallet_address"
worker = "rig-01"
threads = 8
batch_size = 4096
reconnect_secs = 5
```

Run the dashboard:

```sh
./target/release/kaspa-miner
```

Run ASIC fleet controller once:

```sh
./target/release/kaspa-miner --fleet --fleet-once
```

Run ASIC fleet controller continuously:

```sh
./target/release/kaspa-miner --fleet
```

Run plain logs:

```sh
./target/release/kaspa-miner --no-tui
```

Override config from the command line:

```sh
./target/release/kaspa-miner \
  --pool stratum+tcp://pool.example.com:5555 \
  --wallet kaspa:your_wallet_address \
  --worker rig-01 \
  --threads 8
```

Benchmark local hashing without a pool:

```sh
./target/release/kaspa-miner --benchmark --threads 8 --bench-seconds 15
```

Build for the current machine:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

## CLI

```text
--config <PATH>       Config file path, default: config.toml
--pool <URL>          stratum+tcp://host:port
--wallet <ADDRESS>    kaspa: or kaspatest: address
--worker <NAME>       Worker name appended to wallet for pool login
--threads <N>         CPU worker threads
--batch-size <N>      Nonces per thread before checking pool work
--no-tui              Plain terminal logs
--benchmark           Offline hashrate benchmark
--bench-seconds <N>   Benchmark duration
--fleet               ASIC fleet controller mode
--fleet-config <PATH> Fleet config path, default: fleet.toml
--fleet-once          Poll fleet once and exit
```

## ASIC Fleet

`fleet.toml` describes production devices without locking the project to one vendor API:

```toml
poll_secs = 30
timeout_ms = 750

[[devices]]
name = "kas-rig-01"
host = "192.168.1.101"
model = "KS-series"
location = "garage-rack-a"
expected_hashrate_ths = 9.5
api_port = 4028
web_port = 80
enabled = true
```

Current fleet mode checks API and web-port reachability, reports online/offline status, and totals reachable expected TH/s. This is the base layer for vendor adapters that can read real hashrate, chip temps, fan RPM, pool URL, rejected shares, and uptime.

## Optimization

For raw CPU hashrate, start here:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
./target/release/kaspa-miner --benchmark --threads 4 --batch-size 4096 --bench-seconds 15
./target/release/kaspa-miner --benchmark --threads 8 --batch-size 4096 --bench-seconds 15
./target/release/kaspa-miner --benchmark --threads 8 --batch-size 16384 --bench-seconds 15
```

Tune in this order:

- `threads`: usually physical performance cores first; too many threads can reduce hashrate through cache pressure and scheduling overhead.
- `batch_size`: higher values reduce bookkeeping overhead, lower values react faster to fresh 10 BPS work. `4096` is the default balance.
- Power profile: run plugged in, disable low-power modes, and keep thermals under control so clocks do not collapse.
- Pool distance: choose a low-latency pool endpoint; stale shares erase any local hashrate win.
- Plain logs: use `--no-tui` for long-running production processes.

## Protocol Notes

KASPilot expects the common two-parameter Kaspa notify payload:

```text
mining.notify params: ["jobId", "headerHash"]
```

The `headerHash` is parsed as 40 bytes:

```text
32 bytes pre_pow_hash || 8 bytes timestamp_le
```

The miner submits:

```text
mining.submit params: ["wallet.worker", "jobId", "nonce"]
```

The nonce is the full 8-byte hex nonce, including any pool-provided extranonce prefix.

## Production Checklist

- Use a trusted pool endpoint and confirm it supports Kaspa Common Stratum.
- Keep your wallet address out of shell history by using `config.toml` for regular operation.
- Pin `threads` below total CPU capacity if the machine needs to stay responsive.
- Use `--no-tui` under process managers such as `systemd`, `launchd`, Docker, or tmux logs.
- Prefer release builds; debug builds are not representative for mining performance.
- Monitor rejected shares. Repeated `Low difficulty share`, `DuplicateShare`, or `JobNotFound` errors usually indicate protocol, stale-work, or clock/pool issues.

## References

- Kaspa Common Stratum Protocol: https://file1.iceriver.io/protocols/KAS-Miner-Mining-Protocol-EN.pdf
- Rusty Kaspa PoW primitives: https://docs.rs/kaspa-pow
- Kaspa hash primitives: https://docs.rs/kaspa-hashes

## License

No license file is included yet. Add one before distributing binaries.
