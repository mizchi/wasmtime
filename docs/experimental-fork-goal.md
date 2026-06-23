# Experimental Fork Goal: shared-everything threads for vibe

Status: fork-local experiment
Owner: mizchi fork
Date: 2026-06-02

This document tracks the local `mizchi/wasmtime` fork goal for enabling enough
of WebAssembly shared-everything threads and Component Model threading to unblock
`vibe-lang` experiments.

This is not an upstream contribution plan. Do not open pull requests, comment on
issues, or review upstream Wasmtime pull requests from this fork experiment.
Follow the Bytecode Alliance AI Tool Use Policy for all Wasmtime work.

## Goals

- Keep Wasmtime buildable locally with experimental flags enabled.
- Let `vibe-lang/src/x/threads` run probes against the local fork.
- Validate the current shared-everything subset that is already useful:
  - `(ref (shared i31))`
  - `(ref null (shared any))`
  - `ref.i31_shared`
  - `i31.get_u` / `i31.get_s`
- Add the first usable Component Model `thread.spawn-*` path:
  - `canon thread.spawn-indirect`
  - `canon thread.spawn-ref`
  - `canon thread.available-parallelism`
- Keep true `shared=true` preemptive parallel execution behind the fork-local
  unsafe opt-in until the Component Model thread table and shared state model are
  defined.
- For the current unsafe Component Model path, allow fixed-size shared
  start-dispatch tables and the limited imported runtime start-table growth
  shape only for table-only owner modules; reject direct defined growable
  shared-table starts, growable table owners with functions, and unrelated
  growable shared tables until inline VMContext table definitions have a
  cross-store ownership model.
- Treat imported shared globals as the supported mutable diagnostic shape.
  Direct immutable defined shared-global reads are allowed through the copied
  child initial value; direct mutable defined shared-global starts flush child
  owner values back to parent definitions after the start function returns
  until direct defined-global storage is live-shared across sibling stores.
- Reject Component Model resources and Component Model GC canonical options on
  the unsafe OS-thread path until resource tables, host handles, destructors,
  borrows, GC heaps, and roots have a cross-store ownership model.
- Keep the proposal conformance boundary explicit before wiring Vibe:
  `docs/experimental-shared-everything-conformance.md` classifies which parts
  are proposal-defined, proposal-aligned subsets, fork-local diagnostics, or
  gaps.

## Non-goals

- Do not implement the entire shared-everything-threads proposal in one step.
- Do not make GC heap object sharing sound in the first milestone.
- Do not rely on this fork as an upstream-compatible Wasmtime API.
- Do not use the deprecated WASI Threads path as the final Component Model
  implementation, though it remains useful as a speedup baseline in `vibe`.

## Current local baseline

The fork already has local work around these areas:

- CLI flag plumbing for `shared-everything-threads`.
- WAST feature support for the shared-everything flag.
- Shared i31 parsing / validation probes.
- A Component Model threading probe using existing `canon thread.new-indirect`.

Wasmtime already contains a cooperative Component Model threading runtime:

- `canon thread.new-indirect`
- `canon thread.suspend*`
- `canon thread.unsuspend`
- `canon thread.index`

The shortest path for `thread.spawn-indirect` is to reuse that machinery.

## Vibe semantic contract

The fork must target the `vibe-lang/src/x/threads/THREADING_CONTRACT.md`
abstraction. This prevents the same source-level Vibe API from changing meaning
as the Wasmtime fork moves through partial implementations.

Vibe backends map as follows:

| Vibe backend | Wasmtime fork meaning |
| --- | --- |
| `SerialOnly` | no spawn |
| `WasiThreads` | WASI `thread-spawn`; real host-thread baseline |
| `ComponentModelCooperative` | Component Model cooperative scheduler |
| `ComponentModelShared` | true shared-everything host-thread execution |

The key rule is that `ComponentModelCooperative` must not be reported as a
parallel speedup backend. A fork-local `thread.spawn-indirect` that only fuses
`thread.new-indirect` and resume is still cooperative. It becomes
`ComponentModelShared` only when `shared=true` actually runs work concurrently on
host threads with a sound shared state model.

