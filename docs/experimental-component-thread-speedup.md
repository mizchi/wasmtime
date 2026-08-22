# Experimental Component Model thread speedup probe

Status: fork-local unsafe diagnostic
Date: 2026-06-01

This note records a legacy Vibe-shaped CPU-bound speedup probe for the
fork-local Component Model OS-thread path.

The current Vibe runtime does not consume this slot ABI; its production-shaped
backend uses independent Store/Instance/heap workers. The legacy fixture names
are retained as test history. This is not a general shared-everything Component
Model implementation. It only
validates that the current `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`
diagnostic path can run independent CPU work on host OS threads with shared
memory result aggregation.

## Probe

The probe WAST files are:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-parallel.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast`

Both use the same four-slot CPU workload:

- each slot runs a deterministic 50,000,000-iteration 32-bit LCG loop
- the serial probe runs the four slots sequentially in the parent component
  thread
- the parallel probe spawns four `thread.spawn-indirect` component threads, one
  slot per thread
- spawned threads store their slot result in shared memory, increment a shared
  done counter, and notify the parent with `memory.atomic.notify`

Both probes assert the same checksum:

```text
1106140682
```

The parallel probe also checks that `thread.available-parallelism` reports a
positive value. With the unsafe opt-in enabled, this fork reports
`std::thread::available_parallelism()`.

The ABI-shaped pair uses the same workload but stores completion through the
consolidated Vibe slot layout:

| Field | Offset |
| --- | --- |
| Slot stride | `32` bytes |
| `state` | `0` |
| `terminal_code` | `4` |
| `payload` | `8` |
| `input` | `16` |
| `cancel` | `20` |
| `mode` | `24` |
| `worker_func` | `28` |

The serial ABI-shaped probe writes the same slots from the parent thread. The
parallel ABI-shaped probe writes them from generated child trampolines and the
parent aggregates completed payloads from the slots.

## Commands

Build the CLI first, then run the direct WAST probes:

```bash
cargo +1.93.0 build --bin wasmtime

WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 \
  /usr/bin/time -p target/debug/wasmtime wast \
  -Ccache=n \
  -W threads=y \
  -W component-model=y \
  -W component-model-async=y \
  -W component-model-threading=y \
  -W gc=y \
  -W function-references=y \
  -W shared-everything-threads=y \
  tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-serial.wast

WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 \
  /usr/bin/time -p target/debug/wasmtime wast \
  -Ccache=n \
  -W threads=y \
  -W component-model=y \
  -W component-model-async=y \
  -W component-model-threading=y \
  -W gc=y \
  -W function-references=y \
  -W shared-everything-threads=y \
  tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-parallel.wast

/usr/bin/time -p target/debug/wasmtime wast \
  -Ccache=n \
  -W threads=y \
  -W component-model=y \
  -W component-model-async=y \
  -W component-model-threading=y \
  -W gc=y \
  -W function-references=y \
  -W shared-everything-threads=y \
  tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast

WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 \
  /usr/bin/time -p target/debug/wasmtime wast \
  -Ccache=n \
  -W threads=y \
  -W component-model=y \
  -W component-model-async=y \
  -W component-model-threading=y \
  -W gc=y \
  -W function-references=y \
  -W shared-everything-threads=y \
  tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast
```

There is also an ignored CLI test that runs the same WAST files and reports
`available_parallelism`, serial time, parallel time, and speedup:

```bash
cargo +1.93.0 test --test all run_component_thread_speedup_probe -- --ignored --nocapture
cargo +1.93.0 test --test all run_component_thread_vibe_abi_speedup_probe -- --ignored --nocapture
```

The test is ignored because it is a fork-local timing probe and requires the
unsafe opt-in path. It asserts only a conservative wall-clock speedup threshold.

## Local result

Observed on 2026-06-01 with `available_parallelism=10`:

| mode | real | user | sys |
| --- | ---: | ---: | ---: |
| serial | 0.23s | 0.22s | 0.02s |
| parallel | 0.07s | 0.23s | 0.01s |

Observed ABI-shaped probe on 2026-06-02 with `available_parallelism=10` using
direct local CLI timing:

| mode | real | user | sys |
| --- | ---: | ---: | ---: |
| ABI serial | 0.22s | 0.22s | 0.01s |
| ABI parallel | 0.08s | 0.23s | 0.01s |

The parallel run has lower wall-clock time and similar total user CPU time. This
is the expected signature that the work ran on host OS threads.

## Interpretation

This validates the engine's parallel workload shape; it does not validate the
current Vibe structured-concurrency contract:

- independent CPU work can be split into component-thread start functions
- shared memory can carry per-thread results and a done counter
- `thread.spawn-indirect` can produce wall-clock speedup on the unsafe fork path
- `thread-spawn-indirect-os-trampoline-completion.wast` pins the complementary
  language-runtime shape: drop the canonical spawn index and publish
  completion through trampoline-owned shared state plus wait/notify
- `thread-spawn-indirect-os-trampoline-status.wast` extends that shape with
  Vibe-level terminal codes for completed, cancelled, and failed-as-value
- `thread-spawn-indirect-os-trampoline-cancel-wakeup.wast` pins in-flight
  cooperative cancellation through a producer-owned shared cancel flag plus
  wait/notify
- `thread-spawn-indirect-os-trampoline-vibe-abi.wast` consolidates completion,
  terminal status, failure-as-value, in-flight cancellation, and aggregation
  into one Vibe-shaped slot ABI
- `thread-spawn-indirect-os-trampoline-vibe-abi-speedup-*.wast` confirms the
  same ABI slot layout still gives wall-clock speedup on the CPU workload
- `thread-spawn-indirect-os-trampoline-trap-boundary.wast` pins that real Wasm
  traps remain host diagnostic failures instead of synthesized Vibe terminal
  status values

The Vibe-facing contract for using this probe is documented in
`docs/experimental-vibe-thread-contract.md`.

It does not remove the remaining semantic gaps:

- cancellation can interrupt already-running child Wasm only when epoch
  interruption is enabled and the child reaches an epoch check; host-blocked
  calls are still outside this mechanism
- upstream canonical `thread.join` is absent; Vibe-level completion must be
  generated by a language/runtime trampoline using shared state plus
  wait/notify, while the fork keeps only host diagnostic completion hooks
- shared start functions can call the fork-local shared `canon thread.index`
  compatibility path in the current diagnostic shape
- imported runtime start-table growth is supported only for table-only owner
  modules in the current diagnostic shape, and direct mutable defined
  shared-global starts flush back after return; broader shared
  table/global/resource/GC ownership is still incomplete
- the safe `ComponentModelShared` Vibe backend should not be enabled from this
  diagnostic alone
