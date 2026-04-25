# KASPilot

```text
 _  __    _    ____  ____  _ _       _
| |/ /   / \  / ___||  _ \(_) | ___ | |_
| ' /   / _ \ \___ \| |_) | | |/ _ \| __|
| . \  / ___ \ ___) |  __/| | | (_) | |_
|_|\_\/_/   \_\____/|_|   |_|_|\___/ \__|

KASPA OPS TERMINAL / ASIC FLEET CONTROL / CPU DEV MINER
```

**KASPilot** is a high-signal Kaspa operations console for serious mining setups: ASIC fleet visibility, CGMiner-compatible telemetry, Kaspa Common Stratum protocol tooling, and a CPU kHeavyHash benchmark/dev miner for validation work.

It is intentionally Kaspa-only. The production lane is ASIC fleet control. The CPU lane is for development, pool testing, benchmarking, and protocol validation.

> Profitable Kaspa mining is ASIC territory. KASPilot keeps the CPU miner polished, but positions it honestly as a dev/benchmark tool.

## Tags

`kaspa` `kheavyhash` `asic-mining` `stratum` `cgminer-api` `fleet-monitoring` `cpu-benchmark` `rust` `tui` `mining-ops`

## Core Modes

| Mode | Command | Purpose |
| --- | --- | --- |
| ASIC fleet controller | `--fleet` | Monitor ASIC reachability, live TH/s, temp, fan, uptime, pool, accepted/rejected shares |
| CPU benchmark | `--benchmark` | Measure local kHeavyHash throughput without a pool |
| CPU autotune | `--tune` | Sweep thread and batch settings, then recommend the best config |
| CPU dev miner | default / `--no-tui` | Kaspa Common Stratum CPU mining for dev and validation |

## Capabilities

| Layer | Status |
| --- | --- |
| Coin target | Kaspa only |
| ASIC telemetry | CGMiner-compatible `summary`, `pools`, `devs`, `stats` over TCP |
| Fleet inventory | TOML device registry with model, location, API/web ports, expected TH/s |
| Hash algorithm | kHeavyHash PoW path using `kaspa-hashes` primitives |
| Pool protocol | Kaspa Common Stratum over TCP |
| Job format | Compact `mining.notify` header parsing |
| Difficulty | Pool share target from `mining.set_difficulty` |
| Nonce space | Extranonce prefix plus per-thread nonce scanning |
| Runtime | Multi-threaded CPU workers with non-overlapping nonce stride |
| Interface | Ratatui dashboard, plain logs, fleet report, benchmark report |
| Optimization | Release LTO, native CPU build support, configurable batch size, autotune matrix |

## Quick Start

```sh
cargo build --release
cp config.example.toml config.toml
cp fleet.example.toml fleet.toml
```

For best local CPU benchmark numbers:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

## ASIC Fleet

Edit `fleet.toml`:

```toml
poll_secs = 30
timeout_ms = 750

[[devices]]
name = "kas-rig-01"
host = "192.168.1.101"
model = "KS-series"
location = "rack-a"
expected_hashrate_ths = 9.5
api_port = 4028
web_port = 80
enabled = true
```

Poll once:

```sh
./target/release/kaspa-miner --fleet --fleet-once
```

Run continuously:

```sh
./target/release/kaspa-miner --fleet
```

If an ASIC exposes a CGMiner-compatible API on `api_port`, KASPilot reads normalized live TH/s, average TH/s, temperature, fan RPM, uptime, pool URL, accepted shares, and rejected shares. If a unit only exposes a web UI, leave `api_port` unset and use `web_port` for reachability.

## CPU Dev Miner

Edit `config.toml`:

```toml
pool = "stratum+tcp://pool.example.com:5555"
wallet = "kaspa:your_wallet_address"
worker = "rig-01"
threads = 8
batch_size = 4096
reconnect_secs = 5
```

Dashboard:

```sh
./target/release/kaspa-miner
```

Plain logs:

```sh
./target/release/kaspa-miner --no-tui
```

