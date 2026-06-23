# Experimental WASI Threads speedup baseline

Status: fork-local baseline
Date: 2026-06-01

This note records the baseline used to prove that the selected CPU-bound
workload can speed up when Wasmtime uses real host threads.

This is not the target Component Model `thread.spawn-*` implementation. It is a
control case for Vibe and the `mizchi/wasmtime` fork.

## Probe

The probe module is:

- `tests/all/cli_tests/wasi_threads_speedup.wat`

It exports two functions:

- `serial`: runs four independent CPU-bound work chunks sequentially
- `parallel`: spawns four WASI threads, runs one chunk per thread, and joins
  them with `memory.atomic.wait32` / `memory.atomic.notify`

Both exports return the same checksum:

```text
6728552294601276938
```

The regular CLI test verifies this equality:

```bash
cargo +1.93.0 test --test all run_wasi_threads_speedup_probe --features wasi-threads
```

## Local timing

Commands used on 2026-06-01:

```bash
/usr/bin/time -p target/debug/wasmtime run \
  -Wthreads,shared-memory \
  -Sthreads \
  -Ccache=n \
  --invoke serial \
  tests/all/cli_tests/wasi_threads_speedup.wat

/usr/bin/time -p target/debug/wasmtime run \
  -Wthreads,shared-memory \
  -Sthreads \
  -Ccache=n \
  --invoke parallel \
  tests/all/cli_tests/wasi_threads_speedup.wat
```

Observed wall-clock result:

| mode | real | user | sys |
| --- | ---: | ---: | ---: |
| serial | 0.20s | 0.21s | 0.01s |
| parallel | 0.07s | 0.23s | 0.02s |

The `parallel` run has lower wall-clock time and similar total user CPU time,
which is the expected OS-thread parallelism signature.

## Interpretation

This confirms that the workload shape is suitable for demonstrating speedup
once Component Model shared/preemptive `thread.spawn-*` exists.

It does not change the current Component Model result:

- `thread.spawn-indirect` in this fork is still cooperative.
- The preemptive Component Model Red probe still fails with `actual 0`.
- `ComponentModelCooperative` must not be reported as a speedup backend in Vibe.
