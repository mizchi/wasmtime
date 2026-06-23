# Experimental Shared-Everything Threads Conformance Map

Status: fork-local classification
Date: 2026-06-05

This document separates the current `mizchi/wasmtime` thread experiment into:

- proposal-defined behavior from shared-everything threads and the Component
  Model Canonical ABI
- proposal-aligned but incomplete local implementation
- fork-local diagnostics and Vibe ABI choices
- known gaps that must not be exposed as `ComponentModelShared`

The proposal is still under active development, so this is a validation aid for
local Vibe integration, not an upstream Wasmtime conformance claim.

References:

- Shared-everything threads proposal overview:
  <https://github.com/WebAssembly/shared-everything-threads/blob/main/proposals/shared-everything-threads/Overview.md>
- Component Model Canonical ABI thread builtins:
  <https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md>
- Local Vibe backend contract:
  <./experimental-vibe-thread-contract.md>

## Terms

| Classification | Meaning |
| --- | --- |
| Proposal-defined | The feature exists in the current proposal or Canonical ABI text. |
| Proposal-aligned subset | The fork implements the same shape, but only for a constrained subset or through an implementation-specific trigger. |
| Fork-local | The behavior is a local Wasmtime fork diagnostic or Vibe runtime ABI, not a WebAssembly proposal contract. |
| Gap | The proposal has a concept that this fork does not implement yet, or the fork intentionally rejects it. |

## Summary

The fork currently has a useful `thread.spawn-indirect` OS-thread diagnostic
path, but it is not a full shared-everything implementation.

Vibe may use it as `ComponentModelUnsafeOsThreads` for local probes that match
the documented ABI shape. Vibe must not call it `ComponentModelShared` until the
missing shared state model, resource/GC model, and safe thread lifecycle
semantics exist.

## Conformance Matrix

