# Experimental Vibe thread backend contract

Status: fork-local contract
Date: 2026-06-05

This document defines the boundary that `vibe-lang` can target while the
`mizchi/wasmtime` fork experiments with Component Model shared threads.

This is not an upstream Wasmtime contract and not a stable
`ComponentModelShared` backend. It is a narrow contract for local validation
behind `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`.

For the proposal-vs-fork-local classification of this backend, see
`docs/experimental-shared-everything-conformance.md`.

## Backend Names

Vibe should treat the current fork as three distinct backend states:

| Vibe backend | Wasmtime mode | Speedup claim |
| --- | --- | --- |
| `ComponentModelCooperative` | default Component Model threading | no speedup backend |
| `ComponentModelUnsafeOsThreads` | fork-local unsafe opt-in | diagnostic speedup backend |
| `ComponentModelShared` | future safe shared-everything implementation | disabled for now |

The important rule is that `ComponentModelUnsafeOsThreads` is not
`ComponentModelShared`. It can validate workload shape and expose real
OS-thread execution for a constrained program shape, but it does not yet define
the full shared-everything ownership model.

## Opt-In Requirements

The unsafe backend is enabled only when all of these are true:

- the CLI or embedder sets `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`
- Component Model async and threading features are enabled
- core Wasm threads, shared memory, function references, GC, and
  shared-everything threads are enabled
- the program uses `canon thread.spawn-indirect`
- the spawn table is a shared `(ref null (shared func))` table
- the shared spawn table is either fixed-size or is a growable imported runtime
  start table whose table owner core instance defines no functions
- no other growable shared core table is present in the component instance
- the start function type is shared and currently has shape `(param i32)`
- the child start function communicates results through shared memory or
  imported shared globals that the fork can rebind
- if the child start function is defined in a core instance that owns shared
  mutable globals, Vibe only relies on the fork-local post-start flush-back
  point, not live shared storage while the start function is running
- for non-`() Store<T>`, the embedder installs
  `Store::set_unsafe_component_thread_store_data_factory`

The fork now carries an internal canonical shared/preemptive flag through the
component trampoline and runtime libcall path, and the unsafe OS-thread backend
is gated on that flag. The parser surface still does not expose the Component
Model `shared?` immediate yet, so the current legacy local `thread.spawn-*`
reader output is marked shared internally.

## Supported Program Shape

Vibe can target this shape for local experiments:

- split independent CPU work into numbered slots
- pass the slot id as the `i32` start argument
- write each slot result into shared memory
- use an atomic done counter and `memory.atomic.notify` to wake the parent
- use `memory.atomic.wait32`/`memory.atomic.notify` on rebound shared memory for
  simple gate/done synchronization between parent and child threads
- use producer-owned shared cancel-request flags plus
  `memory.atomic.notify` for Vibe-level cooperative cancellation
- aggregate final results in the parent after the done counter reaches the
  expected number of spawned threads
- represent Vibe-level join/completion/cancellation with the consolidated slot
  layout used by `thread-spawn-indirect-os-trampoline-vibe-abi.wast`
- treat the `i32` start argument as a context pointer into the Vibe slot ABI;
  it is deliberately separate from the canonical `thread.spawn-*` return value
- use `thread.available-parallelism` only for sizing or reporting, not as a
  semantic guarantee of safe shared-everything execution
- use Vibe heap allocation from multiple OS-owned children only through the
  standard shared-backend allocation helpers, which now reserve space with a
  shared-memory `i32.atomic.rmw.add` cursor
- use `Threads::spawn("alloc-probe", ch)` only as the current fork-local
  diagnostic task mode for stressing that cursor; it is not a public worker
  dispatch rule
- use `Threads::spawn("worker", ch)` for local worker-dispatch probes only when
  `"worker"` is a string-literal name of a non-exported, capture-free top-level
  Vibe function with the temporary shape
  `ThreadChannel[Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]] -> Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]`
- that temporary worker receives the `ch` handle as its only argument; the
  current channel-read probe proves a worker can read an `Int` payload with
  `Threads::recv(ch)` and return a derived tagged `Int` result through the slot
  payload