## Milestones

### M0: Red tests

Add failing WAST probes before implementation.

Expected probes:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect.wast`
- `tests/misc_testsuite/component-model-threading/thread-available-parallelism.wast`

The `thread.spawn-indirect` probe should:

- define a core start function `(param i32) -> ()`
- place it in a funcref table
- expose `(canon thread.spawn-indirect $start-func-ty (table $table))`
- call the generated core function from a component
- assert that the thread runs and updates observable component state

The first Red failure is expected to come from Wasmtime translation rejecting
`wasmparser::CanonicalFunction::ThreadSpawnIndirect`.

### M1: Translate `thread.spawn-indirect`

Add a new trampoline through the Component Model translation pipeline.

Expected files:

- `crates/environ/src/component/translate.rs`
- `crates/environ/src/component/translate/inline.rs`
- `crates/environ/src/component/dfg.rs`
- `crates/environ/src/component/info.rs`
- `crates/cranelift/src/compiler/component.rs`

Sketch:

- Add `LocalInitializer::ThreadSpawnIndirect`.
- Add `dfg::Trampoline::ThreadSpawnIndirect`.
- Add `info::Trampoline::ThreadSpawnIndirect`.
- Lower it the same way as `ThreadNewIndirect`, carrying:
  - component instance
  - start function type index
  - runtime table index
  - `shared` flag if exposed by the parser version
- Emit a new libcall trampoline in Cranelift.

The first implementation may accept only `shared=false` or treat `shared=true`
as an experimental alias for cooperative execution. If this shortcut is used,
the WAST and documentation must say so explicitly.

### M2: Runtime `thread_spawn_indirect`

Add a runtime libcall that fuses:

1. `thread_new_indirect(...)`
2. `resume_thread(...)`
3. return the created transient thread-table index

Expected files:

- `crates/environ/src/component.rs`
- `crates/wasmtime/src/runtime/vm/component/libcalls.rs`
- `crates/wasmtime/src/runtime/component/concurrent.rs`

Sketch:

```text
thread_spawn_indirect(caller, func_ty, table, func_idx, context) -> thread_idx
  thread_idx = thread_new_indirect(caller, func_ty, table, func_idx, context)
  resume_thread(caller, thread_idx, high_priority = true, allow_ready = false)
  return thread_idx
```

This should match the spec intent that `thread.spawn-indirect` is a fused
`thread.new-indirect` plus `thread.resume-later` operation. In the first fork
implementation this remains cooperative Component Model scheduling, not OS
thread parallelism. In Vibe terms this still maps to
`ComponentModelCooperative`.

### M3: `thread.available-parallelism`

Add a small runtime intrinsic.

Initial behavior:

- return `1` while `thread.spawn-*` is implemented by the cooperative Component
  Model event loop
- return `std::thread::available_parallelism()` only after the
  shared/preemptive path actually runs Component Model threads on host threads

Expected files mirror M1/M2:

- translation trampoline
- info / DFG trampoline
- Cranelift libcall
- runtime libcall

This gives `vibe` a way to size workloads without depending on WASI Threads.

### M4: `thread.spawn-ref`

Implemented for the current fork-local subset.

- `canon thread.spawn-ref $ft` translates through a dedicated component
  trampoline and libcall
- Wasmtime's internal initializer/trampoline/libcall path carries a
  shared/preemptive flag and only takes the unsafe OS-thread backend when that
  flag is set; the current `wasmparser`/`wast` surface still lacks the actual
  Component Model `shared?` immediate, so the legacy local parser output is
  treated as shared internally
- the direct function reference is passed as a `VMFuncRef` and checked against
  the current `(i32) -> ()` start-function shape
- normal mode creates a cooperative component thread and resumes it immediately
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` reuses the sibling-Store
  OS-thread backend without a start table
- concrete shared function references are accepted in the core type converter;
  shared arrays/structs/exceptions remain unsupported

Covered by:

- `tests/misc_testsuite/component-model-threading/thread-spawn-ref.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-ref-preemptive-smoke.wast`

