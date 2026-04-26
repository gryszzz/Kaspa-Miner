# KASPilot GPU Mining Lane

KASPilot supports GPU mining as a managed engine lane. The CLI owns the Kaspa pool/wallet config and supervises an installed kHeavyHash GPU miner process with log prefixing and watchdog restarts.

This is intentionally different from claiming the CPU miner is a GPU miner. A native CUDA/OpenCL kHeavyHash backend needs dedicated kernels, per-vendor tuning, validation against the CPU reference path, and release packaging for driver/runtime differences.

## Why This Shape

Kaspa kHeavyHash is a Keccak, matrix, Keccak proof-of-work path. That maps well to parallel hardware, but current Kaspa mining is ASIC-dominant. GPUs can still be useful for learning, development, spare hardware, and managed rigs, while ASICs remain the production profitability lane.

## Files

```text
start-mining.toml  Kaspa pool/wallet/worker settings
gpu.example.toml   GPU engine command template
gpu.toml           Your local GPU engine config
```

## Run

```sh
cp start-mining.toml config.toml
cp gpu.example.toml gpu.toml
$EDITOR config.toml
$EDITOR gpu.toml
kaspa-miner --gpu --config config.toml --gpu-config gpu.toml
```

## Placeholders

`gpu.toml` supports these placeholders in args and env values:

```text
{pool}
{wallet}
{worker}
{login}
{devices}
```

`{login}` expands to `wallet.worker`.

## Native GPU Backend Plan

1. Add a `GpuBackend` trait that matches the CPU nonce-scanning contract.
2. Add CUDA and OpenCL kernel crates behind explicit features.
3. Validate every found nonce against the CPU `JobContext` before submit.
4. Add per-device benchmark/autotune for work size, blocks, intensity, and refresh cadence.
5. Add platform packaging checks for NVIDIA CUDA, AMD ROCm/OpenCL, and macOS Metal viability.

## References

- Kaspa features: https://kaspa.org/features/
- Rusty Kaspa reference implementation: https://github.com/kaspanet/rusty-kaspa