- the string channel probe similarly proves a worker can read a `String`
  payload with `Threads::recv(ch)` and return a derived tagged `Int` result
  through the slot payload
- the array channel probe proves a worker can read an `Array[Int]` payload with
  `Threads::recv(ch)`, inspect its header and elements, and return a derived
  tagged `Int` result through the slot payload
- the array-of-strings channel probe proves a worker can read an
  `Array[String]` payload with `Threads::recv(ch)`, inspect its header, follow a
  nested string pointer, and return a derived tagged `Int` result through the
  slot payload
- the array-of-strings result probe proves a worker can allocate an
  `Array[String]` result, return it through `ThreadTask[Array[String]]`, and let
  the parent follow a nested string pointer after `Threads::wait`
- the tuple channel probe proves a worker can read a parent-allocated
  `(Int, String)` payload with `Threads::recv(ch)`, follow the nested string
  pointer, and return a derived tagged `Int` result through the slot payload
- the tuple result probe proves a worker can allocate a `(Int, String)` result,
  return the heap tuple pointer through `ThreadTask[(Int, String)]`, and let the
  parent read both fields after `Threads::wait`
- the record result probe proves a worker can allocate a scalar-field record
  result, return the heap record pointer through `ThreadTask[{...}]`, and let
  the parent read both fields after `Threads::wait`
- the record channel probe proves a worker can use a source-level
  `ThreadChannel[{ score: Int, word: String }]` annotation, read a
  parent-allocated scalar-field record payload with `Threads::recv(ch)`, and
  return a derived tagged `Int` result through the slot payload
- thread workers that return tuples deliberately bypass Vibe's multi-value
  tuple-return optimization; the trampoline ABI requires a single `i64` tagged
  heap value, not a `(result i64 i64)` worker function
- treat channel handles as the Vibe type `ThreadChannel[T]` at the checker
  boundary; the runtime representation is still the same tagged `Int` handle,
  but `Threads::send` binds `T` and `Threads::recv` returns the same `T`
- store shared channel payload cells as full 64-bit tagged Vibe values with
  `i64.atomic.store`/`i64.atomic.load`; the channel handle remains the existing
  tagged `Int` handle
- store generated component-thread slot payloads as full 64-bit tagged Vibe
  values as well; the source type surface uses `ThreadTask[T]`, while the
  runtime task handle remains the existing tagged `Int` slot pointer
- keep channel-like payload cells and task slot payloads on this same 64-bit
  shared-value ABI; do not extend canonical `thread.spawn-*` to carry Vibe
  payloads or results
- allocate ordinary Vibe heap objects through a shared atomic cursor that
  returns at least 4-byte aligned object pointers; channel/task allocations
  that contain 64-bit atomic payload fields use 8-byte alignment
- treat task handles as the Vibe type `ThreadTask[T]` at the checker boundary;
  `Threads::spawn("name", ch)` returns `ThreadTask[R]` for supported
  string-literal worker result types, reserved diagnostic names still return
  `ThreadTask[Int]`, and `Threads::wait(task)` returns the task result type
  instead of accepting an arbitrary `Int`
- use `Threads::spawn("noop", ch)` only as the current reserved no-worker smoke
  path; other unknown worker names are compile-time errors

