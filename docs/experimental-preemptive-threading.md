# Experimental preemptive Component Model threading

Status: fork-local design, Red probe, validation scaffold, and unsafe sibling-Store OS-thread probe
Date: 2026-06-02

This note records the current state of the `mizchi/wasmtime` fork experiment for
preemptive, OS-thread-backed Component Model `thread.spawn-*`.

This is not an upstream contribution plan. Do not open pull requests, comment on
issues, or review upstream Wasmtime pull requests from this fork experiment.
Follow the Bytecode Alliance AI Tool Use Policy for all Wasmtime work.

## Target behavior

The Component Model Canonical ABI describes `canon thread.spawn-indirect` as a
fusion of `thread.new-indirect` and `thread.resume-later`. With no `shared`
immediate, the spawned thread is cooperative. With the `shared` immediate, the
spawned thread is preemptive and can execute in parallel with other threads.

For Vibe, that means:

- `ComponentModelCooperative`: current fork behavior; useful for scheduling
  probes but not a speedup backend.
- `ComponentModelShared`: target behavior; guest work runs on real host threads
  and can make progress while the parent guest thread is blocked.

## Current verification result

The current fork does not implement true preemptive Component Model execution.
`thread.available_parallelism` therefore reports `1` for Component Model
threads until the shared/preemptive path exists.

The Red probe is:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-preemptive-smoke.wast`

The probe spawns a shared start function, then the parent thread blocks in
`memory.atomic.wait32`. The start function stores `1` to shared memory and
notifies the waiter.

Expected behavior:

- cooperative implementation: parent times out and returns `0`
- OS-thread implementation: child runs concurrently, notifies parent, returns
  `1`

Current direct command without unsafe opt-in:

```bash
target/debug/wasmtime wast \
  -W threads=y \
  -W component-model=y \
  -W component-model-async=y \
  -W component-model-threading=y \
  -W gc=y \
  -W function-references=y \
  -W shared-everything-threads=y \
  tests/misc_testsuite/component-model-threading/thread-spawn-indirect-preemptive-smoke.wast
```

Current result:

```text
expected 1
actual   0
```

The aggregate wast runner marks this file as a known Red probe in
`crates/test-util/src/wast.rs`, so normal local test runs stay green until the
preemptive path exists. Once the path is implemented, remove that expected-fail
entry.

The fork also has an explicit unsafe opt-in for local validation only:

```bash
WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 \
  target/debug/wasmtime wast \
  -W threads=y \
  -W component-model=y \
  -W component-model-async=y \
  -W component-model-threading=y \
  -W gc=y \
  -W function-references=y \
  -W shared-everything-threads=y \
  tests/misc_testsuite/component-model-threading/thread-spawn-indirect-preemptive-smoke.wast