| Area | Proposal status | Current fork status | Classification | Vibe dependency |
| --- | --- | --- | --- | --- |
| `shared` function types | Shared functions are part of the proposal. A shared function body must only access shared module fields. | Shared function types are parsed and used for `thread.spawn-indirect` and `thread.spawn-ref` start functions. The fork does not implement the full shared-function validation/runtime model for every shared object kind. | Proposal-aligned subset | Required for the unsafe backend start function. |
| Shared memories | Shared memories are already standardized by the threads proposal and are a building block for shared-everything threads. | The unsafe path rebinds child shared-memory VMContext/import slots to parent `SharedMemory`, including futex wait queues. | Proposal-aligned subset | Yes. This is the primary supported communication path. |
| `memory.atomic.wait*` / `memory.atomic.notify` | Existing linear-memory wait/notify is proposal-compatible synchronization. The shared-everything proposal also discusses managed waiter queues for WasmGC. | Linear-memory wait/notify works across rebound shared memory. Managed `waitqueue` instructions are not implemented. | Proposal-aligned subset for linear memory; gap for managed waitqueues | Yes, for linear-memory slots only. |
| Shared tables | The proposal adds `shared` tables whose element type must be valid as shared. | Fixed-size shared start tables are rebound. A single growable imported runtime start table shape is allowed. Direct defined growable table ownership, unrelated growable shared tables, and broad synchronized table mutation remain rejected. | Proposal-aligned subset with fork-local restrictions | Yes, only for start dispatch. |
| Shared globals | The proposal adds shared globals, including mutable shared globals and future atomic global accesses. | Imported shared globals can observe parent definitions. Direct immutable defined globals are copied into the child. Direct mutable defined globals use a fork-local post-start flush-back, not live shared storage. Global atomic instructions are not implemented. | Partial subset; mutable direct access is fork-local | Only use imported shared globals or documented flush-back diagnostics. |
| Shared structs/arrays/WasmGC | The proposal includes shared heap types and atomic struct/array/global/table instructions. | Shared structs, arrays, exceptions, resources, and Component Model GC canonical options are rejected or unsupported. | Gap | No. |
| Atomic table/global/struct/array instructions | The proposal lists new atomic instructions for shared fields, globals, and tables. | Cranelift still rejects shared-everything operators that are not implemented. | Gap | No. |
| `ref.i31_shared` and shared reference parsing probes | The proposal includes shared heap types and `ref.i31_shared`. | The fork has local validation probes for shared i31/any shapes useful to Vibe experimentation. | Proposal-aligned subset | Optional core proposal probe only. |
| `canon thread.spawn-indirect shared?` | Canonical ABI defines it. With `shared`, the spawned thread is preemptive and can execute in parallel. It returns a component thread-table index. | The fork translates/runs `thread.spawn-indirect`; normal mode is cooperative. With `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`, a constrained shared start-table/start-function shape runs on a sibling `Store` in an OS thread. The Wasmtime-internal trampoline/libcall path carries a `shared` flag and gates the unsafe backend on it, but the parser surface does not expose the actual `shared?` immediate yet, so legacy local parser output is marked `shared: true`. | Proposal-aligned entry point; fork-local execution trigger/backend | Yes, as `ComponentModelUnsafeOsThreads`, not `ComponentModelShared`. |
| `canon thread.spawn-ref shared?` | Canonical ABI defines it. With `shared`, the spawned direct function reference is preemptive and can execute in parallel. It returns a component thread-table index. | The fork translates/runs `thread.spawn-ref`; normal mode is cooperative. With `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`, a constrained direct shared start-function shape runs on a sibling `Store` in an OS thread. The Wasmtime-internal trampoline/libcall path carries a `shared` flag and gates the unsafe backend on it, but the parser surface does not expose the actual `shared?` immediate yet, so legacy local parser output is marked `shared: true`. | Proposal-aligned entry point; fork-local execution trigger/backend | Optional. Vibe still uses the trampoline/table ABI for worker dispatch. |
| `canon thread.available-parallelism shared?` | Canonical ABI defines it as a count the engine may allow to run in parallel. | Implemented. Normal cooperative mode reports `1`; unsafe OS-thread mode reports host available parallelism only for internally shared calls. The `shared?` immediate itself is not visible to this fork's parser surface, so legacy local parser output is marked `shared: true`. | Proposal-aligned subset with fork-local mode split | Use only for sizing/reporting, not as a safety proof. |
| `canon thread.index` | Canonical ABI defines `thread.index` for the current component thread-table index. The shared-everything proposal does not require TIDs. | Existing Component Model `thread.index` works. The fork adds a limited shared import compatibility path named exactly `thread.index` with type `(func (result i32))`, so shared start functions can observe the parent-visible transient index. | Canonical ABI existing builtin plus fork-local shared compatibility | Diagnostic only. Do not treat as a durable thread identity. |
| Spawn return value | Canonical ABI `spawn-*` returns a component thread-table index. | The unsafe path returns a parent placeholder index. Consuming diagnostics remove terminal OS-owned entries and numeric indices can be reused. | Proposal-shaped value with fork-local lifecycle | Vibe must ignore it for join/result semantics. |
| Guest-visible join | The shared-everything proposal FAQ says there is no language-level join; join can be implemented with wait/notify. | No canonical guest join is added. Vibe-level join is implemented by generated trampoline-owned shared slots plus wait/notify. Host APIs can observe/join for diagnostics only. | Fork-local/Vibe ABI for join; no proposal builtin | Yes, but only through generated shared-state ABI. |
| Thread completion result | Proposal thread start functions have no return value. Cleanup should be done by a trampoline if needed. Traps abort at the spec level. | Vibe ABI slots encode `completed`, `cancelled`, and `failed-as-value`. Real Wasm traps stay in host diagnostic failure records and are not synthesized into Vibe slots. | Vibe ABI/fork-local | Yes, as language runtime protocol. |
| Vibe worker dispatch | The proposal does not define Vibe's source-level worker naming or channel ABI. A shared start function can call shared functions if the module satisfies shared validation. | The Vibe prototype can compile a string-literal `Threads::spawn("worker", ch)` into a trampoline slot field that dispatches a non-exported capture-free top-level worker over the current shared-value subset and writes the full 64-bit tagged result value into the slot payload. | Fork-local/Vibe ABI over proposal-aligned shared functions | Diagnostic only; not a general function-value, closure, or arbitrary heap-object contract. |
| Vibe typed channel surface | The proposal leaves source-language channel types to the guest language/runtime. | The Vibe checker now represents channel handles as `ThreadChannel[T]`; `Threads::send` binds `T`, `Threads::recv` returns the same `T`, the handle remains the existing tagged `Int`, and shared channel payload cells use `i64.atomic.load/store` to preserve the full tagged value. | Vibe type-layer/runtime ABI contract | Yes for the current scalar, string, array, tuple, and scalar-field record probes. |
| Vibe typed task surface | The proposal exposes a component thread-table index, but does not define Vibe's source-level task/result abstraction. | The Vibe checker now represents task handles as `ThreadTask[T]`; `Threads::spawn("name", ch)` returns `ThreadTask[R]` for supported string-literal worker result types and `ThreadTask[Int]` for reserved diagnostic names, `Threads::wait(task)` returns `T`, and the runtime representation remains the existing tagged `Int` slot pointer. | Vibe type-layer/runtime ABI contract | Yes for preventing arbitrary `Int` values from becoming Vibe join handles. |
| Cancellation | Canonical ABI cooperative thread builtins have cancellation concepts. Shared-everything does not define a forced preemptive cancellation contract for `spawn-*`. | `subtask.cancel` can request fork-local OS-thread cancellation. Epoch interruption and atomic-wait interruption can classify cancel-caused interrupts as `Cancelled`. Vibe cancellation uses producer-owned shared cancel flags. | Fork-local diagnostics plus Vibe ABI | Use cooperative Vibe cancel flags for guest semantics. |
| OS-thread execution backend | Proposal says `shared` spawned threads are preemptive/parallel but does not prescribe Wasmtime internals. | Implemented only behind `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` by creating a sibling `Store`, instantiating a sibling component, and rebinding the positive shared ownership subset. | Fork-local implementation | Yes, for local performance probes only. |
| Component resources | Component Model resources need cross-store resource tables, destructors, borrows, and host handles. | Rejected before unsafe OS-thread execution. | Gap | No. |
| Component Model GC canonical options | Shared GC heaps/roots need a shared ownership model. | Rejected before unsafe OS-thread execution. | Gap | No. |
| Thread-local globals | The proposal includes `thread_local` globals for TLS. | Not implemented in this fork experiment. | Gap | No. |
| Managed waiter queues | The proposal includes `waitqueue` types/instructions for WasmGC synchronization. | Not implemented. | Gap | No. |
| Trap propagation across threads | Proposal FAQ says traps abort all threads; engines should abort quickly but timing is nondeterministic. | Child traps are host diagnostic failures in the unsafe path. They do not yet abort all sibling/parent guest execution as a proposal-level semantics. | Gap/fork-local diagnostic | No. |
| Benchmark evidence | Proposal has goals for task/data parallelism but no benchmark contract. | Local ABI-shaped speedup probes show about `2.75x` direct CLI wall-clock speedup on 2026-06-02 for four CPU chunks. | Fork-local evidence | Yes, as performance evidence only. |