The current speedup probe is the reference shape:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-parallel.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast`
- `docs/experimental-component-thread-speedup.md`

## Vibe Slot ABI

The consolidated trampoline ABI uses fixed-size shared-memory slots. The
canonical spawn return value is intentionally not part of this ABI.
The single `i32` start argument is the slot/context pointer used by generated
Vibe code; the canonical spawn return value remains only a component
thread-table index.

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

Terminal codes are `0 = completed`, `1 = cancelled`, and `2 = failed-as-value`.
Vibe-generated code stores `payload` as an 8-byte tagged Vibe value; the
remaining fields are i32-sized control fields.
The remaining bytes in the 32-byte slot are reserved for future Vibe runtime
metadata and must be zero- or producer-owned.

## Current Guarantees

With the unsafe opt-in enabled, the fork currently guarantees only the following
for the constrained shape above:

- child work runs in a sibling `Store` on a host OS thread
- the parent `Store` is not re-entered from the child host thread
- the `wasmtime run` CLI installs the unsafe component thread store-data
  factory for its own `Host` state by cloning the parent `Host`; embedders that
  use a non-`() Store<T>` still need to install their own factory explicitly
- Vibe's standard fixed-size and dynamic-size linear-memory allocation helpers
  use a shared-memory atomic bump cursor for this backend, and
  `cabi_realloc` uses a shared-memory CAS loop so canonical ABI power-of-two
  alignment requests reserve aligned ranges from the same cursor
- Vibe's JS Preview2 host runner refuses to allocate into shared memory through
  the legacy `__heap_ptr` fallback; shared-memory Preview2 host allocation must
  use exported `cabi_realloc`
- Vibe builder grow and bulk grow avoid the heap-tip in-place fast path under
  this backend; growth takes the full realloc path so new storage is reserved
  through the shared atomic cursor
- the generated Vibe diagnostic allocation probes have exercised that cursor
  from OS-owned child Stores; one local run returned a positive checksum for a
  single child and for four concurrently spawned `alloc-probe` children
- the generated Vibe worker probe can dispatch a non-exported capture-free
  top-level worker by string literal, execute it in the child trampoline, and
  publish its tagged `Int` result through the slot payload
- the generated Vibe worker channel probe can pass the channel handle into that
  worker, let the child read an `Int` payload from shared channel storage, and
  publish the derived tagged `Int` result through the same slot payload
- the Vibe checker now links `Threads::send` and `Threads::recv` through
  `ThreadChannel[T]`, so a channel that has accepted a `String` payload cannot be
  read back as an `Int` in the same typed program
- shared channel cells preserve full 64-bit tagged payload values with
  `i64.atomic.store`/`i64.atomic.load`; they no longer truncate through an
  `i32` cell
- shared core memory used by the spawn plan is rebound into the child sibling
  instance
- parent and OS-owned child threads share the same futex wait queues for that
  rebound shared memory, so bidirectional atomic wait/notify synchronization
  works in the tested shape
- shared start-table dispatch observes parent table state needed by
  `thread.spawn-indirect`
- shared start-table dispatch observes parent-side growth for the imported
  runtime start-table shape
- growable shared tables outside the imported runtime start-table shape are
  rejected even when they are otherwise unused
- imported shared globals that target sibling-defined shared globals are
  rebound to parent global definitions
- start functions whose own core instance defines immutable shared globals are
  allowed because the child sibling receives the copied initial value
- start functions whose own core instance defines mutable shared globals are
  allowed in the diagnostic shape; their child inline VMContext values are
  flushed back to parent definitions after the start function returns
- components that declare Component Model resources or use Component Model GC
  canonical options are rejected before unsafe OS-thread execution
- spawned OS-owned threads are represented by parent thread-table placeholders
- terminal OS-owned host threads are joined by parent cleanup before their
  placeholders are removed
- embedders can use fork-local `Instance::unsafe_component_thread_cancel`,
  `Instance::unsafe_component_thread_status`, and
  `Instance::unsafe_component_thread_try_join` as host diagnostics for the
  unsafe backend; these must not be exposed as Vibe's proposal-level thread
  contract
- embedders can use fork-local `Instance::unsafe_component_thread_join` to
  block-consume completion after the guest protocol has made child completion
  independent of further parent guest execution
- fork-local `Instance::unsafe_component_thread_try_join_completion` and
  `Instance::unsafe_component_thread_join_completion` return host diagnostic
  `UnsafeComponentThreadCompletion` reports; they are not Vibe guest ABI values
- Vibe-level join/completion must be generated by the language runtime with
  shared state plus wait/notify, normally through a spawn trampoline
- Vibe-level cancellation must also be generated by the language runtime with
  producer-owned shared cancel flags; fork-local host diagnostics do not write
  trampoline-owned terminal slots
- fork-local consuming diagnostics remove terminal OS-owned entries from the
  component instance thread table; immediate stale status/cancel/try-join
  attempts are rejected as unknown unsafe OS-thread indices
- fork-local unsafe index lookup ignores numeric collisions with non-OS
  cooperative thread indices in other runtime component instances
- with epoch interruption enabled, `subtask.cancel` can interrupt
  already-running child Wasm at an epoch check and records the cancel-caused
  interrupt as `Cancelled`
- `subtask.cancel` can also wake an OS-owned child blocked in
  `memory.atomic.wait32/64` on rebound shared memory and records the
  cancel-caused interrupt as `Cancelled`
- setup/start/panic failures are recorded and surfaced during parent cleanup
- real Wasm traps in OS-owned child code are host diagnostic failures; they do
  not synthesize Vibe-level terminal status in trampoline-owned shared memory
- `subtask.cancel` records a best-effort cancellation request for OS-owned
  children
- OS-owned child Stores install a Store-local current guest thread whose
  `instance_rep` mirrors the parent-visible transient thread-table index
- shared start functions can import fork-local `canon thread.index` as a shared
  core function and observe the parent-visible `thread.spawn-indirect` index
- direct shared start functions can be spawned with `canon thread.spawn-ref`;
  Vibe still uses the table/trampoline path for worker dispatch and result slots

## Non-Guarantees

Vibe must not rely on these behaviors yet:

- `thread.spawn-ref` as a Vibe join/result ABI; it is only a direct start
  dispatch entry point in this fork
- relying on `thread.index` outside the current shared start-function shape; the
  fork-local compatibility path is intentionally limited to the canonical
  function named `thread.index` with shape `(func (result i32))`
- upstream canonical `thread.join`; the fork-local terminal join result APIs are
  removed, and remaining completion APIs are host diagnostics only
- treating fork-local blocking join as a guest-level operation
- using cooperative resume/suspend builtins on OS-owned thread indices
- forcefully interrupting a child store without epoch interruption, except for
  the fork-local `memory.atomic.wait32/64` interruption hook on rebound shared
  memory
- forcefully interrupting arbitrary host calls
- cancellation as a bounded-time stop-the-world mechanism
- live direct defined mutable shared-global access while the start function is
  running; the current shape only guarantees post-start flush-back
- treating the exported shared mutable Vibe heap pointer global as the live
  allocator cursor; the concurrent allocator authority is the shared-memory
  atomic cursor word, while `__heap_ptr` remains a compatibility/global-shape
  artifact for this backend
- assuming every Vibe heap operation is thread-safe; `enable_rc=true` is now
  rejected for the shared-thread backend, and RC/free-list allocation needs a
  separate shared-thread protocol before use; additional host helpers must not
  allocate by mutating `__heap_ptr` under shared memory unless they first define
  and test a shared allocator protocol
- treating the current `alloc-probe` task mode as user-visible semantics; it is
  a compile-time diagnostic branch used to exercise child-side allocation
- treating the current string-literal worker dispatch as general Vibe thread
  semantics; it is limited to non-exported capture-free top-level
  `ThreadChannel[Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]] -> Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]`
  functions and
  exists to validate the shared trampoline path before broader function values,
  closures, richer payload/result protocols, or heap object protocols are
  specified; dynamic worker-name expressions are rejected rather than treated
  as a no-worker task; the current `ThreadChannel[T]` checker contract only
  covers `Int`, `String`, `Array[Int]`, `Array[String]`, scalar-field tuple
  payloads, and scalar-field record payloads over the
  existing tagged-handle runtime
  representation
- relying on unknown `Threads::spawn("name", ch)` strings to silently behave as
  no-op tasks; only the reserved `"noop"` and `"alloc-probe"` names have
  non-function behavior
- inheriting every parent `Store` runtime setting into child stores; the
  fork-local child store path clones the configured store data through the
  factory and installs its explicit cancellation/epoch hooks, but embedders
  should not assume unrelated limiter, fuel, or callback configuration is
  automatically reproduced
- direct defined growable shared-table access; the unsafe path rejects start
  functions owned by a core instance that defines a growable shared table,
  growable shared table owners that define functions, and unrelated growable
  shared tables outside the imported runtime start-table shape
- fully synchronized table mutation beyond the imported runtime start-table
  dispatch path
- shared resources, GC heaps, or host resource handles crossing child stores;
  resource-bearing components and Component Model GC canonical options are
  rejected before unsafe OS-thread execution
- arbitrary component linking patterns that require all runtime objects to be
  shared
- treating returned thread-table indices as durable user-level join handles
- treating consumed numeric index values as permanently unique; component
  thread table slots can be reused after cleanup and can resolve to a later
  OS-owned thread
- returning ambiguous numeric indices that refer to multiple OS-owned runtime
  component instances
- using blocking join while the child is waiting for the parent guest to publish
  a wakeup; that can deadlock the parent host thread

If Vibe needs any of these, it should keep the backend disabled or route the
program through a more conservative backend.

## Probe Matrix

| Contract point | Probe |
| --- | --- |
| default mode remains cooperative | `thread-spawn-indirect-preemptive-smoke.wast` expected-fail entry |
| unsafe OS-thread wakeup works | `thread-spawn-indirect-preemptive-smoke.wast` with opt-in |
| direct `thread.spawn-ref` default mode remains cooperative | `thread-spawn-ref-preemptive-smoke.wast` expected-fail entry |
| direct `thread.spawn-ref` unsafe OS-thread wakeup works | `thread-spawn-ref-preemptive-smoke.wast` with opt-in |
| real thread-table placeholder exists | `thread-spawn-indirect-handle.wast` |
| OS-owned thread indices reject cooperative resume builtins | `thread-spawn-indirect-os-handle-traps.wast` |
| parent cleanup removes completed placeholders | `thread-spawn-indirect-os-cleanup-reuses-handles.wast` |
| store drop cleans outstanding OS-owned state | `thread-spawn-indirect-os-store-drop.wast` |
| bidirectional shared-memory wait/notify works | `thread-spawn-indirect-os-bidirectional-wait-notify.wast` |
| multiple OS-owned workers can share a gate, contend on an atomic job counter, and aggregate per-worker results | `thread-spawn-indirect-os-multi-worker-queue.wast` |
| multiple OS-owned workers can rendezvous at a shared-memory barrier before a second reduction phase | `thread-spawn-indirect-os-phased-barrier-reduction.wast` |
| Vibe-level completion is trampoline-managed shared state, not the spawn return value | `thread-spawn-indirect-os-trampoline-completion.wast` |
| Vibe-level terminal status codes are trampoline-managed shared state | `thread-spawn-indirect-os-trampoline-status.wast` |
| Vibe-level in-flight cancellation uses trampoline-owned shared cancel flags | `thread-spawn-indirect-os-trampoline-cancel-wakeup.wast` |
| consolidated Vibe runtime ABI uses trampoline-owned slots for join/status/cancel/failure/value aggregation | `thread-spawn-indirect-os-trampoline-vibe-abi.wast` |
| consolidated Vibe runtime ABI manual CLI smoke | `tests/all/cli_tests.rs::run_component_thread_vibe_abi_probe` |
| Vibe shared-value ABI preserves high 32 bits through channel-like cells and task-slot payloads | `thread-spawn-indirect-os-shared-value-payload.wast`, `run_component_thread_shared_value_payload_probe` |
| Vibe-generated allocation probe contends on the shared atomic heap cursor from OS-owned children | `vibe-lang` `feat/thread` exports `thread-alloc-probe` and `thread-alloc-many-probe`, invoked manually by the fork CLI with unsafe OS-thread opt-in |
| real Wasm traps are host diagnostics, not trampoline terminal status values | `thread-spawn-indirect-os-trampoline-trap-boundary.wast` |
| Vibe-generated string channel worker reads `ThreadChannel[String]` from an OS-owned child | `vibe-lang` `feat/thread` exports `thread-worker-string-channel-probe`, invoked manually by the fork CLI with unsafe OS-thread opt-in |
| shared table start dispatch is visible | `thread-spawn-indirect-os-shared-table-update.wast` |
| imported growable start-table dispatch is visible for table-only owners | `thread-spawn-indirect-os-shared-table-grow.wast`, `unsafe_preemptive_validation_allows_growable_imported_start_table` |
| direct defined growable shared-table starts are rejected | `unsafe_preemptive_validation_rejects_growable_defined_table_start` |
| growable shared table owners with functions are rejected | `unsafe_preemptive_validation_rejects_growable_table_owner_functions`, `thread-spawn-indirect-os-growable-table-owner-func-rejected.wast` |
| unrelated growable shared tables are outside the Vibe subset | `unsafe_preemptive_validation_rejects_unowned_growable_shared_table` |
| imported shared globals observe parent definitions | `thread-spawn-indirect-os-shared-global-update.wast` |
| direct mutable defined shared-global start functions flush back after return | `unsafe_preemptive_validation_allows_defined_mutable_shared_global_start`, `thread-spawn-indirect-os-defined-mutable-shared-global.wast` |
| direct immutable defined shared-global start functions are allowed | `unsafe_preemptive_validation_allows_defined_immutable_shared_global_start`, `thread-spawn-indirect-os-defined-immutable-shared-global.wast` |
| component resources are rejected | `unsafe_preemptive_validation_rejects_component_resources` |
| Component Model GC canonical options are rejected | `unsafe_preemptive_validation_rejects_component_gc_options` |
| shared-start `thread.index` observes the parent-visible transient thread-table index | `thread-spawn-indirect-os-thread-index.wast` with unsafe opt-in; normal-runner expected-fail entry |
| Vibe-shaped checksum and speedup work | `thread-spawn-indirect-os-speedup-*.wast` |
| Vibe ABI-shaped checksum and speedup work | `thread-spawn-indirect-os-trampoline-vibe-abi-speedup-*.wast`, `run_component_thread_vibe_abi_speedup_probe` |
| host diagnostics can cancel/observe/consume unsafe completion | `unsafe_os_thread_request_cancel_*`, `unsafe_os_thread_status_observes_completion_without_cleanup`, `unsafe_os_thread_try_join_*`, `unsafe_os_thread_join_*` |
| host diagnostic completion reports preserve child failure as a value | `unsafe_component_thread_try_join_completion_reports_failure_as_value` |
| host diagnostic completion reports return cancellation as a value | `unsafe_component_thread_join_completion_returns_cancelled_as_value` |
| consumed public unsafe indices are immediately stale | `unsafe_component_thread_try_join_rejects_stale_public_index`, `unsafe_component_thread_join_rejects_stale_public_index` |
| public unsafe index lookup ignores non-OS collisions | `unsafe_component_thread_lookup_ignores_non_os_index_collisions` |
| ambiguous OS-owned public indices are rejected | `unsafe_component_thread_lookup_rejects_ambiguous_os_indices` |
| consumed numeric indices can be reused for later OS-owned threads | `unsafe_component_thread_numeric_index_can_be_reused_after_join` |
| cancel can interrupt epoch-instrumented child Wasm | `component_thread_os_completion_epoch_interrupt_is_cancel` |
| cancel can wake atomic waiters | `component_thread_os_completion_cancel_interrupts_atomic_waiters`, `wasm_atomic_wait_interruption_traps` |

## Backend Decision Rule

For now, Vibe should use this decision rule:

1. Use `ComponentModelCooperative` for correctness-only Component Model
   scheduling probes.
2. Use `ComponentModelUnsafeOsThreads` only for local fork experiments that match
   the supported program shape and explicitly opt into the unsafe environment
   variable.
3. Treat the fork-local embedder cancel/status/try-join/join API as a
   diagnostic bridge, not as a stable guest-level thread handle contract. Vibe
   must build join/completion with its own shared state plus wait/notify.
4. Keep `ComponentModelShared` disabled until Wasmtime has guest-visible join or
   an equivalent proposal-level completion contract, stronger
   cancellation/interruption semantics, fixed and growable shared table
   semantics, and a sound ownership model for shared component state.

The next Wasmtime fork work should therefore focus on one semantic gap at a
time, with a WAST probe added before widening the contract.