This is lower priority because `vibe` can target table-based dispatch first.

### M5: true shared/preemptive execution

This is the hard part and should be separated from the initial probe work.

Current status: normal mode is still cooperative, and a fork-local unsafe opt-in
exists for validating the OS-thread execution shape:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-preemptive-smoke.wast`

The probe expects a spawned shared thread to mutate shared memory while the
parent is blocked in `memory.atomic.wait32`. Without unsafe opt-in the current
implementation returns `0`, not `1`, because the spawned guest call remains
queued on the per-`Store` event loop.

Required investigation:

- Store concurrency and reentrancy invariants
- Component instance state mutation across host threads
- GC roots and shared GC heap object safety
- Component Model resource tables, destructors, borrows, and host handles
- cancellation and trap propagation across real threads
- table and resource handle synchronization

Current scaffold:

- `ComponentThreadTemplate` captures the parent `InstancePre` and the component
  runtime memory/table slots visible through `VMComponentContext`.
- `ComponentThreadTemplate::validate_rebindable_runtime_state` rejects normal
  `Export::Memory` runtime slots and all runtime table slots before a future
  OS-thread path can use the unsafe rebind mechanism.
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` enables a fork-local diagnostic
  path that creates a sibling store using a per-thread store-data factory,
  instantiates a sibling component, rebinds child shared core-memory
  VMContext/import slots to the parent shared memory allocation, rebinds child
  shared core-table VMContext/import slots plus shared runtime table slots to
  the parent table import, copies shared global initial values into sibling
  defined-global slots, rebinds matching child imported-global slots to parent
  shared global definitions, and calls the start function on a host OS thread.
- The spawned call now allocates a real parent component thread-table entry, but
  that entry points at an OS-owned lifecycle placeholder. Child setup failure, start
  failure, panic, and successful completion are recorded internally, and
  cooperative resume/suspend builtins reject the OS-owned index deterministically.
  The parent event loop now observes terminal completion records, joins
  terminal host threads, removes the OS-owned placeholder from the parent thread
  table, and surfaces setup/start/panic failures during cleanup.
  Fork-local embedder hooks can also request cancellation, query status, or
  poll-consume or block-consume terminal completion for unsafe OS-owned indices
  without relying on shared-memory polling. The companion completion-report
  hooks preserve child setup/start/panic failures as host diagnostic `Failed`
  reports with messages. Vibe-level join/completion must instead be implemented
  by the language runtime with shared state plus wait/notify, normally through a
  spawn trampoline. Real Wasm traps in child code stay in the host diagnostic
  failure channel and are not synthesized into Vibe trampoline terminal status
  values. Consuming diagnostics remove terminal public indices from
  lookup immediately, but returned numeric index values are still component
  thread-table entries and must not be treated as durable
  identities after cleanup because the same number can later resolve to a new
  OS-owned thread. Public unsafe-index lookup ignores collisions with non-OS
  cooperative thread indices, while multiple matching OS-owned runtime component
  instances are still rejected as ambiguous.
  `subtask.cancel` reaches OS-owned completion records as a best-effort request,
  and can interrupt already-running child Wasm at an epoch check when epoch
  interruption is enabled. It can also wake a child blocked in
  `memory.atomic.wait32/64` on rebound shared memory and record the
  cancel-caused interrupt as `Cancelled`. Parent task lifetime accounting waits
  for OS-owned placeholders to be cleaned up before the task becomes
  uninteresting. Shared memory wait/notify queues are shared across parent and
  OS-owned child threads for the rebound shared-memory shape, table entry
  updates are visible to table-based start dispatch, imported shared globals
  can observe parent definitions, direct mutable defined shared-global starts
  flush back after return, and direct immutable defined shared-global start
  functions can read the copied initial value. Component Model resources and GC
  canonical options are detected and rejected before unsafe OS-thread execution.
  Direct defined table growth/synchronization, live direct mutable
  defined-global sharing, arbitrary host-blocked cancellation,
  upstream canonical guest-visible joins, and general index operations remain
  incomplete.