## Vibe Integration Rule

Before connecting this to Vibe, treat the current fork as follows:

- Use `ComponentModelUnsafeOsThreads` only when the program matches the
  documented shared-memory slot ABI and the unsafe environment variable is set.
- Use shared memory plus linear-memory wait/notify as the only guest-visible
  join/result/cancel synchronization contract.
- Ignore `thread.spawn-indirect` return values for Vibe-level join or result
  identity.
- Treat `thread.index` as a transient diagnostic index, not a stable TID.
- Keep `ComponentModelShared` disabled.

### Current Vibe Smoke (2026-06-05)

The current `vibe-lang` `feat/thread` probe can now reach the unsafe
OS-thread path in this fork:

- the generated component validates with `wasm-tools validate --features all`
- the component contains `canon thread.spawn-indirect`, a shared start table,
  shared memory, and `__heap_ptr` as `(shared mut i32)`
- `wasmtime compile` succeeds with Component Model threading, core threads,
  shared memory, function references, GC, and shared-everything feature flags
  enabled
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-probe(0)' ...` returns `0`
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-probe(0)' ...` returns `168`, the current tagged Vibe `Int`
  representation of `42`
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-channel-probe(0)' ...` returns `168`; this worker reads the
  passed `ThreadChannel[Int]` handle with `Threads::recv(ch)`, adds `21`, and
  returns tagged `42` through the Vibe slot payload
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-string-channel-probe(0)' ...` returns `20`; this worker reads
  a `ThreadChannel[String]` payload with `Threads::recv(ch)`, computes
  `String::length("hello")`, and returns tagged `5` through the Vibe slot
  payload
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-string-result-probe(0)' ...` returns `20`; this worker reads
  a `ThreadChannel[String]` payload, returns that `String` through
  `ThreadTask[String]`, and the parent computes its length after `wait`
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-string-alloc-result-probe(0)' ...` returns `24`; this worker
  reads a `ThreadChannel[String]` payload, allocates a new `"hello!"` string in
  the OS-owned child, returns it through `ThreadTask[String]`, and the parent
  computes its length after `wait`
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-array-result-probe(0)' ...` returns `12`; this worker allocates
  `[1, 2, 3]` in the OS-owned child, returns it through
  `ThreadTask[Array[Int]]`, and the parent computes its length after `wait`
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-array-string-result-probe(0)' ...` returns `820`; this worker
  allocates `["red", "green"]` in the OS-owned child, returns it through
  `ThreadTask[Array[String]]`, and the parent reads the nested string pointer
  after `wait`
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-array-channel-probe(0)' ...` returns `1280`; this parent sends
  `[10, 20, 30]` through `ThreadChannel[Array[Int]]`, the OS-owned child reads
  the array header and element `1`, computes `320`, and returns that tagged
  `Int` through the slot payload
- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 wasmtime run --invoke
  'thread-worker-array-string-channel-probe(0)' ...` returns `820`; this parent
  sends `["red", "green"]` through `ThreadChannel[Array[String]]`, the
  OS-owned child reads the array header, follows the nested string pointer for
  element `1`, computes `205`, and returns that tagged `Int` through the slot
  payload
- the generated diagnostic allocation probes also run through the unsafe
  OS-thread path; the latest local run returned `8716288` for
  `thread-alloc-probe(0)` and a positive diagnostic checksum, `135923200`, for
  `thread-alloc-many-probe(0)`

This started as only a smoke of the ABI path. Vibe now routes the standard
linear-memory allocation helpers for this backend through a shared-memory
`i32.atomic.rmw.add` cursor, while keeping `__heap_ptr` as the proposal-shaped
shared mutable global needed by the current component shape. Allocations that
return Vibe heap object pointers are at least 4-byte aligned, matching the
current tagged pointer contract. Allocations that contain Vibe shared-thread
`i64.atomic.load/store` fields, such as channel cells and task-slot payloads,
use 8-byte aligned atomic bump reservations.

The allocation probes are fork-local Vibe diagnostics, not public language
semantics. They use `Threads::spawn("alloc-probe", ch)` to select a generated
trampoline mode that performs many 16-byte atomic bump reservations from
OS-owned child threads and reports a tagged checksum through the Vibe slot
payload.
This validates contention on the shared cursor from multiple sibling Stores,
but it does not yet prove arbitrary Vibe heap object semantics. The checksum is
not a stable semantic value.

The worker-function probe is also fork-local Vibe ABI. It keeps the worker
non-exported so the shared worker function is not lifted as a component export.
The current DCE rule treats only `Threads::spawn("name", ...)` string literals
as worker roots, and the current dispatcher accepts only capture-free top-level
`ThreadChannel[Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]] -> Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]`
functions. The reserved names `"noop"` and `"alloc-probe"` are the only
non-function task names; other unknown names are compile-time errors. The
checker now gives the
channel handle a
`ThreadChannel[T]` type so the payload type sent through a channel is the
payload type received from the same channel. The generated wasm still passes
the channel handle as the same tagged integer ABI value, but shared channel
payload cells now use `i64.atomic.load/store` so the payload itself is not
truncated through an `i32` cell. The codegen worker
resolver also checks the source worker parameter type, so a bare `(Int) -> Int`
worker is rejected even though it has the same current wasm representation.
Dynamic worker-name expressions are rejected rather than lowered to a no-worker
task, keeping the current ABI limited to explicit string-literal worker roots.
Tuple-returning workers are compiled as ordinary heap-tuple-returning `i64`
functions for this path; Vibe's multi-value tuple-return optimization is
disabled for thread workers because the trampoline publishes exactly one tagged
slot payload.
Record-returning workers are covered for scalar-field records as the same
single tagged heap-pointer payload. Record channel payloads are accepted by the
checker/codegen shared-value guard, can be published into the shared channel
cell, and can be received by a worker using a source-level
`ThreadChannel[{ score: Int, word: String }]` annotation.

That is still not a complete shared heap contract. The atomic cursor covers the
ordinary bump allocation helpers, and `cabi_realloc` now uses a CAS loop so
canonical ABI power-of-two alignment requests reserve aligned ranges from the
same cursor. Builder grow and bulk grow avoid their heap-tip in-place fast path
in this backend and instead allocate replacement storage through that atomic
cursor. Vibe now rejects `enable_rc=true` with the shared-thread backend,
because the current RC/free-list path does not have a shared-thread allocator
contract. The JS Preview2 host runner also refuses the legacy `__heap_ptr`
allocation fallback for shared memory and requires exported `cabi_realloc`.
Other embedder-specific host allocation helpers are outside the current
contract unless they use the same shared allocator protocol.

The Vibe ABI may depend on:

- shared `(func (param i32))` start functions
- shared `(ref null (shared func))` start tables in the documented subset
- rebound shared core memory
- linear-memory atomics and wait/notify
- generated trampoline slots with this fixed layout:

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

Vibe-generated code stores `payload` as an 8-byte tagged Vibe value; the
remaining fields are i32-sized control fields.

The Vibe ABI must not depend on:

- `thread.spawn-ref` as the join/result protocol
- canonical guest join
- durable thread IDs
- full direct shared table/global ownership
- shared resources or shared Component Model GC
- forced bounded-time cancellation
- child traps becoming Vibe terminal status values

The parser/tooling surface still cannot expose the proposal's `shared?`
immediate. This fork intentionally avoids a reader-only heuristic because the
proposal binary shape places `shared?` before the function type index, while the
legacy local binary shape starts with the function type index. Legacy type
indices `0` and `1` are therefore byte-ambiguous with `shared? = false/true`.
Real support needs a coordinated wasm-tools update across `wasmparser`, `wast`,
`wasm-encoder`, and `wasmprinter`.

## Probe Checklist

| Contract point | Probe |
| --- | --- |
| Shared-memory OS-thread execution | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-preemptive-smoke.wast` |
| Direct shared function reference OS-thread execution | `tests/misc_testsuite/component-model-threading/thread-spawn-ref-preemptive-smoke.wast` |
| Shared-memory wait/notify | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-bidirectional-wait-notify.wast` |
| Shared start table dispatch | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-table-update.wast` |
| Imported growable start table subset | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-table-grow.wast` |
| Direct defined growable table rejection | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-growable-table-owner-func-rejected.wast` |
| Imported shared global visibility | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-global-update.wast` |
| Direct defined shared-global diagnostic shape | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-defined-mutable-shared-global.wast`, `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-defined-immutable-shared-global.wast` |
| Host diagnostic completion boundary | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-trap-boundary.wast` |
| Vibe ABI slots | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi.wast` |
| Vibe context pointer distinct from canonical spawn handle | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-context-pointer-not-handle.wast` |
| Vibe full 64-bit shared-value payload path | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-value-payload.wast` |
| Vibe ABI speedup | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-*.wast`, `tests/all/cli_tests.rs::run_component_thread_vibe_abi_speedup_probe` |
| Vibe generated atomic heap cursor | `vibe-lang` `feat/thread` component exports `thread-alloc-probe` and `thread-alloc-many-probe`, then manual fork CLI runs invoke those exports with unsafe OS-thread opt-in |
| Vibe generated worker dispatch | `vibe-lang` `feat/thread` component exports `thread-worker-probe`, then manual fork CLI invokes it with unsafe OS-thread opt-in |
| Vibe generated worker channel input | `vibe-lang` `feat/thread` component exports `thread-worker-channel-probe`, then manual fork CLI invokes it with unsafe OS-thread opt-in |
| Vibe channel payload type link | `vibe-lang` checker rejects a program that sends `String` to a `ThreadChannel[T]` and then uses `Threads::recv(ch)` as `Int` |
| Shared `thread.index` compatibility | `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-thread-index.wast` |
| Positive shared ownership subset | `runtime::component::threading::tests::unsafe_preemptive_validation_*` |

## Current Decision

This fork is ready to validate Vibe's ABI shape and performance hypothesis. It
is not ready to expose a proposal-complete shared-everything backend.

The next Vibe-side work should broaden the same slot ABI from the diagnostic
`Int -> Int` worker probe toward realistic heap object protocols and richer
worker/function forms while keeping the backend label explicitly fork-local:
`ComponentModelUnsafeOsThreads`.