```

With that environment variable set, the direct probe currently returns `1`.
The opt-in path now creates a sibling store using a fork-local per-thread
store-data factory and a sibling component instance on a host OS thread, then
rebinds the child core instances' defined shared-memory VMContext
pointers and matching imported-memory `from` slots to the parent shared-memory
allocation. It also rebinds shared core table definitions/imports and shared
runtime table slots to the parent table import before resolving the
`thread.spawn-indirect` start function. That table path supports fixed-size
shared tables and the imported runtime start-table growth shape only when the
growable table owner core instance defines no functions; direct defined
growable shared-table starts, growable table owners with functions, and
unrelated growable shared tables are rejected because their child VMContext
table definitions are inline copies and only the runtime start-table shape is in
the current Vibe ownership subset. For
shared globals, it copies the parent
defined-global value into the sibling defined-global slot and rebinds matching
child imported-global slots to the parent global definition. The unsafe path
also resolves the start function in the parent before spawning the OS thread.
Direct defined mutable global accesses still use inline VMContext storage, but
the child start function owner's mutable shared globals are flushed back to the
parent definitions after the start function returns. Direct immutable
shared-global reads are allowed because the copied initial value cannot diverge.

This is closer to the WASI Threads shape because it no longer re-enters the
same `Store` from two host threads. It is still intentionally incomplete:

- non-`()` store data requires
  `Store::set_unsafe_component_thread_store_data_factory`; the setter requires
  `T: Send`, and missing factories are rejected before OS-thread execution
- the returned thread id is now allocated in the parent component instance's
  thread table, but it is only an OS-owned lifecycle placeholder
- the OS-owned child Store installs a Store-local current guest thread while the
  start function runs, and that synthetic thread's `instance_rep` mirrors the
  parent-visible transient thread-table index returned by
  `thread.spawn-indirect`
- child setup failure, start failure, panic, and successful completion are
  recorded in that placeholder's completion record
- the placeholder uses a distinct OS-owned thread state; cooperative
  resume/suspend builtins reject it with `CannotResumeThread`
- the parent event loop polls completion records, joins terminal host threads,
  removes terminal OS-owned placeholders from the parent thread table,
  and surfaces setup/start/panic failures from cleanup
- embedders can request cancellation, query, or consume unsafe OS-owned
  completion through fork-local `Instance::unsafe_component_thread_cancel`,
  `Instance::unsafe_component_thread_status`, and
  `Instance::unsafe_component_thread_try_join`; embedders can also use
  `Instance::unsafe_component_thread_join` to block until completion when the
  guest protocol does not require further parent progress
- `Instance::unsafe_component_thread_try_join_completion` and
  `Instance::unsafe_component_thread_join_completion` return host diagnostic
  `UnsafeComponentThreadCompletion` reports, preserving child setup/start/panic
  failures as `Failed` values with failure messages instead of host API errors;
  they must not be lowered as a guest-visible `thread.join`
- real Wasm traps in OS-owned child code stay in that host diagnostic failure
  channel; Vibe-level terminal status values must be written by a generated
  trampoline, not synthesized from host cleanup
- consuming fork-local diagnostics remove terminal OS-owned entries from the
  component instance thread table; immediate stale public-index operations are
  rejected as unknown unsafe OS-thread indices
- public unsafe-index lookup ignores numeric collisions with non-OS cooperative
  thread indices in other runtime component instances
- `subtask.cancel` sets a best-effort cancellation request on OS-owned child
  completions; a child can observe it before entering the start function
- when epoch interruption is enabled, the child store installs a fork-local
  epoch callback so `subtask.cancel` can also interrupt already-running child
  Wasm at an epoch check and record that interrupt as `Cancelled`
- if the child is blocked in `memory.atomic.wait32/64` on rebound shared
  memory, `subtask.cancel` also interrupts the shared-memory waiter; the Wasm
  wait libcall reports `Trap::Interrupt`, which is recorded as `Cancelled`
- there is still no upstream canonical `thread.join`; completion reports are
  fork-local host diagnostics only
- returned thread-table indices are not durable identities and may be reused
  after cleanup, including resolving to a later OS-owned thread
- numeric indices that match multiple OS-owned runtime component instances
  remain ambiguous
- calling `thread.index` from the shared start-function shape is supported by a
  fork-local canonical typing/runtime compatibility path, and the child observes
  the parent-visible `thread.spawn-indirect` index
- shared table dispatch observes parent table entry updates, and imported
  runtime start-table growth is allowed for table-only owner modules. Direct
  defined growable shared-table starts, growable table owners that define
  functions, and unrelated growable shared tables remain rejected. Imported
  shared globals can point at parent
  definitions, direct mutable defined shared-global starts flush back to parent
  definitions after return, and direct immutable shared-global start functions
  are allowed through the copied initial value. Components that declare
  Component Model resources or use Component Model GC canonical options are
  also rejected; direct defined table growth and live direct mutable
  defined-global sharing do not have a sound ownership model yet
- rebound shared memory uses the same futex wait queues for parent and child
  `memory.atomic.wait32`/`memory.atomic.notify`; the child rebind replaces both
  the VMContext memory pointer and the host-side `Memory::Shared` object so the
  atomic wait/notify libcall reaches the parent `SharedMemory` parking spot.
  This is covered by a bidirectional wait/notify probe, but it is still not a
  complete shared-object ownership model

Do not use this as the final architecture. It exists to validate the workload
shape and to keep the next safe implementation target concrete.

When the unsafe opt-in is enabled, `thread.available_parallelism` reports
`std::thread::available_parallelism()`. Without it, the fork still reports `1`.

The fork also has a Vibe-shaped CPU-bound speedup probe:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-parallel.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast`
- `tests/all/cli_tests.rs::run_component_thread_speedup_probe` (ignored timing
  probe)
- `tests/all/cli_tests.rs::run_component_thread_vibe_abi_speedup_probe`
  (ignored timing probe)
- `docs/experimental-component-thread-speedup.md`
- `docs/experimental-vibe-thread-contract.md`

The fork also has a bidirectional shared-memory synchronization probe:

- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-bidirectional-wait-notify.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-cancel-wakeup.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-multi-worker-queue.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-phased-barrier-reduction.wast`

This probe requires the unsafe opt-in. It proves that a parent waiter can be
observed and woken by an OS-owned child, and that a child waiter can be observed
and woken by the parent, through the same rebound shared memory. The trampoline
cancel wakeup probe applies the same mechanism to Vibe-level in-flight
cancellation: the parent writes a producer-owned cancel flag, not a canonical
thread handle, and the generated child trampoline publishes the cancelled
terminal status. The Vibe ABI probe combines that mechanism with normal
completion, failed-as-value status, and parent-side aggregation.

The multi-worker queue probe extends the same shared-memory synchronization
shape to four OS-owned child threads: all workers block on a parent-published
start gate, contend on a shared atomic job counter, publish per-worker
counts/checksums, and notify a shared done counter that the parent waits on.
The phased barrier/reduction probe then exercises child-to-child cooperation:
all workers publish phase-1 contributions, rendezvous at a shared-memory
barrier, and only after the last arrival advances the barrier generation do
they enter phase 2 and consume the complete phase-1 reduction.

On 2026-06-01, direct local CLI timing with `available_parallelism=10` measured
the serial WAST at `0.23s` real time and the unsafe OS-thread parallel WAST at
`0.07s` real time for the same checksum. This validates speedup for the narrow
diagnostic workload; it does not make the path a complete shared-everything
implementation.

On 2026-06-02, the ABI-shaped serial/parallel probes measured `0.22s` and
`0.08s` real time respectively for the same checksum while using the
trampoline-owned Vibe slot layout.

For Vibe integration, the fork-local unsafe path should be treated as
`ComponentModelUnsafeOsThreads`, not as stable `ComponentModelShared`. The
supported program shape and non-guarantees are defined in
`docs/experimental-vibe-thread-contract.md`.

## Why the current path is cooperative

`Instance::thread_spawn_indirect` currently creates a guest thread and calls
`resume_thread`. `resume_thread` enqueues `WorkItem::GuestCall` into the
Component Model concurrent event loop.

That event loop is per `Store`, and its worker is a fiber, not an OS thread.
`run_on_worker` resumes the worker fiber in the same store-owned execution
context. This lets `thread.yield` and other explicit suspension points switch
between guest tasks, but it cannot make progress while the currently-running
guest frame blocks the host thread in `memory.atomic.wait32`.

## Frontend limitation

The current `wasmparser` / `wast` APIs used by this fork do not expose the
Component Model `shared?` immediate on `thread.spawn-ref`,
`thread.spawn-indirect`, or `thread.available-parallelism`, even though the
checked-in Component Model design documents describe it.

The validator does already require `thread.spawn-indirect` and
`thread.spawn-ref` to use a shared start function type; the indirect path also
requires a shared `(ref null (shared func))` table. The Wasmtime-internal
component trampoline path now carries a `shared` flag through initializer
metadata, Cranelift lowering, VM libcalls, and runtime dispatch, and the unsafe
OS-thread backend only runs when that flag is true and the unsafe environment
variable is set. Until the parser surface catches up, the current legacy
fork-local `thread.spawn-*` reader output is marked as `shared: true`
internally.

This cannot be fixed safely by only making the local `wasmparser` reader guess
between legacy and proposal encodings. The legacy local binary shape starts
with a type index, while the proposal shape starts with `shared?`; common type
indices `0` and `1` are byte-identical to valid `shared?` immediates, and
canonical function entries are not independently length-delimited. Real support
needs the affected wasm-tools surface to move together: `wasmparser`, `wast`,
`wasm-encoder`, and `wasmprinter`.

## Why same-Store OS spawning is not safe

The tempting minimal implementation is to call `std::thread::spawn` around the
existing start closure and pass the same `StoreOpaque`/`VMStore` pointer to the
new host thread. That would be unsound.

Concrete blockers:

- `VMStore` is not a public `Send + Sync` execution object.
- `StoreOpaque` owns one `VMStoreContext` and is self-referential/pinned.
- Wasm entry mutates per-store runtime fields such as stack limits, last-wasm
  metadata, and stack chains.
- Component concurrent state, worker fibers, task queues, pending calls, and
  cleanup paths are store-local mutable state.
- GC roots, tables, globals, component resources, cancellation, and traps need a
  real cross-thread ownership model.

Using unsafe raw pointers to run the same store on two OS threads may appear to
work for a narrow smoke test, but it would create Rust aliasing violations and
runtime data races. The fork's current unsafe opt-in has moved away from this:
it creates a sibling store on the spawned host thread and explicitly rebinds
only the narrow shared-memory state needed by the smoke probe.

## Existing safe model to copy

The existing `wasmtime-wasi-threads` implementation is the useful baseline. It
spawns a Rust OS thread, then creates a new `Store` and a new instance from an
`InstancePre` inside that OS thread. Shared state is explicitly shared, starting
with `SharedMemory`.

This avoids concurrent calls into the same store. For Component Model shared
threads, the equivalent architecture needs one of these designs:

1. Per-thread `Store`/instance template:
   - capture enough instantiation context to create a sibling component instance
     on the spawned host thread
   - share only explicitly shareable runtime objects
   - first milestone can target shared memory and table-based entry dispatch
2. Split shared and per-thread runtime state:
   - make shared tables/globals/resources/GC heaps synchronized or atomic
   - give each host thread its own call-stack and `VMStoreContext`-like state
   - implement trap, cancellation, cleanup, and TLS semantics across threads

The first design is closer to WASI Threads and can validate Vibe speedup sooner.
The second design is closer to full shared-everything semantics.

## Sibling instances are not enough

`Instance::instance_pre` can recover the `InstancePre<T>` used to instantiate a
component instance, and `InstancePre<T>` can be cloned without requiring
`T: Clone`. That makes it look like the Component Model path can copy the WASI
Threads architecture directly: spawn an OS thread, create a fresh `Store<T>`,
instantiate the same `InstancePre<T>`, then call the start function there.

That is only a safe execution-state boundary. It is not yet a
shared-everything state boundary.

During component instantiation, `Instantiator::run` processes
`GlobalInitializer::InstantiateModule` by calling
`Instance::new_started` for each core module. A sibling component instance
therefore receives fresh core instances. If a component internally defines a
shared memory, table, or global, the sibling receives a different runtime object
from the parent. The same issue applies to a linker-provided imported core
module: the linker stores the `Module`, not an already-instantiated export set,
so each component instantiation creates fresh module state.

The regression guard is:

- `tests/all/component_model/instance.rs::instance_pre_sibling_does_not_share_defined_shared_memory`

That test writes through one component instance and verifies that a sibling
instantiated from the same `InstancePre` does not observe the write, while the
parent still does. This prevents the fork from treating sibling
`InstancePre` instantiation as a complete `thread.spawn-* shared` solution.

The next real implementation layer must split the state explicitly:

- per-thread execution state: `Store`, call stack, trap/cancellation state,
  current component thread, and TLS-like runtime state
- shared component state: shared memories, shared tables, shared globals,
  shared function references, and any resource/GC state that can cross threads

For an MVP, the fork can be narrower than full shared-everything semantics, but
it still needs an explicit `ComponentThreadTemplate`-like runtime object that
captures the parent instance's shareable objects and reuses them when creating
the spawned thread's execution state. Re-instantiating the component alone is
insufficient.

The fork now has the first internal scaffold:

- `crates/wasmtime/src/runtime/component/threading.rs`
- `ComponentThreadTemplate<T>`
- `tests: runtime::component::threading::tests::template_records_runtime_state_and_core_instance_gap`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-table-update.wast`