- OS-owned child Stores now install a Store-local current guest thread while the
  start function runs, with an `instance_rep` that mirrors the parent-visible
  transient thread-table index. The fork-local shared `canon thread.index` compatibility path
  lets shared start functions observe that index in the current diagnostic
  shape.
- This means the fork currently has the first executable shape of the safe
  boundary, but not yet a general preemptive Component Model execution path.

Until this milestone exists, `thread.spawn-indirect shared` must not be claimed
as true shared-everything preemptive parallelism and must not be mapped to
Vibe's `ComponentModelShared` backend.

See `docs/experimental-preemptive-threading.md` for the current implementation
shape and safe architecture options.

## Validation from vibe

`vibe-lang` should keep three independent probes:

- shared-everything core WAST probe
- Component Model threading WAST probe
- WASI Threads speedup baseline

The WASI Threads speedup probe is not the target implementation. It exists to
prove that the chosen workload shape can speed up when real parallelism is
available. Once this fork has a true shared/preemptive `thread.spawn-*` path,
reuse the same workload shape to compare:

- serial
- WASI Threads parallel baseline
- Component Model `thread.spawn-indirect` cooperative
- Component Model `thread.spawn-indirect shared` preemptive

Current baseline:

- `tests/all/cli_tests/wasi_threads_speedup.wat`
- `tests/all/cli_tests.rs::run_wasi_threads_speedup_probe`
- `docs/experimental-wasi-threads-speedup.md`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-parallel.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-bidirectional-wait-notify.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-cancel-wakeup.wast`
- `tests/all/cli_tests.rs::run_component_thread_speedup_probe`
- `tests/all/cli_tests.rs::run_component_thread_vibe_abi_speedup_probe`
- `docs/experimental-component-thread-speedup.md`
- `docs/experimental-vibe-thread-contract.md`

On 2026-06-01, the local debug CLI run measured `serial` at 0.20s real time and
`parallel` at 0.07s real time for the same checksum. This demonstrates true
host-thread speedup through WASI Threads, independent of the Component Model
threading implementation.

The unsafe Component Model OS-thread diagnostic now reuses the same validation
shape. On 2026-06-01, direct local CLI timing with `available_parallelism=10`
measured the Component Model serial WAST at 0.23s real time and the unsafe
`thread.spawn-indirect` parallel WAST at 0.07s real time for checksum
`1106140682`.

This is useful evidence for Vibe's workload shape, but it is still a fork-local
diagnostic. It must not be treated as the final safe `ComponentModelShared`
backend until join, cancellation, shared-object ownership, and interruption
semantics are tightened.

The current validator now exposes a fork-local positive Vibe ownership subset
for the unsafe path and rejects growable shared tables outside the imported
runtime start-table shape.

The trampoline probes now include a consolidated Vibe runtime ABI shape that
uses producer-owned shared slots for join, terminal status, failed-as-value,
in-flight cancellation, and result aggregation.

The speedup probes now also include an ABI-shaped serial/parallel pair using
the same slot layout; direct local timing measured about `2.75x` wall-clock
speedup on 2026-06-02.

For Vibe, the current fork-local backend name should be
`ComponentModelUnsafeOsThreads`. `ComponentModelShared` remains reserved for a
future implementation with a stronger semantic contract.

## Useful commands

From `mizchi/wasmtime`:

```bash
cargo test -p wasmtime-environ component
cargo test -p wasmtime-cranelift component
cargo test -p wasmtime-wast
```

From `mizchi/vibe-lang`:

```bash
pkf run experimental_shared_everything_threads_probe
pkf run experimental_component_model_threading_probe
pkf run experimental_wasi_threads_speedup_probe
```

## References

- WebAssembly shared-everything threads:
  <https://github.com/WebAssembly/shared-everything-threads>
- Proposal/fork-local conformance map:
  <./experimental-shared-everything-conformance.md>
- Component Model explainer, `thread.spawn-*`:
  <https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md>
- Component Model Canonical ABI:
  <https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md>
- Component Model binary format:
  <https://github.com/WebAssembly/component-model/blob/main/design/mvp/Binary.md>
