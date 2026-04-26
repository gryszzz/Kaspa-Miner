# KASPilot

[![CI](https://github.com/gryszzz/KasPilot/actions/workflows/ci.yml/badge.svg)](https://github.com/gryszzz/KasPilot/actions/workflows/ci.yml)
[![Release](https://github.com/gryszzz/KasPilot/actions/workflows/release.yml/badge.svg)](https://github.com/gryszzz/KasPilot/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/gryszzz/KasPilot?label=release)](https://github.com/gryszzz/KasPilot/releases/latest)
[![Rust](https://img.shields.io/badge/rust-2021-111827)](https://www.rust-lang.org/)
[![Kaspa](https://img.shields.io/badge/kaspa-only-70c7ba)](https://kaspa.org/)

```text
 _  __    _    ____  ____  _ _       _
| |/ /   / \  / ___||  _ \(_) | ___ | |_
| ' /   / _ \ \___ \| |_) | | |/ _ \| __|
| . \  / ___ \ ___) |  __/| | | (_) | |_
|_|\_\/_/   \_\____/|_|   |_|_|\___/ \__|

KASPA OPS TERMINAL  ::  ASIC FLEET CONTROL  ::  CPU DEV MINER
```

**KASPilot** is a Kaspa-only mining operations terminal built for ASIC fleet visibility, pool validation, and local kHeavyHash benchmarking. It gives you a production-facing fleet controller and a polished CPU dev miner in one Rust binary.

The production lane is ASIC operations. The CPU lane is for benchmarking, pool testing, stratum validation, and development work.


## UI Preview

<p align="center">
  <img src="docs/assets/kaspilot-ui-preview.svg" alt="CLI-rendered KASPilot real-time mining cockpit preview" width="100%">
</p>

## Signal

`kaspa` `kheavyhash` `asic-mining` `stratum` `stratum-ssl` `cgminer-api` `fleet-monitoring` `cpu-benchmark` `autotune` `rust` `tui` `mining-ops`

## Command Deck

| Mission | Command | What it does |
| --- | --- | --- |
| Fleet control | `kaspa-miner --fleet` | Monitor ASIC reachability, TH/s, temp, fan, uptime, pool, accepted/rejected shares |
| One-shot audit | `kaspa-miner --fleet --fleet-once` | Poll every configured ASIC once and exit |
| CPU benchmark | `kaspa-miner --benchmark` | Measure local kHeavyHash throughput without touching a pool |
| CPU autotune | `kaspa-miner --tune` | Sweep thread and batch settings, then rank the fastest configs |
| Dev mining | `kaspa-miner` | Run the Kaspa Common Stratum CPU dev miner with TUI |
| Plain logs | `kaspa-miner --no-tui` | Run without dashboard rendering for tmux, services, and logs |

## What It Has

| Layer | Capability |
| --- | --- |
| Coin target | Kaspa only |
| Fleet telemetry | CGMiner-compatible `summary`, `pools`, `devs`, `stats` over TCP |
| ASIC inventory | TOML registry with model, location, API/web ports, expected TH/s, enabled state |
| Pool transport | `stratum+tcp://`, `stratum://`, `tcp://`, `stratum+ssl://`, `ssl://` |
| Pool hardening | Connect timeout, TLS support, `TCP_NODELAY`, reconnect loop |
| Hash path | kHeavyHash PoW using `kaspa-hashes` primitives |
| Difficulty | Share target derived from `mining.set_difficulty` |
| Nonce scan | Extranonce support plus non-overlapping per-thread stride |
| Operator UI | Ratatui dashboard, plain logs, fleet report, benchmark report |
| Performance knobs | Release LTO, native CPU build option, configurable batch size, autotune matrix |
| Releases | Linux archive, signed Windows archives, notarized macOS universal installer |

## Download

Grab the latest binaries from:

https://github.com/gryszzz/KasPilot/releases/latest

Release assets:

- `kaspa-miner-macos-universal.pkg`
- `kaspa-miner-x86_64-unknown-linux-gnu.tar.gz`
- `kaspa-miner-x86_64-pc-windows-msvc.zip`
- `kaspa-miner-aarch64-pc-windows-msvc.zip`
- `SHA256SUMS.txt`

Each release archive includes the binary, `README.md`, `start-mining.toml`, `config.example.toml`, and `fleet.example.toml`. Windows archives also include a first-run installer that puts the binary on your user path. The macOS package installs starter configs to `/usr/local/share/kaspilot/`.

## First Run

macOS, recommended universal package:

```sh
sudo installer -pkg kaspa-miner-macos-universal.pkg -target /
kaspa-miner --version
cp /usr/local/share/kaspilot/start-mining.toml ./config.toml
```

Windows:

```powershell
.\install-windows.cmd
kaspa-miner --version
```

Public desktop releases are blocked unless Apple notarization and Windows Authenticode signing secrets are configured. See `docs/macos-no-warning-release.md` and `docs/windows-no-warning-release.md`.

If macOS says Apple could not verify `kaspa-miner` is free of malware, the file was not notarized or was not the notarized `.pkg` release asset. Use the signed `kaspa-miner-macos-universal.pkg` from the latest release after the Apple signing secrets are configured.

## Start Mining Config

Release packages include `start-mining.toml` as the dedicated Kaspa pool and wallet config.

```sh
cp start-mining.toml config.toml
$EDITOR config.toml
kaspa-miner --config config.toml
```

In the macOS installer package, the starter config is installed here:

```sh
cp /usr/local/share/kaspilot/start-mining.toml ./config.toml
```

## Install From Source

```sh
git clone https://github.com/gryszzz/KasPilot.git
cd KasPilot
cargo build --release
```

For best local benchmark numbers on the same machine:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

Create local configs:

```sh
cp start-mining.toml config.toml
cp fleet.example.toml fleet.toml
```

## ASIC Fleet Control

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

Run a single fleet scan:

```sh
./target/release/kaspa-miner --fleet --fleet-once
```

Run the controller continuously:

```sh
./target/release/kaspa-miner --fleet
```

If an ASIC exposes a CGMiner-compatible API on `api_port`, KASPilot normalizes live TH/s, average TH/s, temperature, fan RPM, uptime, pool URL, accepted shares, and rejected shares. If a unit only exposes a web UI, leave `api_port` unset and use `web_port` for reachability.

## CPU Dev Miner

Edit `config.toml` or the release-provided `start-mining.toml`:

```toml
pool = "stratum+ssl://YOUR_KASPA_POOL_HOST:5555"
wallet = "kaspa:your_wallet_address"
worker = "kaspa-rig-01"
threads = 8
batch_size = 4096
reconnect_secs = 5
```

Start the TUI:

```sh
./target/release/kaspa-miner
```

From a downloaded release, you can mine directly from the starter config after editing the wallet and pool:

```sh
kaspa-miner --config start-mining.toml
```

Use plain logs:

```sh
./target/release/kaspa-miner --no-tui
```

Override config from the command line:

```sh
./target/release/kaspa-miner \
  --pool stratum+ssl://pool.example.com:5555 \
  --wallet kaspa:your_wallet_address \
  --worker rig-01 \
  --threads 8 \
  --batch-size 4096 \
  --no-tui
```

Supported pool URL schemes:

- `stratum+tcp://host:port`
- `stratum://host:port`
- `tcp://host:port`
- `stratum+ssl://host:port`
- `ssl://host:port`

## Benchmark And Autotune

Single benchmark:

```sh
./target/release/kaspa-miner --benchmark --threads 8 --batch-size 4096 --bench-seconds 15
```

Autotune:

```sh
./target/release/kaspa-miner --tune --tune-max-threads 8 --tune-seconds 5
```

Custom sweep:

```sh
./target/release/kaspa-miner \
  --tune \
  --tune-max-threads 12 \
  --tune-batches 1024,4096,16384,65536 \
  --tune-seconds 8
```

Tuning rules:

- Start with physical performance cores.
- Increase `batch_size` to reduce bookkeeping overhead.
- Lower `batch_size` if you need faster work refresh on high-latency pools.
- Keep clocks stable. Thermal throttling eats hashrate.
- Use nearby pool endpoints to reduce stale shares.
- Use `--no-tui` for `systemd`, `launchd`, Docker, tmux, and log collectors.

## CLI Reference

```text
--config <PATH>       CPU miner config path, default: config.toml
--pool <URL>          stratum+tcp://host:port or stratum+ssl://host:port
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

## Stratum Notes

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
- Prefer `stratum+ssl://` when the pool supports TLS.
- Keep wallet addresses in `config.toml` or `start-mining.toml`, not shell history.
- Keep ASIC management ports on a trusted LAN or VPN.
- Do not expose CGMiner API ports to the public internet.
- Use `--fleet` for ASIC operations and CPU mode for protocol validation.
- Use release builds for benchmarks and production binaries.
- Use `RUSTFLAGS="-C target-cpu=native"` only when the binary will run on the build machine.
- Watch rejected shares. Repeated low-difficulty, duplicate, or stale share errors usually indicate pool, protocol, or latency issues.

## Release Flow

CI runs on Linux, macOS, and Windows. Release archives are produced whenever a `v*` tag is pushed.

```sh
git tag v0.1.1
git push origin v0.1.1
```

The release workflow builds:

- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `aarch64-pc-windows-msvc`
- `macos-universal.pkg`

## Roadmap

- Vendor-specific adapters for richer IceRiver and Bitmain-style telemetry.
- JSON and CSV fleet export.
- Alert thresholds for offline rigs, high temperature, fan faults, and reject spikes.
- Pool failover visibility and worker grouping.
- Optional local web dashboard.
- Apple notarization and Windows Authenticode signing.

## References

- Kaspa Common Stratum Protocol: https://file1.iceriver.io/protocols/KAS-Miner-Mining-Protocol-EN.pdf
- Rusty Kaspa PoW primitives: https://docs.rs/kaspa-pow
- Kaspa hash primitives: https://docs.rs/kaspa-hashes
- CGMiner-compatible API pattern: https://docs.luxor88.com/firmware/api/cgminer/summary

## License

`Cargo.toml` declares `MIT OR Apache-2.0`. Add matching license files before public distribution.