This template captures the original `InstancePre<T>` plus the runtime
memory/table slots currently visible through `VMComponentContext`. It also
records whether the component instantiated core modules, which means the
preemptive path still requires an explicit core-instance-state sharing layer.
For each captured runtime memory/table slot, the template records the parent
runtime core instance that the canonical ABI extracted the slot from.

One subtlety is that this extraction source is not necessarily the ultimate
allocation owner. A component can instantiate one core module that defines a
memory/table, pass those exports into another core module, and then lift through
the importing module's re-export. In that case the runtime memory slot is
extracted from the importing module while a thread table can be extracted from
the defining module. The fork keeps those source instances distinct so the
future rebind layer does not accidentally collapse component linking structure.

In normal mode the current template is not sent to an OS thread and does not
claim that captured runtime slots are thread-shareable. With
`WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`, the fork derives a
typed spawn plan from the template and an explicit store-data factory, then uses
that plan to instantiate a sibling component on the spawned host thread.

That spawn plan currently reuses parent-owned shared core memories, tables, and
imported globals. For shared memories, it rebinds both:

- the child defining core instance's shared-memory VMContext pointer
- any child importing core instance's matching `VMMemoryImport.from` slot

For shared tables, it rebinds child defined/imported table VMContext slots and
the shared runtime table slot used by `thread.spawn-indirect` start dispatch.