Override config from CLI:

```sh
./target/release/kaspa-miner \
  --pool stratum+tcp://pool.example.com:5555 \
  --wallet kaspa:your_wallet_address \
  --worker rig-01 \
  --threads 8 \
  --batch-size 4096 \
  --no-tui
```

## Benchmark And Autotune

Single benchmark:

```sh
./target/release/kaspa-miner --benchmark --threads 8 --batch-size 4096 --bench-seconds 15
```

Autotune:

```sh
./target/release/kaspa-miner --tune --tune-max-threads 8 --tune-seconds 5
```

Custom batch sweep:

```sh
./target/release/kaspa-miner \
  --tune \
  --tune-max-threads 12 \
  --tune-batches 1024,4096,16384,65536 \
  --tune-seconds 8
```

Tuning priorities:

- `threads`: start with physical performance cores; too many threads can reduce hashrate through scheduling and cache pressure.
- `batch_size`: higher values reduce bookkeeping overhead, lower values check pool work more often for 10 BPS-era freshness.
- Thermals: keep clocks stable; throttling erases hashrate.
- Pool latency: use a nearby pool endpoint to reduce stale shares.
- Production logs: use `--no-tui` under `systemd`, `launchd`, Docker, or tmux.

## CLI Reference

```text
--config <PATH>       CPU miner config path, default: config.toml
--pool <URL>          stratum+tcp://host:port
--wallet <ADDRESS>    kaspa: or kaspatest: address
--worker <NAME>       Worker name appended to wallet for pool login
--threads <N>         CPU worker threads
--batch-size <N>      Nonces per thread before checking pool work
--no-tui              Plain terminal logs
--benchmark           Offline kHeavyHash benchmark
--bench-seconds <N>   Benchmark duration
--tune                Sweep CPU settings and rank local hashrate
--tune-seconds <N>    Per-test duration for --tune
--tune-max-threads N  Maximum thread count for --tune
--tune-batches LIST   Comma-separated batch sizes for --tune
--fleet               ASIC fleet controller mode
--fleet-config <PATH> Fleet config path, default: fleet.toml
--fleet-once          Poll fleet once and exit
```

## Kaspa Stratum Notes

KASPilot expects the common two-parameter Kaspa notify payload:

```text
mining.notify params: ["jobId", "headerHash"]
```

The `headerHash` is parsed as:

```text
32 bytes pre_pow_hash || 8 bytes timestamp_le
```

The miner submits:

```text
mining.submit params: ["wallet.worker", "jobId", "nonce"]
```

The nonce is the full 8-byte hex nonce, including any pool-provided extranonce prefix.

## Production Checklist

- Confirm your pool supports Kaspa Common Stratum.
- Keep wallet addresses in `config.toml` instead of shell history.
- Keep ASIC management ports on a trusted LAN or VPN.
- Do not expose CGMiner API ports directly to the public internet.
- Use `--fleet` for ASIC operations and CPU mode for protocol validation.
- Use release builds for benchmarks and production binaries.
- Watch rejected shares. Repeated low-difficulty, duplicate, or stale share errors usually indicate pool/protocol/latency issues.

## Roadmap

- Vendor-specific ASIC adapters for richer IceRiver/Bitmain-style telemetry.
- JSON/CSV export for fleet dashboards.
- Alert thresholds for offline rigs, high temperature, fan faults, and reject spikes.
- Pool failover view and worker grouping.
- Optional local web dashboard.

## References

- Kaspa Common Stratum Protocol: https://file1.iceriver.io/protocols/KAS-Miner-Mining-Protocol-EN.pdf
- Rusty Kaspa PoW primitives: https://docs.rs/kaspa-pow
- Kaspa hash primitives: https://docs.rs/kaspa-hashes
- CGMiner-compatible API pattern: https://docs.luxor88.com/firmware/api/cgminer/summary

## License

`Cargo.toml` declares `MIT OR Apache-2.0`. Add matching license files before public distribution.