For shared globals, it cannot pointer-rebind direct mutable defined-global accesses
because Wasmtime stores defined globals inline in each core instance's
`VMContext`. The current fork copies the parent value into the child defined
global and rebinds child imported-global slots that targeted the child
counterpart so they point at the parent definition. That makes imported shared
global users observe the parent global. Direct immutable accesses in the child
defining module are also allowed because the copied value cannot change after
instantiation. Direct mutable accesses in the child defining module are not live
shared, but the unsafe fork now flushes those child owner values back to the
parent definition after the start function returns.

The corresponding regression guards are:

- `tests: runtime::component::threading::tests::spawn_plan_rebinds_child_core_shared_memory`
- `tests: runtime::component::threading::tests::spawn_plan_rebinds_child_core_shared_table_and_runtime_slot`
- `tests: runtime::component::threading::tests::spawn_plan_rebinds_child_core_shared_global_import`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_rejects_unshared_mutable_global`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_mutable_shared_global_start`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_immutable_shared_global_start`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_rejects_component_resources`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_rejects_component_gc_options`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_table_owner_functions`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-growable-table-owner-func-rejected.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-defined-mutable-shared-global.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-defined-immutable-shared-global.wast`
- `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-global-update.wast`

This is enough for the preemptive smoke's start function to mutate the parent
shared memory from an OS thread. It is not a general shared-everything component
state model.

The fork also has a low-level rebind scaffold:

- `Instance::rebind_component_thread_runtime_state`
- `ComponentThreadTemplate::rebind_runtime_state_to`
- `ComponentInstance::component_thread_rebind_runtime_memory`
- `ComponentInstance::component_thread_rebind_runtime_table`
- `tests: runtime::component::threading::tests::rebind_runtime_state_replaces_sibling_slots`

This proves that a sibling component instance's `VMComponentContext` runtime
memory/table slots can be overwritten with the parent template's captured slots.
The test intentionally runs inside one `Store`; it is not evidence that
cross-`Store` or cross-OS-thread execution is sound. The low-level rebind must
be fronted by a validation layer that only permits runtime objects with an
explicit shareable ownership model.

That validation layer now exists as a separate guard:

- `ComponentThreadTemplate::validate_rebindable_runtime_state`
- `ComponentThreadTemplate::validate_unsafe_preemptive_spawn_indirect`
- `tests: runtime::component::threading::tests::rebind_validation_rejects_unshared_runtime_memory`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_allows_start_table_only`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_rejects_component_resources`
- `tests: runtime::component::threading::tests::unsafe_preemptive_validation_rejects_component_gc_options`

The guard currently permits only runtime memory slots proven to come from
`Export::SharedMemory`, shared table slots in the fixed-size shape or the
single table-only imported growable start-dispatch shape, imported shared
globals, immutable direct defined shared-global reads, and the fork-local
post-start flush-back shape for direct mutable defined shared globals. Runtime
memory extracted from a normal `Export::Memory` is rejected before an OS-thread
path may rebind it into a sibling execution instance. Mutable unshared globals,
direct defined growable shared-table owner starts, growable shared table owners
that define functions, unrelated growable shared tables, Component Model
resources, and Component Model GC canonical options are also rejected.

This is intentionally stricter than the unsafe single-`Store` rebind scaffold.
The unsafe rebind test proves that the VM slot can be overwritten; the
validation guard defines when a future OS-thread path is allowed to use that
mechanism.

The unsafe opt-in path uses a narrower guard: it requires a captured shared core
memory spawn plan, and the only runtime table it accepts is the
`thread.spawn-indirect` start table. It then calls the start function from a
host thread whose join handle is retained in the parent completion record. The
thread is still not registered as a normal Component Model cooperative thread.
That table acceptance currently relies on the Component Model validator's
`thread.spawn-indirect` checks, because the fork does not yet retain enough
runtime table sharedness metadata after translation.

The same unsafe opt-in guard now rejects Component Model resources and Component
Model GC canonical options before OS-thread execution. Those shapes require
store-owned resource tables, destructors, borrow tracking, host handles, GC
heaps, and roots to cross sibling stores, which the current diagnostic path does
not implement.

## Next implementation slices

1. Keep the Red probe failing in normal mode until real OS-thread execution
   exists.
2. Keep fork-local completion APIs as host diagnostics, not Vibe guest ABI.
3. Implement Vibe-level completion with trampoline-managed shared state and
   wait/notify.
4. Only after that, widen toward full shared tables/globals/GC/resource sharing.

## References

- Shared-everything threads proposal:
  <https://github.com/WebAssembly/shared-everything-threads>
- Component Model Canonical ABI threading builtins:
  <https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md>
- Local fork goal:
  <./experimental-fork-goal.md>
