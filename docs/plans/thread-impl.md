# Component Model Thread Implementation Plan

Status: fork-local execution plan
Date: 2026-06-02
Scope: `mizchi/wasmtime` only

This plan tracks the remaining work to turn the current
`WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` probe into a more complete
Component Model shared-thread experiment for Vibe.

This is not an upstream contribution plan. Do not open pull requests, comment on
issues, or review upstream Wasmtime pull requests from this fork experiment.
Follow the Bytecode Alliance AI Tool Use Policy for all Wasmtime work.

## Current Baseline

Implemented:

- cooperative `canon thread.spawn-indirect`
- `canon thread.available-parallelism`
- fork-local unsafe opt-in for host OS thread execution
- spawned host thread creates a sibling store from a per-thread store-data
  factory
- spawned host thread instantiates a sibling component from a typed
  `InstancePre<T>`
- child core defined shared-memory pointers are rebound to parent shared memory
- child core imported-memory `from` slots that target those definitions are also
  rebound
- child shared table definitions/imports and shared runtime table slots are
  rebound to parent table state for table-based start dispatch
- the unsafe preemptive validator now reports the positive Vibe shared
  ownership subset: shared core memories, shared runtime start tables,
  fixed-size shared core tables, the limited growable imported runtime
  start-table shape, shared global definitions, and direct mutable
  shared-global flush-back slots
- growable shared tables are allowed only when they are the imported runtime
  start table and their owner core instance defines no functions; direct
  defined growable shared-table starts, growable shared table owners with
  functions, and unrelated growable shared tables remain rejected
- child shared global imports that target sibling-defined shared globals are
  rebound to parent global definitions, with an initial value copy for child
  defined-global slots
- direct defined mutable shared-global writes from the start function owner are
  flushed from the child inline VMContext slot back to the parent definition
  after the start function returns
- unsafe OS-thread spawn allows start functions that directly read immutable
  defined shared globals; the child sibling receives the copied initial value
- unsafe OS-thread spawn rejects components that declare Component Model
  resources or use Component Model GC canonical options, because those stores'
  resource tables and GC heaps are not shared by this fork
- mutable unshared globals are rejected before unsafe OS-thread execution
- unsafe OS-thread spawns allocate a real parent component thread-table index
- child OS threads record setup failure, start failure, panic, and successful
  completion in a parent-owned completion record
- parent cleanup joins terminal OS-owned host threads, removes completed
  placeholders from the thread table, and surfaces child failures
- fork-local host diagnostic APIs can observe, poll-consume, or block-consume
  unsafe OS-thread completion: `Instance::unsafe_component_thread_status`,
  `Instance::unsafe_component_thread_try_join`, and
  `Instance::unsafe_component_thread_join`
- fork-local embedders can request cancellation of a single OS-owned thread
  index with `Instance::unsafe_component_thread_cancel`
- `subtask.cancel` requests cancellation of OS-owned child completions; without
  epoch interruption this remains a best-effort request
- when epoch interruption is enabled, the unsafe OS-thread child store installs
  a fork-local cancellation deadline callback; `subtask.cancel` ticks the
  engine epoch and a cancel-caused `Trap::Interrupt` is recorded as
  `Cancelled`
- when an unsafe OS-thread child is blocked in `memory.atomic.wait32/64` on a
  rebound shared memory, cancellation interrupts that waiter and records the
  cancel-caused `Trap::Interrupt` as `Cancelled`
- parent task lifetime accounting keeps OS-owned children interesting until
  their placeholders are cleaned up
- the preemptive smoke probe passes with the unsafe opt-in
- a Vibe-shaped CPU-bound checksum workload shows wall-clock speedup through
  the unsafe Component Model OS-thread path

Known limitations:

- the OS-thread path is not registered in the Component Model thread lifecycle
- the returned thread id points at a parent lifecycle placeholder; there is no
  guest-visible join operation yet
- forced cancellation of a child store already executing Wasm uses epoch
  interruption for CPU execution and a fork-local shared-memory wait
  interruption hook for `memory.atomic.wait32/64`; arbitrary host-blocked calls
  are still not interrupted by this mechanism
- general thread-handle operations are incomplete
- direct defined shared-table growth is not soundly shared; growable shared
  table owners that define functions are rejected by the unsafe preemptive
  guard. Direct mutable defined-global access is only a start-return flush-back
  diagnostic shape, not live shared storage
- Component Model resources and GC canonical options are rejected before unsafe
  OS-thread execution rather than shared

## Invariants

- Normal mode remains cooperative and green.
- The unsafe OS-thread path stays behind
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`.
- No path re-enters the same `Store` from two host threads.
- Every widening step must be guarded by a WAST or unit test first.
- Unsupported state must fail explicitly instead of silently falling back to
  unsound sharing.

## Milestones

### T1: Register Spawned OS Threads In The Parent Thread Table

Status: done

Goal: stop returning a synthetic thread id.

Implementation:

- allocate a parent `GuestThread` entry before spawning the host thread
- insert it into the parent component instance's thread handle table
- return that real handle from `thread.spawn-indirect`
- keep the parent entry in a running/OS-owned state until completion tracking is
  implemented

Tests:

- add a WAST probe where `thread.spawn-indirect` returns a handle different from
  `thread.index`
- run it with and without the unsafe opt-in
- current test:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-handle.wast`

Known remaining gap after T1:

- the parent entry is only a lifecycle placeholder; it is not yet joined or
  cleaned up by the child thread.

### T2: Add Structured OS Thread Completion

Status: done

Goal: record whether the OS thread completed, trapped, or panicked.

Implementation:

- move the child-thread result into an `Arc<Mutex<...>>` or narrow completion
  cell owned by the parent placeholder
- record setup failure, start-function success, trap, and panic separately
- expose debug logging from the parent entry
- avoid mutating `StoreOpaque` from the child host thread

Tests:

- WAST smoke for successful completion
- Rust unit tests for successful completion and setup/start/panic records
- current tests:
  `runtime::component::concurrent::tests::component_thread_os_completion_records_success`
  `runtime::component::concurrent::tests::component_thread_os_completion_records_failures`

### T3: Define Handle Semantics For OS-Owned Threads

Status: done

Goal: make invalid handle operations explicit and prepare for join/cancel.

Implementation:

- add a distinct `GuestThreadState` for OS-owned component threads
- make `thread.yield-to-suspended`, `thread.suspend-to-suspended`, and
  `thread.unsuspend` reject OS-owned threads with a deterministic trap
- document that cooperative thread handles and OS-owned thread handles are not
  interchangeable yet

Tests:

- WAST trap probes for using cooperative resume builtins on OS-owned handles
- current tests:
  `runtime::component::concurrent::tests::unsafe_os_spawned_thread_uses_distinct_state`
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-handle-traps.wast`

### T4: Parent-Driven Cleanup

Status: done

Goal: avoid leaking parent thread-table placeholders.

Implementation:

- add a parent-store polling/cleanup point that observes completion records
- remove completed OS-owned thread entries from the parent thread handle table
- ensure cleanup does not require the child host thread to borrow the parent
  store
- current implementation:
  `ConcurrentState::unsafe_os_threads` tracks OS-owned placeholders, and
  `StoreOpaque::cleanup_completed_unsafe_os_threads` is called from the parent
  event loop and test-only state probes

Tests:

- repeated spawn smoke that confirms table entries do not grow without bound
- store-drop test with outstanding OS-owned threads
- current tests:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-cleanup-reuses-handles.wast`
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-store-drop.wast`

### T5: Per-Thread Store Data Factory

Status: done

Goal: remove the `Store<()>` restriction.

Implementation:

- add a fork-local API or internal callback for constructing child store data
- move `ComponentThreadSpawnPlan` from `InstancePre<()>` to a typed plan plus a
  data factory
- reject non-`Send` or non-factory data at the opt-in boundary
- current implementation:
  `Store::set_unsafe_component_thread_store_data_factory` installs a `T: Send`
  factory, and `ComponentThreadSpawnPlan<T>` builds the child store from that
  factory. `Store<()>` keeps a default unit factory for existing probes.

Tests:

- Rust embedder test with non-unit store data
- regression test that no unsafe `MaybeUninit<T>` data construction exists
- current tests:
  `runtime::component::threading::tests::spawn_plan_uses_non_unit_store_data_factory`
  `runtime::component::threading::tests::spawn_plan_rejects_non_unit_store_data_without_factory`
  `rg MaybeUninit crates/wasmtime/src/runtime/component/threading.rs`

### T6: Shared Table Support

Status: done

Goal: make table-based dispatch and shared function references explicit instead
of relying only on child-instantiated tables.

Implementation:

- capture core shared tables in `ComponentThreadTemplate`
- preserve `wasmparser::TableType::shared` in `wasmtime_environ::Table` and
  include table sharedness in type matching
- rebind child defined/imported table VMContext slots where the table type is
  shared
- rebind shared runtime table slots used by `thread.spawn-indirect` to the
  parent table import before OS-thread start dispatch
- reject unshared runtime tables before OS-thread execution; the current
  canonical validator also rejects unshared `thread.spawn-indirect` tables at
  component validation time
- initially require fixed-size shared tables for unsafe OS-thread execution
  until table growth has a shared ownership model; T28 later relaxes this for
  imported runtime start-table growth only

Tests:

- WAST probe where parent updates a shared table entry and child dispatch sees
  the update:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-table-update.wast`
- WAST invalid probe for unshared `thread.spawn-indirect` table state:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-unshared-table-invalid.wast`
- Rust unit test for child shared table/core import/runtime table rebinding:
  `runtime::component::threading::tests::spawn_plan_rebinds_child_core_shared_table_and_runtime_slot`
- Rust unit test for the original growable shared-table rejection boundary:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_shared_table`

### T7: Shared Global Support

Status: done

Goal: make shared globals visible across sibling component instances.

Implementation:

- preserve `wasmparser::GlobalType::shared` in `wasmtime_environ::Global` and
  include global sharedness in type matching
- capture shared core globals in `ComponentThreadTemplate`
- copy the parent defined-global value into the child counterpart before
  starting the OS-owned thread
- rebind child imported-global slots that targeted the child counterpart so they
  point at the parent shared global definition
- reject mutable unshared globals before the unsafe preemptive path runs

Known limitation:

- Wasmtime stores defined globals inline in each core instance's `VMContext`.
  The fork therefore cannot pointer-rebind direct mutable defined-global
  accesses yet. Direct immutable accesses in the child defining module see the
  copied initial value; imported users are rebound to the parent definition.
- As of T17, the unsafe preemptive path rejects a start function whose callee
  vmctx belongs to a core instance that defines shared globals. T20 refines
  this to reject only mutable shared globals.

Tests:

- WAST probe where an OS-spawned start function observes a parent shared-global
  update:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-global-update.wast`
- Rust unit test for shared global capture, child import rebinding, and initial
  value copy:
  `runtime::component::threading::tests::spawn_plan_rebinds_child_core_shared_global_import`
- Rust unit test for mutable unshared global rejection:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_unshared_mutable_global`
- Rust unit test for direct defined mutable shared-global start flush-back
  validation added later in T29:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_mutable_shared_global_start`

### T8: Cancellation, Trap, And Join Model

Status: done

Goal: make OS-owned threads participate in the Component Model lifecycle.

Implementation:

- store a host `JoinHandle` in the OS-owned completion record and join terminal
  host threads before removing the parent placeholder
- surface setup/start/panic failures from
  `cleanup_completed_unsafe_os_threads` after the placeholder is cleaned up
- treat a host thread that exits by panic while the completion still says
  `Running` as a panicked OS-owned child during cleanup
- propagate `subtask.cancel` to OS-owned child completion records as a
  best-effort cancellation request
- let a child observe that request before entering the start function; once
  child Wasm is already running this fork does not force an interrupt yet
- defer a guest-visible join operation because the current proposal surface does
  not expose one for thread handles
- decrement the parent task's interesting-task count only after all OS-owned
  child placeholders have been removed

Tests:

- Rust unit tests for completion cancel request, terminal join, failure
  propagation through cleanup, and task lifetime accounting:
  `runtime::component::concurrent::tests::component_thread_os_completion_records_cancel_request`
  `runtime::component::concurrent::tests::component_thread_os_completion_waits_for_join_handle`
  `runtime::component::concurrent::tests::cleanup_completed_unsafe_os_thread_reports_child_failure`
  `runtime::component::concurrent::tests::unsafe_os_thread_keeps_parent_task_interesting_until_cleanup`
  `runtime::component::concurrent::tests::unsafe_os_thread_cancel_request_reaches_completion`
- existing WAST regressions for OS-owned cleanup and store-drop behavior:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-cleanup-reuses-handles.wast`
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-store-drop.wast`

### T9: Vibe Speedup Validation

Status: done

Goal: validate that Vibe can actually get parallel speedup through the Component
Model path.

Implementation:

- added Vibe-compatible serial and parallel Component Model WAST probes:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-serial.wast`
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-speedup-parallel.wast`
- the workload runs four independent 50,000,000-iteration CPU chunks and keeps
  the checksum identical across serial and OS-thread Component Model paths
- the parallel path spawns four component threads, writes per-slot results into
  shared memory, increments a shared done counter, and wakes the parent with
  `memory.atomic.notify`
- the parallel WAST also checks that `thread.available-parallelism` is positive
- added an ignored CLI timing probe:
  `tests/all/cli_tests.rs::run_component_thread_speedup_probe`
- kept the parallel WAST in the normal aggregate runner as a should-fail probe
  because it requires `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`

Tests:

- direct WAST checksum probes with the unsafe opt-in:
  `target/debug/wasmtime wast ... thread-spawn-indirect-os-speedup-serial.wast`
  `target/debug/wasmtime wast ... thread-spawn-indirect-os-speedup-parallel.wast`
- normal aggregate WAST behavior:
  `cargo test --test wast thread-spawn-indirect-os-speedup-serial`
  `cargo test --test wast thread-spawn-indirect-os-speedup-parallel`
- ignored timing probe:
  `cargo test --test all run_component_thread_speedup_probe -- --ignored --nocapture`
- local direct timing on 2026-06-01 with `available_parallelism=10`:
  serial `0.23s` real, parallel `0.07s` real
- details:
  `docs/experimental-component-thread-speedup.md`

### T10: Toward Usable Vibe Backend Semantics

Status: done

Goal: decide what Vibe can safely call from this fork and what still needs
runtime semantics before exposing a stable backend.

Implementation:

- defined the Vibe-side abstraction boundary for spawn, completion,
  cancellation, and result aggregation:
  `docs/experimental-vibe-thread-contract.md`
- split the fork state into three backend names:
  `ComponentModelCooperative`, `ComponentModelUnsafeOsThreads`, and
  `ComponentModelShared`
- documented the supported unsafe program shape: slot-based
  `thread.spawn-indirect`, shared-memory result slots, atomic done counter, and
  parent aggregation
- documented non-guarantees: guest-visible join, forced child interruption,
  broad shared table/global/resource/GC ownership, and durable user-level thread
  handles
- chose the next fork direction: widen one semantic gap at a time, starting
  with a completion/join contract before treating the backend as stable
- keep `ComponentModelShared` disabled unless the exposed behavior is stronger
  than the current unsafe diagnostic path

Tests:

- the contract maps each supported point to existing WAST probes:
  preemptive smoke, handle allocation, OS-owned handle traps, cleanup reuse,
  store drop, shared table/global rebinding, and T9 speedup probes
- Wasmtime fork must keep adding direct WAST probes before every semantic
  widening

### T11: Guest-Visible Completion Or Join Contract

Status: done

Goal: give Vibe a bounded way to observe spawned thread completion without
encoding every result path through ad hoc shared-memory polling.

Implementation:

- the current proposal/parser surface does not expose a join-like operation for
  Component Model thread handles in this fork, so this milestone uses a
  fork-local embedder API instead of adding new canonical syntax
- added `UnsafeComponentThreadStatus`:
  `Running`, `Completed`, `Cancelled`, and `Failed`
- added `Instance::unsafe_component_thread_status(store, thread)` as a
  non-consuming status query for unsafe OS-owned thread handles
- added `Instance::unsafe_component_thread_try_join(store, thread)` as a
  consuming join-like operation:
  - `Ok(None)` means the child is still running
  - `Ok(Some(Completed))` or `Ok(Some(Cancelled))` removes the parent
    placeholder and releases the host `JoinHandle`
  - failed setup/start/panic records are reported after the same cleanup step
    used by the parent event loop
- retained shared-memory polling as the low-level guest fallback until a
  proposal-level thread-handle join contract exists
- T18 later adds a blocking consuming variant for embedders; this still does
  not change the guest-visible Component Model ABI

Tests:

- Rust unit coverage for non-consuming status, consuming try-join cleanup, and
  failure-after-cleanup ordering:
  `runtime::component::concurrent::tests::unsafe_os_thread_status_observes_completion_without_cleanup`
  `runtime::component::concurrent::tests::unsafe_os_thread_try_join_cleans_terminal_thread`
  `runtime::component::concurrent::tests::unsafe_os_thread_try_join_running_retains_thread`
  `runtime::component::concurrent::tests::unsafe_os_thread_try_join_reports_failure_after_cleanup`
- existing WAST probes still cover guest-visible handle creation and invalid
  cooperative resume/suspend use; a true guest-visible join WAST remains blocked
  on canonical syntax/proposal surface

### T12: Epoch-Based Child Store Cancellation

Status: done

Goal: make `subtask.cancel` capable of interrupting an already-running unsafe
OS-owned child store when Wasmtime epoch interruption is enabled.

Implementation:

- each unsafe OS-thread child store installs an epoch-deadline callback before
  component instantiation
- while no cancellation is requested, the callback returns
  `UpdateDeadline::Continue(1)` so unrelated epoch ticks do not terminate the
  child
- `ComponentThreadOsCompletion::request_cancel` records the cancel request and
  increments the child store's engine epoch when the child attached an epoch
  interrupter
- if the child start function exits with `Trap::Interrupt` while the cancel flag
  is set, the completion is recorded as `Cancelled` instead of
  `StartFailed`

Limits:

- this is still not a proposal-level preemptive cancellation contract
- it only works when the component was compiled with epoch interruption and
  reaches an epoch check
- it does not wake or interrupt a child blocked inside a host call

Tests:

- Rust unit coverage for epoch tick on cancellation and cancel-caused interrupt
  classification:
  `runtime::component::concurrent::tests::component_thread_os_completion_request_cancel_ticks_epoch`
  `runtime::component::concurrent::tests::component_thread_os_completion_epoch_interrupt_is_cancel`

### T13: Bidirectional Atomic Wait/Notify Ownership

Status: done

Goal: make guest-level synchronization explicit enough that Vibe does not have
to treat shared-memory polling as the only portable synchronization shape for
the fork-local unsafe backend.

Implementation:

- added a fork-local WAST probe where the parent and an OS-owned child
  component thread synchronize through a rebound shared memory using
  `memory.atomic.wait32` and `memory.atomic.notify`
- the child first proves it can observe and wake a parent waiter before
  publishing `ready`
- the parent then proves it can observe and wake a child waiter before
  publishing a `gate`
- the child finally proves it can observe and wake the parent waiter before
  publishing `done`
- the child defined-memory rebind now replaces both the VMContext
  `VMMemoryDefinition` pointer and the child store's host-side
  `Memory::Shared` object with the parent `SharedMemory`
- this is required because JIT atomic wait/notify libcalls look up the
  `SharedMemory` object in the current store to reach the futex wait queues;
  rebinding only the VMContext pointer shares loads/stores but not waiters
- the probe remains a normal-runner expected-fail because the cooperative
  default path cannot make progress while the parent blocks in atomic wait

Limits:

- this only covers shared-memory futex queues for rebound shared core memory
- it is not a general shared-object ownership model for resources, GC heaps,
  table growth, or direct mutable defined-global access
- cancellation of a child blocked inside `memory.atomic.wait32/64` is handled
  by T14; guest protocols should still publish ordinary wakeup flags for normal
  non-cancel synchronization

Tests:

- direct Red/Green WAST probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-bidirectional-wait-notify.wast`
- normal aggregate runner classification:
  `crates/test-util/src/wast.rs`

### T14: Atomic Wait Cancellation Wakeup

Status: done

Goal: make `subtask.cancel` capable of waking an unsafe OS-owned child thread
that is blocked inside `memory.atomic.wait32/64` on rebound shared memory.

Implementation:

- added a fork-local `WaitResult::Interrupted` state for runtime wait
  cancellation
- added `ParkingSpot::interrupt_all` and
  `SharedMemory::interrupt_atomic_waiters` to wake all waiters in a shared
  memory without exposing that as guest `memory.atomic.notify`
- `memory.atomic.wait32/64` libcalls translate interrupted waits to
  `Trap::Interrupt` instead of returning a guest wait result
- `ComponentThreadOsCompletion` now records the shared memories captured by the
  OS-thread spawn plan and interrupts their atomic waiters when
  `request_cancel()` runs
- existing cancel-caused interrupt classification records the child completion
  as `Cancelled`

Limits:

- this only targets waits on rebound shared core memory captured by the unsafe
  Component Model spawn plan
- it interrupts all waiters on those shared-memory parking spots; this is a
  fork-local diagnostic cancellation hook, not proposal-level
  `memory.atomic.notify`
- host calls other than Wasm atomic waits are still not forcibly interrupted

Tests:

- runtime parking spot interrupt:
  `runtime::vm::parking_spot::tests::atomic_wait_interrupt_all`
- completion-to-shared-memory cancel wake:
  `runtime::component::concurrent::tests::component_thread_os_completion_cancel_interrupts_atomic_waiters`
- Wasm libcall trap conversion:
  `runtime::component::concurrent::tests::wasm_atomic_wait_interruption_traps`

### T15: Per-Handle Unsafe OS Thread Cancellation

Status: done

Goal: let Vibe's embedder/runtime request cancellation of one unsafe OS-owned
thread handle without cancelling every OS-owned child in the parent guest task.

Implementation:

- added the fork-local embedder API
  `Instance::unsafe_component_thread_cancel(store, thread)`
- added a store-level `unsafe_os_thread_request_cancel` helper that validates
  the handle is an OS-owned component thread and calls the existing completion
  cancellation path
- the API is non-consuming; embedders still use
  `unsafe_component_thread_status` or `unsafe_component_thread_try_join` to
  observe the eventual terminal state
- cancellation inherits T12/T14 behavior: epoch interruption for executing
  child Wasm and atomic-wait interruption for rebound shared-memory waits

Limits:

- this is not a Component Model canonical ABI operation
- there is still no guest-visible join/cancel syntax for OS-owned thread
  handles
- cancellation remains cooperative/interrupt-based, not a bounded-time hard
  kill

Tests:

- single-handle cancellation reaches only the selected completion:
  `runtime::component::concurrent::tests::unsafe_os_thread_request_cancel_reaches_single_handle`
- non-OS handles are rejected:
  `runtime::component::concurrent::tests::unsafe_os_thread_request_cancel_rejects_non_os_thread`

### T16: Fixed-Size Shared Table Boundary

Status: done

Goal: avoid silently running the unsafe OS-thread path with shared table growth
semantics that the fork does not yet implement.

Implementation:

- captured whether each shared core table and extracted runtime table can grow
  by inspecting its table limits
- rejected growable shared core tables before unsafe preemptive spawn
- rejected growable shared runtime tables before rebinding the runtime start
  table slot
- updated Component Model threading probes that rely on the unsafe OS-thread
  path to declare fixed-size shared start tables
- T28 supersedes this broad rejection for imported runtime start-table growth
  while keeping direct defined growable shared-table starts rejected

Limits:

- this slice did not implement shared `table.grow`; T28 adds a limited imported
  runtime start-table growth path
- table entry mutation through the fixed-size shared start table remains covered
  by the existing shared-table update probe
- a future widening needs a table-allocation ownership model that keeps the
  `Table` object and all rebound `VMTableDefinition` slots synchronized across
  sibling stores

Tests:

- growable shared-table validation rejection:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_shared_table`
- existing unsafe table validation:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_start_table_only`
- WAST probes with shared start tables now use fixed-size declarations:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect*.wast`

### T17: Direct Defined Shared-Global Guard

Status: done

Goal: prevent the unsafe OS-thread path from silently executing a start function
whose own core instance defines mutable shared globals. That shape can directly
access inline VMContext global slots, so the child sibling instance would not
observe live parent updates after the initial value copy.

Implementation:

- when the unsafe OS-thread path is selected, the parent resolves the
  `thread.spawn-indirect` start function from the runtime table before spawning
  the child
- the resolved `VMFuncRef` callee vmctx is passed into
  `ComponentThreadTemplate::validate_unsafe_preemptive_spawn_indirect`
- validation originally rejected the spawn if that callee vmctx matched a
  captured core instance that owns defined shared globals; T20 refines this to
  mutable shared globals only
- imported shared-global users remain supported because their callee vmctx is
  the importing module, while their imported-global slot is rebound to the
  parent definition

Limits:

- this is a conservative runtime guard, not full instruction-level analysis
- it does not implement live direct mutable defined-global sharing
- T29 enables a fork-local post-start flush-back for direct mutable defined
  shared globals, but live concurrent access still needs a different global
  storage representation or precise access analysis

Tests:

- direct defined mutable shared-global start flush-back validation:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_mutable_shared_global_start`
- existing imported shared-global WAST remains green with the unsafe opt-in:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-global-update.wast`

### T18: Blocking Unsafe OS Thread Join

Status: done

Goal: give Vibe's embedder/runtime a consuming completion operation that does
not require shared-memory polling or repeated `try_join` polling when it is
acceptable to block the parent host thread.

Implementation:

- added `Instance::unsafe_component_thread_join(store, thread)`
- added a store-level `unsafe_os_thread_join` helper that validates the handle,
  waits for the child host `JoinHandle`, then removes the parent placeholder
- refactored terminal OS-thread cleanup so `try_join` and blocking `join` share
  the same cleanup/failure-reporting path
- added `ComponentThreadOsCompletion::join_blocking`
- if a host thread exits while the completion state is still `Running`, the
  join path records a failure instead of leaving a running placeholder without
  a join handle

Limits:

- this is still a fork-local embedder API, not a Component Model canonical ABI
  operation
- blocking join can deadlock if the child is waiting for the parent guest to
  publish a shared-memory wakeup; use it only after the protocol has made child
  completion independent, or after cancellation has been requested
- failed children are reported after parent placeholder cleanup, matching
  `unsafe_component_thread_try_join` and parent event-loop cleanup

Tests:

- blocking join waits and removes the OS-owned placeholder:
  `runtime::component::concurrent::tests::unsafe_os_thread_join_waits_and_cleans_terminal_thread`
- blocking join does not consume a running placeholder when no join handle is
  available:
  `runtime::component::concurrent::tests::unsafe_os_thread_join_rejects_running_without_join_handle`
- a host thread that exits without recording completion becomes a failure:
  `runtime::component::concurrent::tests::unsafe_os_thread_join_reports_missing_completion_record`

### T19: Component Resource And GC Boundary

Status: done

Goal: keep the unsafe OS-thread path from silently running Component Model
resource or GC shapes that would require shared store-owned resource tables,
destructors, borrow state, host handles, or GC heap/root ownership.

Implementation:

- captured a `ComponentThreadResourceState` summary in
  `ComponentThreadTemplate` from `env_component.num_resources`,
  `imported_resources`, and `defined_resource_instances`
- counted canonical options whose data model is
  `CanonicalOptionsDataModel::Gc`
- rejected unsafe preemptive `thread.spawn-indirect` before table/memory/global
  rebinding if the component declares any resources
- rejected unsafe preemptive `thread.spawn-indirect` if the component uses any
  Component Model GC canonical options

Limits:

- this does not implement shared Component Model resources
- this does not implement shared Component Model GC heaps or cross-store GC
  rooting
- resource-free linear-memory canonical ABI paths remain eligible for the
  current unsafe diagnostic shape

Tests:

- resource-bearing component rejection:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_component_resources`
- Component Model GC canonical option rejection:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_component_gc_options`

### T20: Immutable Direct Shared-Global Start Boundary

Status: done

Goal: make the direct defined shared-global guard precise enough to allow
immutable globals while keeping mutable direct global access out of the unsafe
OS-thread path.

Implementation:

- recorded mutability for each captured core shared global in
  `ComponentThreadCoreGlobal`
- changed `validate_unsafe_preemptive_spawn_indirect` to reject only start
  functions whose callee vmctx owns mutable shared globals
- kept mutable direct defined shared-global starts rejected because direct
  mutation still targets inline child VMContext storage
- allowed direct immutable defined shared-global starts, where the existing
  child defined-global initial value copy is semantically sufficient

Limits:

- this does not implement live direct mutable defined-global sharing
- immutable direct reads are safe only because the value cannot change after
  instantiation
- imported shared-global users remain the preferred mutable diagnostic shape

Tests:

- mutable direct defined shared-global flush-back validation added in T29:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_mutable_shared_global_start`
- immutable direct defined shared-global validation:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_immutable_shared_global_start`
- unsafe OS-thread WAST probe that blocks the parent and reads an immutable
  direct defined shared global from the child:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-defined-immutable-shared-global.wast`

### T21: Child Store Thread Identity Foundation

Status: done

Goal: prepare OS-owned child Stores to expose the parent-visible transient
thread-table index through `thread.index` once the canonical intrinsic can be imported by shared
start functions.

Implementation:

- added a Store-local unsafe OS-thread start scope for sibling Stores
- creates a synthetic child `GuestTask`/`GuestThread` before the child start
  function runs
- sets that synthetic thread as the child Store's current guest thread
- records the parent Store's returned transient thread-table index as the synthetic thread's
  `instance_rep`
- restores the previous current thread and deletes the synthetic task/thread
  after the start function returns, traps, or observes a pre-start cancellation

Limits:

- the synthetic child thread mirrors only the parent-visible transient index; it
  is not a general shared scheduler state

Tests:

- runtime helper lifecycle and parent handle rep:
  `runtime::component::concurrent::tests::unsafe_os_thread_start_scope_exposes_parent_thread_index_rep`
- shared canonical probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-thread-index.wast`

### T22: Consumed Public Handle Staleness

Status: done

Goal: make the fork-local public embedder APIs explicit about consumed
OS-owned thread handles.

Implementation:

- added regression coverage through real component instance thread handle
  tables, not only store-local `TableId<GuestThread>` helpers
- verified `Instance::unsafe_component_thread_try_join` consumes a terminal
  OS-owned public handle and removes it from lookup
- verified `Instance::unsafe_component_thread_join` consumes a terminal
  OS-owned public handle and removes it from lookup
- verified immediate status/cancel/try-join attempts against the consumed
  handle return an unknown unsafe OS-thread handle error

Limits:

- this does not make guest-visible thread handles durable identities
- component handle table slots are still reusable after cleanup; Vibe must not
  retain a numeric handle value and assume it can never refer to a later handle

Tests:

- public `try_join` consumes the handle and rejects immediate stale status/cancel:
  `runtime::component::concurrent::tests::unsafe_component_thread_try_join_rejects_stale_public_index`
- public blocking `join` consumes the handle and rejects immediate stale
  `try_join`:
  `runtime::component::concurrent::tests::unsafe_component_thread_join_rejects_stale_public_index`

### T23: Public OS-Handle Lookup Ignores Cooperative Collisions

Status: done

Goal: avoid rejecting a valid OS-owned public handle just because another
runtime component instance has the same numeric cooperative thread handle.

Implementation:

- changed fork-local public unsafe handle lookup to first collect matching
  guest-thread handle-table entries, then filter them by
  `GuestThreadState::UnsafeOsSpawned`
- non-OS guest thread handles no longer participate in OS-owned ambiguity
  detection
- if matching thread handles exist but none are OS-owned, the public API still
  reports that the handle is not an unsafe OS-owned thread
- multiple matching OS-owned handles remain rejected as ambiguous

Limits:

- this does not solve ambiguous numeric handles between two OS-owned runtime
  component instances
- Vibe should still prefer returning/using handles from one known runtime
  component instance for the unsafe diagnostic backend

Tests:

- public unsafe status ignores a cooperative handle collision and resolves the
  OS-owned handle:
  `runtime::component::concurrent::tests::unsafe_component_thread_lookup_ignores_non_os_index_collisions`

### T24: Public OS-Handle Reuse And OS-Owned Ambiguity

Status: done

Goal: make the remaining public unsafe handle identity edge cases explicit for
Vibe.

Implementation:

- added regression coverage for two OS-owned runtime component instances that
  expose the same numeric public thread handle
- verified that public unsafe lookup rejects that shape as ambiguous
- added regression coverage for handle-table reuse after a consuming join
- verified that a consumed numeric handle value can be reused by a later
  OS-owned thread in the same runtime component instance, and then resolves to
  the later thread

Limits:

- the public unsafe APIs still accept only a numeric thread handle, not a
  runtime component instance discriminator
- Vibe must treat a consumed handle value as invalid for the old thread even if
  the same number later resolves to a new OS-owned thread

Tests:

- ambiguous OS-owned numeric handles are rejected:
  `runtime::component::concurrent::tests::unsafe_component_thread_lookup_rejects_ambiguous_os_indices`
- consumed numeric handles can be reused for later OS-owned threads:
  `runtime::component::concurrent::tests::unsafe_component_thread_numeric_index_can_be_reused_after_join`

### T25: Fork-Local Completion Reports For Vibe Join Lowering

Status: done

Goal: add a host diagnostic completion report without turning child
setup/start/panic failures into host API errors.

Implementation:

- added `UnsafeComponentThreadCompletion` as a fork-local completion report
  with `status()`, `failure_message()`, and `into_failure_message()`
- added `Instance::unsafe_component_thread_try_join_completion(store, thread)`
  and `Instance::unsafe_component_thread_join_completion(store, thread)`
- kept the existing `unsafe_component_thread_try_join` and
  `unsafe_component_thread_join` behavior unchanged: child failures still
  become `Err(...)` after placeholder cleanup
- completion-report diagnostics consume terminal OS-owned indices exactly like
  the legacy consuming diagnostics

Limits:

- this is still not a Component Model canonical ABI intrinsic
- T32 later clarified that Vibe must not lower this as guest-visible
  `thread.join`; it is a host diagnostic report only
- blocking completion-report join can deadlock under the same parent/child
  protocol shapes as the legacy blocking join

Tests:

- child panic is preserved as a `Failed` completion value:
  `runtime::component::concurrent::tests::unsafe_component_thread_try_join_completion_reports_failure_as_value`
- cancellation is returned as a completion value:
  `runtime::component::concurrent::tests::unsafe_component_thread_join_completion_returns_cancelled_as_value`

### T26: Shared `canon thread.index` For Shared Start Functions

Status: done

Goal: let a shared start function import `canon thread.index` and observe the
same parent-visible handle returned by `thread.spawn-indirect`.

Implementation:

- patched the local `wasmparser` fork so `canon thread.index` produces a shared
  `(func (result i32))` core function type
- kept a fork-local validator/runtime compatibility path for `thread.index`
  imports whose only mismatch is sharedness, so older unshared import probes
  continue to instantiate
- when the parent shared start table selects a parent VMFuncRef, resolve that
  function to `(runtime core instance, func index)` before spawning
- in the OS-owned child Store, call the sibling Store's counterpart VMFuncRef
  instead of reusing the parent table's VMFuncRef, so `thread.index` reads the
  child Store's current unsafe OS-thread scope

Limits:

- this is a fork-local compatibility path, not an upstream Canonical ABI change
- the compatibility exception is limited to an import named `thread.index` with
  shape `(func (result i32))`
- at this point the start-table contract was still fixed-size; T28 later allows
  limited imported runtime start-table growth
- the WAST probe remains a normal-runner expected-fail because it still needs
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` for OS-thread execution; the
  canonical boundary itself is no longer the blocker

Tests:

- shared start function observes the parent-visible transient thread-table index:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-thread-index.wast`
  with unsafe opt-in
- parent shared table updates still select the updated start function:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-table-update.wast`
- unshared import compatibility remains green:
  `tests/all/component_model/func.rs::thread_index_via_instantiation_sync`

### T27: Terminal Guest Join Result Shape

Status: superseded by T32

Goal: turn the fork-local completion report into a Vibe-lowerable terminal
guest join shape without pretending that upstream has a `thread.join`
canonical intrinsic.

Implementation:

- added `UnsafeComponentThreadJoinCode` with stable fork-local numeric codes:
  `Completed = 0`, `Cancelled = 1`, and `Failed = 2`
- added `UnsafeComponentThreadJoinResult` with `code()`, `failure_message()`,
  and `into_failure_message()`
- added `UnsafeComponentThreadCompletion::into_join_result()` so completion
  reports can be explicitly narrowed to terminal join values
- added `Instance::unsafe_component_thread_try_join_result(store, thread)` and
  `Instance::unsafe_component_thread_join_result(store, thread)` as the
  Vibe-facing lowering helpers
- kept `Running` out of the join result contract; nonblocking join reports a
  running child as `Ok(None)`

Superseded finding:

- after re-reading the current Component Model Canonical ABI, the value returned
  by `thread.spawn-*` is a transient component thread-table index, not a stable
  join handle
- Vibe must not lower a guest-visible `thread.join` from the canonical spawn
  return value
- the T27 APIs were removed in T32; host completion reports remain diagnostics
  only

Limits:

- this is still a fork-local embedder API, not a Component Model canonical ABI
  intrinsic
- the latest shared-everything/component-model thread proposal shape still has
  `thread.spawn-*`, `thread.index`, and `thread.available-parallelism`, but no
  canonical `thread.join`
- failure messages are diagnostic strings for local lowering, not stable
  proposal-level error payloads
- consumed numeric handle reuse and ambiguous OS-owned handle restrictions still
  apply

Tests:

- removed in T32 together with the terminal guest join result APIs

### T28: Imported Runtime Start-Table Growth

Status: done

Goal: allow the unsafe OS-thread path to use a growable shared start table when
the running start function observes that table through an imported runtime table
slot, while still rejecting direct defined-table ownership shapes whose inline
VMContext `VMTableDefinition` can go stale.

Implementation:

- narrowed the growable shared-table validation guard from "all growable shared
  tables are rejected" to "direct defined growable shared-table owner starts
  are rejected"
- kept growable shared tables rejected when the start function owner VMContext
  is unknown
- allowed growable shared runtime start tables when the runtime table is the
  selected `thread.spawn-indirect` start table
- kept the child runtime table slot rebound to the parent `VMTableImport`, so
  parent `table.grow` updates are observed through the parent's
  `VMTableDefinition`

Limits:

- this does not make direct defined shared-table growth sound for child code;
  child defined-table VMContext slots are still inline copies
- start functions defined in the same core instance that owns a growable shared
  table remain rejected
- growable shared table owners that define functions were allowed here but are
  rejected later in T30, because those owner functions can directly observe the
  inline child `VMTableDefinition`
- arbitrary shared table ownership, concurrent growth synchronization, and
  table growth from child-owned stores are not yet proposal-level contracts

Tests:

- imported growable runtime start table is allowed when the start function is
  defined in a different core instance:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_growable_imported_start_table`
- direct defined growable shared-table start remains rejected:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_defined_table_start`
- parent grows the shared start table and spawns the newly allocated index:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-table-grow.wast`
  with unsafe opt-in

### T29: Direct Mutable Defined Shared-Global Flush-Back

Status: done

Goal: allow a shared start function that is defined in the same core instance
as a mutable shared global to write that global directly, while keeping the
limited fork-local semantics explicit.

Implementation:

- removed the unsafe preemptive validation rejection for start functions whose
  owner core instance defines mutable shared globals
- kept the existing child initial-value copy for defined shared globals and
  parent-definition rebinding for imported shared globals
- added `ComponentThreadSpawnPlan::flush_direct_defined_shared_globals_from`
  to copy mutable defined shared-global values from the child start function
  owner back to the parent definitions after the start function returns
- called that flush after `call_unchecked` and before terminal completion is
  recorded, so writes before traps/cancellation interrupts are treated like
  normal Wasm side effects in this diagnostic shape

Limits:

- this is not live shared storage; parent visibility is a post-start flush-back
  point
- only mutable shared globals owned by the start function's runtime core
  instance are flushed
- arbitrary concurrent direct defined-global sharing, GC-reference globals, and
  host-blocked cancellation remain outside the safe contract

Tests:

- direct mutable defined shared-global starts pass validation:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_defined_mutable_shared_global_start`
- child direct defined mutable shared-global write is flushed to the parent
  imported global:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-defined-mutable-shared-global.wast`
  with unsafe opt-in

### T30: Growable Shared Table Owner Function Guard

Status: done

Goal: keep T28's imported runtime start-table growth path, but reject another
direct defined-table ownership shape where the start function is in a different
core instance yet can call functions defined by the growable table owner.

Implementation:

- captured whether each shared table owner core module defines functions in
  `ComponentThreadCoreTable`
- kept table-only growable shared table owners allowed for the imported runtime
  start-table dispatch shape
- rejected growable shared table owner core instances that define functions,
  because those functions can directly observe or grow the owner's inline child
  `VMTableDefinition`
- kept the existing direct-owner rejection when the selected start function is
  defined in the growable table owner's core instance

Limits:

- this is a conservative reachability guard, not instruction-level table-use
  analysis
- it does not synchronize child inline `VMTableDefinition` slots after growth
- it intentionally preserves the table-only imported runtime start-table growth
  diagnostic shape from T28

Tests:

- growable imported start-table owner with no functions remains allowed:
  `runtime::component::threading::tests::unsafe_preemptive_validation_allows_growable_imported_start_table`
- growable shared table owner that defines a helper function is rejected even
  when the selected start function is owned by a different core instance:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_table_owner_functions`
- direct growable table owner starts remain rejected:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_defined_table_start`

### T31: Growable Shared Table Owner Function WAST Probe

Status: done

Goal: make T30's conservative growable shared-table owner-function rejection
observable through the actual `thread.spawn-indirect` runtime path, not only
the validation unit test.

Implementation:

- added an unsafe opt-in WAST where the selected shared start function is
  defined by a different core instance, but calls a helper function defined by
  the growable shared table owner
- asserted that `thread.spawn-indirect` rejects the shape before spawning a
  sibling Store
- registered the probe as a normal-runner expected-fail because the rejection
  is only observable when `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` is
  active

Limits:

- this does not widen table semantics; it only pins the current rejection
  boundary at the runtime invocation layer
- the guard remains conservative and does not inspect whether the owner helper
  actually grows or reads the table in all possible code paths

Tests:

- runtime WAST rejection probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-growable-table-owner-func-rejected.wast`
  with unsafe opt-in
- validation unit test remains the direct guard:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_growable_table_owner_functions`

### T32: Align Spawn Return With Transient Thread Index Semantics

Status: done

Goal: stop treating the canonical `thread.spawn-*` return value as a stable
guest-level join handle.

Implementation:

- updated `mizchi/wasmtime#1` with the corrected reading:
  `thread.spawn-*` returns the component instance's transient thread-table
  index, and completion/join belongs in producer-generated shared state plus
  wait/notify if a language needs it
- removed `UnsafeComponentThreadJoinCode`,
  `UnsafeComponentThreadJoinResult`,
  `UnsafeComponentThreadCompletion::into_join_result`, and the public
  `Instance::unsafe_component_thread_{try_join_result,join_result}` helpers
- kept fork-local cancel/status/try-join/join/completion APIs as host
  diagnostics for the unsafe backend, with docs warning that they are not Vibe
  guest ABI values
- updated `thread.index` / spawn docs and WAST comments to call the value a
  transient thread-table index instead of a stable handle

Limits:

- the unsafe backend still retains parent placeholders until parent cleanup or
  explicit host diagnostic cleanup joins terminal host threads
- Vibe-level join is now explicitly out of the Wasmtime fork API; the next Vibe
  layer should generate a trampoline that stores completion in shared state and
  notifies waiters

Tests:

- existing host diagnostic cleanup tests continue to cover stale index and
  numeric reuse behavior:
  `runtime::component::concurrent::tests::unsafe_component_thread_try_join_rejects_stale_public_index`,
  `runtime::component::concurrent::tests::unsafe_component_thread_join_rejects_stale_public_index`,
  `runtime::component::concurrent::tests::unsafe_component_thread_numeric_index_can_be_reused_after_join`
- runtime WAST cleanup/reuse probe remains:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-cleanup-reuses-handles.wast`

### T33: Trampoline-Managed Vibe Completion Probe

Status: done

Goal: pin the Vibe-level completion direction without reintroducing a stable
guest join handle.

Implementation:

- added a WAST probe where the parent drops both `thread.spawn-indirect` return
  values
- modeled the generated language-runtime trampoline as the shared start
  function:
  - mark the slot running
  - call a shared worker function
  - write the result
  - mark the slot completed
  - notify waiters on the slot state word
- parent waits on the shared state word and reads results only after completion
  is published
- registered the probe as normal-runner expected-fail because it requires the
  unsafe OS-thread opt-in
- registered the probe as pooling-unsupported with the other shared-memory
  OS-thread probes

Limits:

- this is a normal-return completion shape only; failure/cancellation mapping is
  still a Vibe runtime design task
- the probe intentionally does not use `thread.index` or the canonical spawn
  return value for completion

Tests:

- runtime WAST completion probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-completion.wast`
  with unsafe opt-in

### T34: Trampoline-Managed Terminal Status Probe

Status: done

Goal: map Vibe-level terminal statuses into the trampoline-owned shared-state
protocol without using the canonical spawn return value as a join handle.

Implementation:

- added a WAST probe with three spawned tasks:
  - completed: writes terminal code `0` and result payload
  - cancelled: observes a producer-owned cancel-request flag and writes
    terminal code `1`
  - failed-as-value: maps a producer-level worker error into terminal code `2`
- parent drops every `thread.spawn-indirect` return value
- parent waits on each slot's state word, then reads the terminal code/result
  from shared memory
- registered the probe as normal-runner expected-fail and pooling-unsupported
  with the other unsafe OS-thread shared-memory probes

Limits:

- this covers producer-level failure-as-value and cooperative cancellation; it
  does not catch Wasm traps, host panics, or host-blocked cancellation
- trap/cancel interop with the host diagnostic completion record remains a
  separate design point

Tests:

- runtime WAST terminal-status probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-status.wast`
  with unsafe opt-in

### T35: Trampoline Trap Boundary Probe

Status: done

Goal: pin that real Wasm traps in an OS-owned child are host diagnostic
failures, not producer-generated Vibe terminal status values.

Implementation:

- added a WAST probe where the spawned trampoline marks its shared slot
  `running` and then executes `unreachable`
- asserted that parent cleanup surfaces the child start trap as
  `unsafe Component Model OS thread failed`
- intentionally left the shared terminal slot untouched; only generated
  producer code may write Vibe-level `completed/cancelled/failed-as-value`
  terminal codes
- registered the probe as normal-runner expected-fail and pooling-unsupported
  with the other unsafe OS-thread shared-memory probes

Limits:

- this does not catch traps inside guest code or translate them into the
  trampoline protocol
- host diagnostics still record the child failure, but those diagnostics remain
  outside Vibe guest ABI
- if Vibe wants failed-as-value semantics, its generated trampoline must catch
  or avoid the source-level failure before it becomes a real Wasm trap

Tests:

- runtime WAST trap-boundary probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-trap-boundary.wast`
  with unsafe opt-in

### T36: Trampoline In-Flight Cancellation Wakeup Probe

Status: done

Goal: prove that Vibe-level in-flight cancellation can stay inside the
producer-owned trampoline protocol instead of depending on the canonical spawn
return value or fork-local host diagnostics.

Implementation:

- added a WAST probe where the parent drops the `thread.spawn-indirect` return
  value and waits until the child trampoline publishes `running`
- child trampoline blocks in `memory.atomic.wait32` on the slot's shared cancel
  flag
- parent writes the cancel flag, wakes the child with `memory.atomic.notify`,
  and waits for the trampoline-owned terminal slot
- child publishes terminal code `1 = cancelled` through the same shared state
  used by the completion/status probes
- registered the probe as normal-runner expected-fail and pooling-unsupported
  with the other unsafe OS-thread shared-memory probes

Limits:

- this is cooperative, producer-owned cancellation, not a forced host-level
  stop guarantee
- the fork-local host cancel/status/join diagnostics remain outside Vibe guest
  ABI
- child code that traps before the generated trampoline writes the terminal
  slot still follows the T35 host-diagnostic failure boundary

Tests:

- runtime WAST in-flight cancellation probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-cancel-wakeup.wast`
  with unsafe opt-in

### T37: Positive Shared Ownership Subset

Status: done

Goal: turn the existing table/global/resource guards into an explicit positive
subset that Vibe may rely on, and reject growable shared-table shapes outside
that subset.

Implementation:

- added `ComponentThreadSharedOwnershipSubset` as a validation report for the
  fork-local unsafe OS-thread path
- changed `validate_unsafe_preemptive_spawn_indirect` to delegate through the
  positive subset validator
- the report counts:
  - rebound shared core memories
  - shared runtime start-table slots
  - fixed-size core shared tables
  - growable imported runtime start tables
  - shared global definitions
  - direct mutable defined shared-global flush-back slots
- tightened growable shared-table validation so a growable shared core table is
  allowed only if it backs the `thread.spawn-indirect` runtime start table,
  the start function is not defined in that table owner, and the table owner
  core instance defines no functions
- added a Red/Green unit test for an unrelated growable shared table: even if
  its owner defines no functions, it is now rejected as outside the Vibe shared
  ownership subset

Limits:

- this still does not implement live direct defined table/global sharing
- the subset report is fork-local validation/debug surface, not an upstream
  proposal-level contract
- resources, Component Model GC canonical options, mutable unshared globals,
  and arbitrary growable shared tables remain rejected

Tests:

- positive subset allow/reject tests:
  `runtime::component::threading::tests::unsafe_preemptive_validation_*`
- new unrelated growable table rejection:
  `runtime::component::threading::tests::unsafe_preemptive_validation_rejects_unowned_growable_shared_table`

### T38: Consolidated Vibe Runtime ABI Probe

Status: done

Goal: consolidate the separate trampoline completion/status/cancellation probes
into a single Vibe-shaped runtime ABI probe.

Implementation:

- added a WAST probe with one generated trampoline and a fixed shared slot
  layout:
  - `state`
  - terminal code: `0 = completed`, `1 = cancelled`, `2 = failed`
  - result/error payload
  - input
  - producer-owned cancel flag
  - runtime mode
- fixed the ABI offsets used by this probe:

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

- parent drops every canonical `thread.spawn-indirect` return value
- parent joins by waiting on trampoline-owned shared slots
- the same trampoline publishes:
  - normal completed results for two slots
  - failed-as-value for one slot
  - in-flight cooperative cancellation for one slot
- parent aggregates only completed payloads and checks terminal codes for every
  slot
- registered the probe as normal-runner expected-fail and pooling-unsupported
  with the other unsafe OS-thread trampoline probes

Limits:

- this still leaves real Wasm traps on the T35 host-diagnostic boundary
- this does not add a proposal-level join/cancel handle; it deliberately avoids
  the canonical spawn return value
- this is an ABI-shaped validation probe, not a performance benchmark

Tests:

- runtime WAST Vibe ABI probe:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi.wast`
  with unsafe opt-in

### T39: ABI-Shaped Speedup Probe

Status: done

Goal: benchmark the same CPU workload through the consolidated Vibe slot ABI,
not only the older done-counter speedup shape.

Implementation:

- added ABI-shaped serial and parallel WAST probes:
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast`
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast`
- both probes use the same four-slot 50,000,000-iteration deterministic CPU
  workload and assert checksum `1106140682`
- the serial probe executes the workload sequentially in the parent but writes
  results through the same `state/code/payload/input/cancel/mode` slot layout
- the parallel probe spawns four `thread.spawn-indirect` children and each
  generated trampoline publishes `completed` terminal status plus payload into
  its slot
- parent aggregation reads completed payloads from the ABI slots and ignores
  every canonical spawn return value
- added ignored CLI timing hook:
  `tests/all/cli_tests.rs::run_component_thread_vibe_abi_speedup_probe`
- registered the parallel probe as normal-runner expected-fail and both probes
  as pooling-unsupported

Local result:

- direct local CLI timing on 2026-06-02 with `available_parallelism=10`:
  serial `0.22s` real, parallel `0.08s` real, about `2.75x`
- the ignored timing test also passed; its serial measurement is slower in the
  test harness environment, so direct `/usr/bin/time` remains the baseline used
  here

Tests:

- direct WAST checksum/timing probes with unsafe opt-in:
  `target/debug/wasmtime wast ... thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast`
  `target/debug/wasmtime wast ... thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast`
- harness behavior:
  `cargo +1.93.0 test --test wast thread-spawn-indirect-os-trampoline-vibe-abi-speedup`
- ignored timing probe:
  `cargo +1.93.0 test --test all run_component_thread_vibe_abi_speedup_probe -- --ignored --nocapture`

### T40: Proposal Conformance Map

Status: done

Goal: make it clear, before wiring Vibe, which parts of the current fork follow
the shared-everything threads / Component Model proposal surface and which parts
are fork-local implementation or Vibe ABI choices.

Implementation:

- added `docs/experimental-shared-everything-conformance.md`
- classified the current backend into:
  - proposal-defined behavior
  - proposal-aligned local subsets
  - fork-local diagnostics and Vibe ABI
  - gaps that must not be exposed as `ComponentModelShared`
- documented that `thread.spawn-indirect` is a proposal-defined entry point,
  but the current unsafe OS-thread path is fork-local because it uses
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1` and a shared start
  function/table shape instead of a parser-visible `shared?` immediate
- documented that Vibe-level join/completion/cancellation are generated
  shared-memory ABI slots, not canonical spawn return semantics
- linked the conformance map from the fork goal and Vibe backend contract

Tests:

- documentation-only change
- verification command:
  `git diff --check`

### T41: Vibe ABI Drift Guard And Manual Smoke

Status: done

Goal: keep the Vibe-generated slot ABI, local WAST fixtures, and fork-local
documentation from drifting before wiring real Vibe source lowering through the
unsafe backend.

Implementation:

- added `tests/all/component_thread_abi.rs` with a single source of truth for
  the Vibe runtime slot shape used by the Wasmtime fork probes:
  - slot stride `32`
  - `state` offset `0`
  - `terminal_code` offset `4`
  - `payload` offset `8`
  - `input` offset `12`
  - `cancel` offset `16`
  - `mode` offset `20`
  - `worker_func` offset `24`
- the ABI guard checks the consolidated Vibe ABI WAST fixtures and the
  contract/conformance/speedup/plan docs for the same fixed layout
- added `tests/all/cli_tests.rs::run_component_thread_vibe_abi_probe` as an
  ignored manual smoke test for the non-benchmark consolidated ABI probe
- refactored the unsafe Component Model thread CLI helper so timing probes and
  non-timing ABI smoke probes share the same unsafe WAST execution path

Limits:

- this does not make the slot ABI a Wasmtime runtime contract; it is still a
  fork-local Vibe integration contract
- this does not compare against `vibe-lang` generated output directly, because
  the Wasmtime test suite should not depend on the sibling repository

Tests:

- Red/Green ABI drift test:
  `cargo +1.93.0 test --test all vibe_runtime_slot_abi -- --nocapture`
- manual unsafe ABI smoke:
  `cargo +1.93.0 test --test all run_component_thread_vibe_abi_probe -- --ignored --nocapture`

### T42: Vibe Generated Component Smoke

Status: done

Goal: prove that the `vibe-lang` `feat/thread` generated component reaches the
fork-local unsafe OS-thread path, and record the remaining semantic boundary
before using it as a real parallel Vibe workload.

Implementation:

- updated `vibe-lang` to emit `__heap_ptr` as a `(shared mut i32)` global for
  shared-memory thread backends that use the heap global
- added a Vibe runtime compile test that parses the generated core wasm global
  section and checks the first global flags are `0x03` (`shared mut`)
- updated `wasmtime run` so the CLI installs a fork-local unsafe component
  thread store-data factory for its own non-`() Store<Host>` by cloning `Host`
- documented that this is an ABI smoke, not allocator safety: Vibe's current
  heap bump path still needs atomic allocation, per-thread arenas, or another
  explicit shared-heap ownership scheme before multi-threaded guest allocation
  is correct
- documented that child stores clone configured store data and install the
  fork-local cancellation/epoch hooks, but embedders should not assume all
  unrelated parent `Store` runtime settings are inherited automatically

Tests:

- Vibe:
  `moon test src/runtime_compile --target native --filter '*component unsafe exports shared indirect function table*'`
- Vibe:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Vibe:
  `moon test src/runtime_compile --target native --filter '*component*'`
- Vibe:
  `moon test src/runtime_compile --target native --filter '*Threads*'`
- Vibe:
  `moon test src/codegen --target native --filter '*thread*'`
- Vibe:
  `moon check src/codegen --target native`
- Vibe:
  `moon check src/runtime_compile --target native`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `cargo +1.93.0 build --release -p wasmtime-cli`
- Wasmtime fork:
  `cargo +1.93.0 test -p wasmtime spawn_plan_uses_non_unit_store_data_factory`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
- hygiene:
  `git diff --check`

### T43: Vibe Shared Atomic Heap Cursor

Status: done

Goal: move the Vibe shared-thread backend beyond validator-only shared
`__heap_ptr` support by making the ordinary bump allocation helpers reserve
heap space through a shared-memory atomic cursor.

Implementation:

- added a Vibe `i32.atomic.rmw.add` emitter and made the component unsafe
  shared-thread compile test require atomic RMW add in addition to wait/notify
- added `shared_heap_cursor_offset` to Vibe codegen context
- reserved a 4-byte shared heap cursor word after static string data and before
  coverage/heap data for shared-memory thread backends
- initialized that cursor in the wasm data section, even when no string data
  segment exists
- changed fixed-size, dynamic-size, and unchecked linear-memory allocation
  helpers to use `i32.atomic.rmw.add` for this backend
- changed `cabi_realloc` for the current component string-wrapper shape to use
  the shared cursor for align=4 allocations
- changed the HTTP host string helper's direct bump path to use the shared
  cursor in this backend
- skipped ordinary `heap_local` to `__heap_ptr` sync for the atomic backend so
  child functions do not write stale local heap pointers back to the shared
  global
- taught the local single-threaded wasm evaluator to skip and approximate
  `i32.atomic.rmw.add`

Limits:

- this does not make `__heap_ptr` the live allocator cursor; the shared-memory
  cursor word is authoritative for the atomic backend
- RC/free-list allocation remains outside this backend's thread-safe heap
  contract
- tip-based in-place realloc/grow optimizations still need separate probes
  before parallel allocating workloads can rely on them
- `cabi_realloc` is only covered for the current align=4 component wrapper
  usage; arbitrary canonical ABI alignments need a CAS loop or another
  alignment-safe allocation protocol. This is later closed by T54 for the
  shared-thread backend's `cabi_realloc`.

Tests:

- Red/Green allocation opcode check:
  `moon test src/runtime_compile --target native --filter '*lowers component unsafe shared Threads spawn wait with wait notify*'`
- Vibe generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Vibe component table/global shape:
  `moon test src/runtime_compile --target native --filter '*component unsafe exports shared indirect function table*'`
- Vibe thread compile tests:
  `moon test src/runtime_compile --target native --filter '*Threads*'`
- Vibe component compile tests:
  `moon test src/runtime_compile --target native --filter '*component*'`
- Vibe codegen thread tests:
  `moon test src/codegen --target native --filter '*thread*'`
- Vibe evaluator thread tests:
  `moon test src/tests --target native --filter '*Threads component unsafe shared*'`
- Vibe checks:
  `moon check src/codegen --target native`
- Vibe checks:
  `moon check src/runtime_compile --target native`
- Vibe checks:
  `moon check src/tests --target native`
- Vibe metadata:
  `moon info`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`

### T44: Vibe Multi-Child Atomic Allocation Probe

Status: done

Goal: prove the Vibe-generated component can make OS-owned child threads
contend on the shared atomic heap cursor, instead of only proving that the
parent-side component shape validates and spawns.

Implementation:

- added a fork-local Vibe diagnostic task mode selected by
  `Threads::spawn("alloc-probe", ch)`
- kept non-worker spawns as the existing no-payload completion path
- changed the generated `__vibe_thread_start` trampoline to inspect the task
  slot `mode` field and, for `alloc-probe`, run a child-side loop that performs
  repeated 16-byte `i32.atomic.rmw.add` reservations against the shared heap
  cursor
- wrote diagnostic words into each reserved block and accumulated the allocated
  pointers into the task slot `payload`
- added a non-component evaluator fallback so the local single-threaded wasm
  evaluator can still validate the generated slot protocol
- extended the generated component smoke to export `thread-alloc-probe` and
  `thread-alloc-many-probe`

Limits:

- `alloc-probe` is a diagnostic compile-time mode, not Vibe language worker
  dispatch semantics
- the probe validates atomic cursor contention from sibling Stores, not RC,
  free-list reuse, arbitrary object invariants, or host helpers that still
  derive allocation state from `__heap_ptr`
- checksum values are diagnostic and may vary with code shape or allocation
  interleaving; the contract is successful completion with a positive payload

Tests:

- Vibe thread library mode tests:
  `moon test src/x/threads --target native`
- Vibe evaluator allocation probes:
  `moon test src/tests --target native --filter '*alloc*probe*'`
- Vibe generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Vibe thread compile tests:
  `moon test src/runtime_compile --target native --filter '*Threads*'`
- Vibe codegen thread tests:
  `moon test src/codegen --target native --filter '*thread*'`
- Vibe evaluator thread tests:
  `moon test src/tests --target native --filter '*Threads component unsafe shared*'`
- Vibe checks:
  `moon check src/codegen --target native`
- Vibe checks:
  `moon check src/runtime_compile --target native`
- Vibe checks:
  `moon check src/tests --target native`
- Vibe checks:
  `moon check src/x/threads --target native`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-alloc-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `8658944` in the latest local run
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-alloc-many-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned a positive diagnostic checksum, `135692288`, in the latest local run

### T45: Vibe Worker Function Dispatch Probe

Status: done

Goal: move beyond diagnostic `alloc-probe` mode and prove the generated Vibe
component can dispatch a real Vibe top-level worker function from the OS-owned
child trampoline, then publish the worker result through the Vibe slot payload.

Implementation:

- extended the generated Vibe slot ABI with `worker_func` at offset `24`
- lowered `Threads::spawn("worker", ch)` to store a fork-local worker-function
  code in the slot while keeping `alloc-probe` selected by the existing `mode`
  field
- compiled string-literal worker targets as shared core functions when they are
  capture-free top-level Vibe functions with the temporary shape
  `ThreadChannel[Int] -> Int`
- extended the child `__vibe_thread_start` trampoline to dispatch those shared
  worker functions and store the untagged `Int` result into the slot payload
- kept the worker definition non-exported and added a Vibe DCE root for
  `Threads::spawn("name", ...)` string literals, avoiding component export/lift
  of the shared worker function
- kept `thread-probe` on the no-worker path and added `thread-worker-probe` for
  the real worker path

Limits:

- worker lookup is currently string-literal only
- the supported worker shape is only capture-free top-level
  `ThreadChannel[Int] -> Int`
- this is not yet a general Vibe function-value, closure, typed-channel, or heap
  object contract
- the generated component must not export the shared worker function directly;
  exported shared workers currently conflict with component lifting expectations

Tests:

- DCE worker root:
  `moon test src/frontend --target native --filter '*thread spawn worker*'`
- Vibe evaluator worker result:
  `moon test src/tests --target native --filter '*worker function result*'`
- Vibe generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `0`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-worker-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `168`, the current tagged Vibe `Int` representation of `42`

### T46: Vibe Worker Name Resolution Contract

Status: done

Goal: prevent typos in `Threads::spawn("name", ch)` from silently becoming
no-op thread tasks now that string-literal worker dispatch can execute real Vibe
worker functions.

Implementation:

- introduced the reserved no-worker task name `"noop"` for smoke probes
- kept `"alloc-probe"` as the reserved diagnostic allocation task mode
- changed worker resolution so any other string-literal spawn target must name a
  known top-level Vibe worker function
- kept the existing worker checks for capture-free top-level functions and the
  temporary `ThreadChannel[Int] -> Int` shape
- updated the no-worker smoke fixtures to use `"noop"` explicitly

Limits:

- name resolution is still string-literal only
- `"noop"` and `"alloc-probe"` remain fork-local diagnostic names, not public
  Vibe thread API

Tests:

- missing worker rejection:
  `moon test src/runtime_compile --target native --filter '*missing component unsafe thread worker*'`
- Vibe thread compile tests:
  `moon test src/runtime_compile --target native --filter '*Threads*'`
- Vibe evaluator thread tests:
  `moon test src/tests --target native --filter '*Threads component unsafe shared*'`
- Vibe generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `0`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-worker-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `168`

### T47: Vibe Worker Channel Input Probe

Status: done

Goal: extend the T45 generated worker-dispatch probe so the OS-owned child
worker uses the channel handle passed through the Vibe slot input field instead
of ignoring it.

Implementation:

- kept the supported worker shape constrained to non-exported, capture-free
  top-level `ThreadChannel[Int] -> Int` functions
- added a Vibe evaluator probe where `Threads::spawn("worker_recv", ch)` starts
  a worker that evaluates `Threads::recv(ch) + 21`
- added the same `thread-worker-channel-probe` export to the generated component
  smoke fixture
- broadened the temporary Vibe `Threads::send` checker contract to accept
  `Int` payloads as well as the existing `String` payloads
- changed the temporary Vibe `Threads::recv` checker contract to return an
  unresolved payload type so surrounding expression context can bind it to
  `Int` for numeric worker probes or to `String` for the existing string
  roundtrip probes

Limits:

- this is still not a typed channel abstraction; the handle is an `Int`, and the
  payload type is inferred from use rather than stored in the channel type
- the worker ABI still returns only an `Int` through the current slot payload
- the probe validates an `Int` payload path only; general heap objects,
  structured payloads, and closure/function-value workers remain out of scope

Tests:

- Vibe checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Vibe evaluator worker channel result:
  `moon test src/tests --target native --filter '*worker reads channel*'`
- Vibe generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `target/release/wasmtime compile -Ccache=n -W threads=y -W shared-memory=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run -Ccache=n -W threads=y -W shared-memory=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y --invoke 'thread-worker-channel-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `168`, the current tagged Vibe `Int` representation of `42`
- Wasmtime fork regression smokes on the same generated component:
  `thread-probe(0)` returned `0`, `thread-worker-probe(0)` returned `168`,
  `thread-alloc-probe(0)` returned `8716288`, and
  `thread-alloc-many-probe(0)` returned `135921664`

### T48: Vibe ThreadChannel Payload Type Link

Status: done

Goal: remove the type-layer hole left by T47 where `Threads::recv(ch)` returned
an unconstrained fresh type without being tied to the payload type sent through
the same channel.

Implementation:

- added `ThreadChannel[T]` as a Vibe checker builtin type
- changed `Threads::channel_new(cap)` to return `ThreadChannel[T]` with a fresh
  payload type
- changed `Threads::send(ch, payload)` to require `ch: ThreadChannel[T]`, accept
  only the current experimental `Int`/`String` payload set, and unify `payload`
  with `T`
- changed `Threads::recv(ch)` to require `ch: ThreadChannel[T]` and return `T`
- changed `Threads::spawn("name", ch)` to require the second argument to be a
  `ThreadChannel[T]` instead of a bare `Int`
- updated the worker probes to annotate worker channel parameters as
  `ThreadChannel[Int]`; codegen still lowers the handle as the existing tagged
  `i64` ABI value

Limits:

- `ThreadChannel[T]` is a Vibe type-layer contract, not a new Wasmtime proposal
  type
- the runtime ABI still stores and passes channel handles as tagged `Int`
  values
- payload lowering is still limited to the currently implemented `Int` and
  `String` paths; structured heap objects need separate runtime/GC safety work

Tests:

- checker builtin contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- rejected payload mismatch:
  `moon test src/runtime_compile --target native --filter '*inconsistent Threads channel payload type*'`
- worker channel evaluator:
  `moon test src/tests --target native --filter '*worker reads channel*'`
- generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Wasm tools:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
  `target/release/wasmtime compile -Ccache=n -W threads=y -W shared-memory=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Wasmtime fork:
`WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run -Ccache=n -W threads=y -W shared-memory=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y --invoke 'thread-worker-channel-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `168`

### T49: Vibe Public Thread Contract Docs

Status: done

Goal: keep Vibe's public builtin contract documentation aligned with the new
`ThreadChannel[T]` checker surface so follow-up work does not accidentally
target the old bare-`Int` channel API.

Implementation:

- updated Vibe's builtin contract table generator so `Threads::channel_new`,
  `Threads::send`, `Threads::recv`, and `Threads::spawn` document
  `ThreadChannel[T]`
- regenerated `docs/builtin_contract_table.generated.md`
- updated `docs/vibe.md` to describe the current typed channel surface, the
  remaining `Int`/`String` payload-lowering limit, and the fact that the
  runtime ABI still passes the handle as the existing tagged `Int` value
- clarified that `component-unsafe-os-threads` uses the local fork's
  `canon thread.spawn-indirect` path and that its returned task id is a
  Vibe-owned slot pointer, not a canonical guest join/result handle

Tests:

- `node scripts/gen_builtin_contract_table.mjs`
- `rg -n 'Threads::(channel_new|send|recv|spawn)' docs/builtin_contract_table.generated.md docs/vibe.md`

### T50: Vibe Worker Source-Type Shape Guard

Status: done

Goal: make the generated worker-dispatch contract enforce the source-level
`ThreadChannel[Int] -> Int` shape instead of accepting any `i64 -> i64` worker,
because bare `Int` and `ThreadChannel[Int]` currently share the same tagged
integer wasm representation.

Implementation:

- added `fn_param_types` and `fn_return_types` metadata to Vibe's codegen
  context alongside the existing wasm `ValueKind` function metadata
- copied that metadata through top-level function registration, scoped function
  registration, and function aliases
- changed the `Threads::spawn("name", ch)` worker resolver to require a
  capture-free top-level worker whose first source parameter is
  `ThreadChannel[Int]` and whose explicit source return type, when present, is
  `Int`
- kept the wasm-level `i64 -> i64` check as the ABI representation check
- added a compile test proving a plain `(_ch: Int) -> Int` worker is rejected
  with the `ThreadChannel[Int] -> Int` contract message

Tests:

- Red before implementation:
  `moon test src/runtime_compile --target native --filter '*plain Int thread worker parameter*'`
- Green after implementation:
  `moon test src/runtime_compile --target native --filter '*plain Int thread worker parameter*'`
- Return-type regression:
  `moon test src/runtime_compile --target native --filter '*non Int thread worker return*'`
- Regression:
  `moon test src/tests --target native --filter '*worker reads channel*'`

### T51: Vibe Worker Name Literal Guard

Status: done

Goal: close the remaining worker-dispatch ambiguity where a dynamic
`Threads::spawn(target, ch)` name could compile as a normal no-worker task even
when the source intended a real worker dispatch.

Implementation:

- changed the component/shared thread worker resolver so the spawn target must
  be a string literal in the current backend
- kept the string-literal reserved task names `"noop"` and `"alloc-probe"`
  explicit
- kept known worker names routed through the existing
  `ThreadChannel[Int] -> Int` source-type guard
- rejected non-literal worker-name expressions with a deterministic compile
  error instead of silently selecting worker code `0`
- documented that dynamic worker names remain unsupported until Vibe defines
  function-value or closure worker semantics

Tests:

- Red before implementation:
  `moon test src/runtime_compile --target native --filter '*dynamic thread worker name*'`
- Green after implementation:
  `moon test src/runtime_compile --target native --filter '*dynamic thread worker name*'`
- Regressions:
  `moon test src/runtime_compile --target native --filter '*plain Int thread worker parameter*'`
  and `moon test src/tests --target native --filter '*worker reads channel*'`

### T52: Vibe Shared-Thread RC Guard

Status: done

Goal: avoid compiling Vibe's Perceus/RC heap path into the unsafe shared-thread
backend until the RC/free-list allocator protocol has its own shared-thread
contract.

Implementation:

- added a compile-time `emit_module_wasm_with_options` guard for
  `enable_rc=true` plus any thread backend that requires shared linear memory
- kept the existing `linear-local` and normal wasm `enable_rc` paths unchanged
- made the diagnostic error name the option, backend id, and shared-thread
  backend constraint so accidental use is visible during local experiments
- left the atomic bump allocator path as the only currently supported shared
  heap allocation path for `component-unsafe-os-threads`

Tests:

- Red before implementation:
  `moon test src/runtime_compile --target native --filter '*enable_rc with component unsafe threads*'`
- Green after implementation:
  `moon test src/runtime_compile --target native --filter '*enable_rc with component unsafe threads*'`
- RC regression:
  `moon test src/runtime_compile --target native --filter '*enable_rc produces valid WASM*'`
- Thread regressions:
  `moon test src/runtime_compile --target native --filter '*Threads*'`
  and `moon test src/runtime_compile --target native --filter '*thread worker*'`

### T53: Vibe Shared-Thread Builder Grow Guard

Status: done

Goal: keep builder grow/realloc paths from using a Store-local heap tip as an
ownership proof when the shared-thread backend uses an atomic shared heap
cursor.

Implementation:

- changed `emit_builder_grow_check` so the shared-thread backend always takes
  the full realloc path instead of the heap-tip in-place grow fast path
- applied the same rule to `emit_builder_bulk_grow_check`
- kept the normal non-shared backend heap-tip fast path unchanged
- relied on the existing shared `emit_heap_alloc_local` atomic path for the new
  builder buffer allocation

Tests:

- Red/Green builder grow opcode guard:
  `moon test src/codegen --target native --filter '*shared thread builder grow avoids heap tip*'`
- Bulk grow regression:
  `moon test src/codegen --target native --filter '*shared thread builder*heap tip*'`

### T54: Vibe Shared-Thread cabi_realloc Alignment

Status: done

Goal: remove the align=4-only assumption from the shared-thread
`cabi_realloc` path so canonical ABI callers can pass arbitrary power-of-two
alignments without racing on `__heap_ptr`.

Implementation:

- added an `i32.atomic.rmw.cmpxchg` emitter
- changed the shared-thread `compile_cabi_realloc_function` path from
  `i32.atomic.rmw.add` to a CAS loop
- the CAS loop atomically loads the shared cursor, aligns the candidate pointer
  with the canonical ABI `align` parameter, computes the aligned end pointer,
  and commits that new cursor with compare-exchange
- kept the existing post-reservation memory growth check
- left the non-shared `cabi_realloc` path unchanged

Tests:

- Red/Green opcode guard:
  `moon test src/codegen --target native --filter '*shared thread cabi_realloc uses cmpxchg*'`
- Shared codegen regressions:
  `moon test src/codegen --target native --filter '*shared thread*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation and compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Unsafe run smoke:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-worker-channel-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `168`

### T55: Vibe Host Runner Shared Preview2 Allocation Guard

Status: done

Goal: prevent Vibe's JS Preview2 host runner from treating exported
`__heap_ptr` as an allocator authority when the guest memory is shared.

Implementation:

- made `scripts/wasm_vibe_host_runner.js` importable by tests without running
  the CLI entrypoint
- added a small shared-memory detector for exported WebAssembly memories
- kept the non-shared `__heap_ptr` Preview2 allocation fallback unchanged
- kept `cabi_realloc` as the preferred allocation path even when memory is
  shared
- rejected shared-memory Preview2 host allocation when `cabi_realloc` is not
  exported, instead of falling back to mutating `__heap_ptr`

Tests:

- Red/Green Node unit test:
  `node --test scripts/wasm_vibe_host_runner.test.mjs`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation and compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Unsafe run smoke:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-worker-channel-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `168`

Known test note:

- fixed in the same follow-up: `bash scripts/test_wasm_vibe_host_runner.sh`
  now decodes normal tagged `Int` results, treats `_start`'s untagged result
  separately, and the runner inserts a newline before its result line when
  guest stdout wrote data without a trailing newline.

### T56: Vibe String Channel Worker Probe

Status: done

Goal: broaden the generated worker-dispatch probe from `ThreadChannel[Int] ->
Int` to the next payload shape that still uses the current tagged handle/cell
ABI: `ThreadChannel[String] -> Int`.

Implementation:

- kept the worker return type fixed to `Int`
- allowed capture-free top-level worker functions whose sole parameter is
  `ThreadChannel[Int]` or `ThreadChannel[String]`
- kept dynamic worker names rejected and reserved diagnostic names unchanged
- at this point the channel handle and channel cell payload still used the
  existing tagged integer representation; T57 later widens the shared channel
  cell itself to preserve the full 64-bit tagged value
- added a generated component export
  `thread-worker-string-channel-probe` whose parent sends `"hello"` over a
  channel, the OS-owned child reads it with `Threads::recv(ch)`, and the worker
  returns `String::length(...)`

Tests:

- Red/Green component compile probe:
  `moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Worker rejection regressions:
  `moon test src/runtime_compile --target native --filter '*thread worker*'`
- Codegen call-split regressions:
  `moon test src/codegen --target native --filter '*Threads*'`
- Component validation and compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Unsafe run smoke:
  `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/release/wasmtime run ... --invoke 'thread-worker-string-channel-probe(0)' /tmp/vibe-thread-probe/thread.component.wasm`
  returned `20`, the current tagged Vibe `Int` representation of `5`

### T57: Vibe Shared Channel i64 Payload Cells

Status: done

Goal: remove the accidental 32-bit truncation from shared-thread channel
payload cells. The channel should preserve the full Vibe tagged value even
though the current probes only exercise `Int` and heap-pointer `String` values.

Implementation:

- changed `SharedThreadChannelAbi.cell_size` from `4` to `8`
- kept the channel header layout unchanged, so the first cell still starts at
  `channel_base + 40`
- added `i64.atomic.load` and `i64.atomic.store` emitters
- changed shared `Threads::send` to store the payload as an `i64` with
  `i64.atomic.store`
- changed shared `Threads::recv` to load the payload with `i64.atomic.load` and
  return that tagged value directly
- left the slot ABI unchanged in T57; worker results were still current `Int`
  payloads stored in the slot payload field until T58 widens that field

Tests:

- Red/Green ABI guard:
  `moon test src/x/threads --target native --filter '*shared atomic channel abi*'`
- Shared channel opcode guard:
  `moon test src/runtime_compile --target native --filter '*shared Threads channel builtins*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Codegen regressions:
  `moon test src/codegen --target native --filter '*Threads*'`
- Component validation and compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Unsafe run smoke:
  `thread-worker-string-channel-probe(0)` returned `20`, and
  `thread-worker-channel-probe(0)` still returned `168`

### T58: Vibe Slot i64 Payload Value

Status: done

Goal: remove the remaining 32-bit result-payload assumption from the generated
Vibe component-thread slot ABI. The slot payload should be an 8-byte tagged
Vibe value, matching the shared channel cell representation from T57.

Implementation:

- changed `ComponentThreadSlotAbi` so `payload` occupies bytes `8..16`
- shifted `input`, `cancel`, `mode`, and `worker_func` to offsets `16`, `20`,
  `24`, and `28`
- changed generated Vibe worker completion to publish the returned tagged value
  with `i64.atomic.store`
- changed generated `Threads::wait` to read the slot payload with
  `i64.atomic.load` and return the tagged value directly
- updated fork-local WAST fixtures and ABI drift guards to treat slot payloads
  as 8-byte cells while preserving the same diagnostic result values

Tests:

- Red/Green ABI guard:
  `moon test src/x/threads --target native --filter '*trampoline slot abi*'`
- Shared spawn/wait opcode guard:
  `moon test src/runtime_compile --target native --filter '*spawn wait with wait notify*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- WAST ABI fixtures:
  `target/release/wasmtime wast ... thread-spawn-indirect-os-trampoline-vibe-abi*.wast`
- Fork CLI unsafe smoke:
  `thread-worker-channel-probe(0)` returned `168`,
  `thread-worker-string-channel-probe(0)` returned `20`, and
  `thread-alloc-probe(0)` returned a positive tagged checksum, `8814592`
- Rust drift guard:
  `cargo +1.93.0 test --test all component_thread_abi`

### T59: Vibe Typed Task Handle Surface

Status: done

Goal: stop exposing Vibe task handles as arbitrary `Int` values at the source
type layer. The runtime representation can remain the existing tagged `Int`
slot pointer, but `Threads::wait` should consume a typed task handle and return
the task's result type.

Implementation:

- added the checker-level `ThreadTask[T]` named type
- changed `Threads::spawn("name", ch)` to return `ThreadTask[Int]` for the
  current worker/diagnostic result shape
- changed `Threads::wait(task)` to require `ThreadTask[T]` and return `T`
- kept the compiled ABI unchanged; generated code still stores and passes the
  task handle as the tagged slot pointer
- updated Vibe docs and generated builtin-contract docs to distinguish the
  source type surface from the runtime handle representation

Tests:

- Red/Green checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Source-level raw handle rejection:
  `moon test src/runtime_compile --target native --filter '*raw Int Threads wait handle*'`
- Thread regression suites:
  `moon test src/checker --target native --filter '*Threads*'`,
  `moon test src/runtime_compile --target native --filter '*Threads*'`, and
  `moon test src/codegen --target native --filter '*Threads*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile ... /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-channel-probe(0)` returned `168`, and
  `thread-worker-string-channel-probe(0)` returned `20`

### T60: Vibe Worker Result-Typed Tasks

Status: done

Goal: stop treating every real Vibe worker as an `Int` result once the slot
payload and shared channel cells both carry full 64-bit tagged Vibe values.
`Threads::spawn("name", ch)` should produce `ThreadTask[R]` when the named
worker has result type `R`, while reserved diagnostic names can keep returning
`ThreadTask[Int]`.

Implementation:

- changed the Vibe checker spawn contract to read the string-literal worker
  function's return type and construct `ThreadTask[R]`
- kept reserved diagnostic names such as `"noop"` and `"alloc-probe"` as
  `ThreadTask[Int]`
- allowed generated shared-thread workers to return either `Int` or `String`
  in the temporary `ThreadChannel[Int|String] -> Int|String` shape at this
  stage
- added a generated component probe where the OS-owned child reads a
  `ThreadChannel[String]` payload and returns that `String` through the slot;
  the parent waits on `ThreadTask[String]` and computes its length
- fixed the shared atomic allocator contract exposed by that probe: shared
  channel allocation and component task-slot allocation now use 8-byte aligned
  atomic bump reservations, so `i64.atomic.load/store` on channel cells and
  slot payloads cannot trap on accidental 4-byte alignment
- kept the runtime task handle unchanged as the tagged slot pointer

Tests:

- Red/Green checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Red/Green worker return support:
  `moon test src/runtime_compile --target native --filter '*String thread worker return*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-string-result-probe(0)` returned `20`,
  `thread-worker-string-channel-probe(0)` returned `20`, and
  `thread-worker-channel-probe(0)` returned `168`

### T61: Vibe Child-Allocated String Result Probe

Status: done

Goal: prove that a Vibe worker can allocate a heap object in the OS-owned child
and return it through `ThreadTask[String]`, not just echo a parent-allocated
string pointer through the slot payload.

Implementation:

- added a generated component probe where `worker_string_append` reads a
  `ThreadChannel[String]` payload, computes `String::concat(value, "!")`, and
  returns the child-allocated `String`
- fixed the shared atomic heap allocator so ordinary Vibe heap object
  allocations return at least 4-byte aligned pointers; this is required because
  the current tagged pointer contract stores object values as `ptr | 1` and
  untagging masks the low two bits
- kept the 8-byte aligned allocation path for shared channel cells and
  component task slots that contain `i64.atomic.load/store` payload fields
- documented the distinction between ordinary 4-byte object alignment and
  8-byte atomic-field alignment

Tests:

- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-string-alloc-result-probe(0)` returned `24`, proving the
  parent observed the child-allocated `"hello!"` string length after `wait`
- Regression smoke:
  `thread-worker-string-result-probe(0)` still returned `20`

### T62: Vibe Child-Allocated Array Result Probe

Status: done

Goal: extend the child-allocated result probe from a flat string object to a
small structured heap object. This checks that an OS-owned child can allocate
an `Array[Int]`, return the tagged pointer through the Vibe slot payload, and
let the parent read the array header after `Threads::wait`.

Implementation:

- broadened the temporary worker return guard from `Int|String` to
  `Int|String|Array[Int]`
- kept worker channel parameters limited to `ThreadChannel[Int]` or
  `ThreadChannel[String]` at this stage
- added a checker contract test for `Threads::spawn("worker_array", ch)` as
  `ThreadTask[Array[Int]]`
- added a compile test and generated component export
  `thread-worker-array-result-probe`
- updated Vibe's generated builtin contract docs and this fork's conformance
  docs to describe the narrow `Array[Int]` result support

Tests:

- Red/Green worker result support:
  `moon test src/runtime_compile --target native --filter '*Array*thread worker return*'`
- Checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-array-result-probe(0)` returned `12`, proving the parent
  observed the child-allocated `[1, 2, 3]` length after `wait`
- Regression smoke:
  `thread-worker-string-alloc-result-probe(0)` still returned `24`

### T63: Vibe Parent-Allocated Array Channel Payload Probe

Status: done

Goal: extend the structured heap-object probe from child-to-parent results to a
parent-to-child channel payload. This checks that a parent can allocate an
`Array[Int]`, send the full tagged pointer through the shared channel cell, and
let an OS-owned child read both the array header and element storage after
`Threads::recv`.

Implementation:

- broadened the temporary channel payload guard from `Int|String` to
  `Int|String|Array[Int]`
- broadened the worker parameter guard to accept
  `ThreadChannel[Array[Int]]`
- added a checker contract test for `Threads::send(ch_array, xs)` and
  `Threads::spawn("worker_array_payload", ch_array)`
- added a compile test and generated component export
  `thread-worker-array-channel-probe`
- updated Vibe's generated builtin contract docs and this fork's conformance
  docs to describe the narrow `Array[Int]` payload support

Tests:

- Red/Green channel payload support:
  `moon test src/runtime_compile --target native --filter '*Array*thread channel payload*'`
- Checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-array-channel-probe(0)` returned `1280`, proving the child
  observed the parent-allocated `[10, 20, 30]` length and element `1` after
  `recv`; decoded Vibe result is `320`
- Regression smoke:
  `thread-worker-array-result-probe(0)` still returned `12` and
  `thread-worker-string-alloc-result-probe(0)` still returned `24`

### T64: Vibe Nested String Array Channel Payload Probe

Status: done

Goal: extend the parent-to-child structured payload probe from a flat
`Array[Int]` to an `Array[String]` whose elements are nested heap-object
pointers. This checks that an OS-owned child can receive a parent-allocated
array, read its header, follow an element pointer to a string object, and return
a derived tagged `Int` result.

Implementation:

- broadened the temporary channel payload guard from `Int|String|Array[Int]` to
  `Int|String|Array[Int]|Array[String]`
- kept worker result support limited to `Int|String|Array[Int]` at this stage
- added checker contract coverage for `ThreadChannel[Array[String]]`,
  `Threads::send(ch_array_string, strings)`, and
  `Threads::spawn("worker_array_string_payload", ch_array_string)`
- added a compile test and generated component export
  `thread-worker-array-string-channel-probe`
- updated Vibe's generated builtin contract docs and this fork's conformance
  docs to describe the narrow `Array[String]` payload support

Tests:

- Red/Green channel payload support:
  `moon test src/runtime_compile --target native --filter '*Array*String*thread channel payload*'`
- Checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-array-string-channel-probe(0)` returned `820`, proving the
  child observed the parent-allocated `["red", "green"]` length and followed
  element `1` to compute `String::length("green")`; decoded Vibe result is
  `205`
- Regression smoke:
  `thread-worker-array-channel-probe(0)` still returned `1280`

### T65: Vibe Child-Allocated String Array Result Probe

Status: done

Goal: extend the child-to-parent structured result probe from a flat
`Array[Int]` to an `Array[String]` whose elements are nested heap-object
pointers. This checks that an OS-owned child can allocate an array, allocate or
embed string objects as elements, return the array pointer through
`ThreadTask[Array[String]]`, and let the parent follow the nested string pointer
after `Threads::wait`.

Implementation:

- broadened the temporary worker result guard from `Int|String|Array[Int]` to
  `Int|String|Array[Int]|Array[String]`
- kept worker channel payload support at
  `Int|String|Array[Int]|Array[String]`
- added checker contract coverage for
  `Threads::spawn("worker_array_string", ch_int)` returning
  `ThreadTask[Array[String]]`
- added a compile test and generated component export
  `thread-worker-array-string-result-probe`
- updated Vibe's generated builtin contract docs and this fork's conformance
  docs to describe the narrow `Array[String]` result support

Tests:

- Red/Green worker result support:
  `moon test src/runtime_compile --target native --filter '*Array*String*thread worker return*'`
- Checker contract:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-array-string-result-probe(0)` returned `820`, proving the
  parent observed the child-allocated `["red", "green"]` length and followed
  element `1` to compute `String::length("green")`; decoded Vibe result is
  `205`
- Regression smoke:
  `thread-worker-array-string-channel-probe(0)` still returned `820`

### T66: Vibe Shared Thread Value Guard Consolidation

Status: done

Goal: keep the temporary Vibe worker ABI subset explicit before broadening it
again. Channel payloads and worker results should use the same narrow
shared-value predicate, currently
`Int|String|Array[Int]|Array[String]`, so the next structured-value work does
not accidentally diverge between `ThreadChannel[T]` and `ThreadTask[R]`.

Implementation:

- renamed the Vibe checker payload guard to a shared-value guard while keeping
  the supported set unchanged
- collapsed Vibe codegen's separate channel-payload array and worker-result
  array predicates into one worker shared-value predicate
- added regression coverage that rejects nested array thread channel payloads
  and nested array worker results, preserving the current intentionally narrow
  subset

Tests:

- Checker contract and nested payload regression:
  `moon test src/checker --target native --filter '*Threads*'`
- Runtime compile nested payload/result regression:
  `moon test src/runtime_compile --target native --filter '*nested array thread*'`
- Existing codegen and runtime thread coverage:
  `moon test src/codegen --target native --filter '*Threads*'`
  and `moon test src/runtime_compile --target native --filter '*Threads*'`

### T67: Vibe Tuple Thread Payload and Result Probe

Status: done

Goal: broaden the temporary Vibe shared-value subset from arrays to one
structured heap object shape without changing the fork-local slot ABI. The
minimal supported tuple shape is `(Int, String)` / `Tuple[Int|String, ...]`:
tuple fields are still scalar-only, and nested arrays/tuples remain outside the
current guard.

Implementation:

- extended the Vibe checker/codegen shared-value guards to accept
  `Type::Tuple` when every tuple field is `Int` or `String`
- added compile coverage for tuple worker results and tuple channel payloads
- added generated component exports
  `thread-worker-tuple-result-probe` and
  `thread-worker-tuple-channel-probe`
- disabled Vibe's multi-value tuple-return optimization for registered thread
  workers, because the thread trampoline expects one `i64` tagged heap value
  and otherwise generated invalid wasm for `(result i64 i64)` worker functions
- updated Vibe's public builtin contract docs and this fork's conformance docs

Tests:

- Red/Green tuple support:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
  and `moon test src/runtime_compile --target native --filter '*tuple thread*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-tuple-result-probe(0)` returned `2820`, proving the parent
  decoded the child-allocated `(7, "green")` tuple as `705`
- Fork CLI unsafe smoke:
  `thread-worker-tuple-channel-probe(0)` returned `820`, proving the child
  decoded the parent-allocated `(2, "green")` tuple as `205`

### T68: Vibe Scalar-Field Record Thread Value Probe

Status: done

Goal: broaden the temporary Vibe shared-value subset from scalar arrays and
tuples to one named-field heap object shape while keeping the fork-local slot
ABI unchanged. The supported record shape is intentionally narrow: every record
field must be `Int` or `String`, and nested arrays/tuples/records remain
outside the current guard.

Implementation:

- extended the Vibe checker/codegen shared-value guards to accept
  `Type::Record` when every field type is `Int` or `String`
- passed checker-inferred top-level function return types into component
  codegen so unannotated worker returns are checked by the same guard as
  annotated worker returns
- added compile coverage for a record worker result and a record value sent
  through the shared channel cell
- added the generated component export
  `thread-worker-record-result-probe`
- documented that full child-side record channel receive coverage still needed
  a source-level record type annotation; T69 closes that follow-up
- updated Vibe's public builtin contract docs and this fork's conformance docs

Tests:

- Red/Green record support:
  `moon test src/checker --target native --filter '*Threads builtins match documented contract*'`
  and `moon test src/runtime_compile --target native --filter '*record thread*'`
  The runtime filter includes both scalar-field record accept cases and
  array-field record reject cases.
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-record-result-probe(0)` returned `2820`, proving the parent
  decoded the child-allocated `{ score: 7, word: "green" }` record as `705`

### T69: Vibe Record Type Annotation and Channel Receive Probe

Status: done

Goal: remove the last record-channel caveat by making Vibe source type
annotations able to express scalar-field record channel payloads directly. This
lets the child worker type its channel as
`ThreadChannel[{ score: Int, word: String }]` and exercise the same
parent-allocated record through `Threads::recv(ch)` that arrays and tuples
already covered.

Implementation:

- added Vibe parser support for source-level record type literals
  `{ field: Type, ... }`
- added parser coverage for `type Row = { score: Int, word: String }` and
  `ThreadChannel[{ score: Int, word: String }]`
- replaced the earlier record-channel noop send smoke with a real worker
  receive probe that reads both record fields in the OS-owned child
- added generated component export
  `thread-worker-record-channel-probe`
- updated Vibe syntax/thread docs and this fork's conformance docs

Tests:

- Parser Red/Green:
  `moon test src/parser --target native --filter '*record type literal*'`
- Runtime compile record receive regression:
  `moon test src/runtime_compile --target native --filter '*record thread*'`
- Generated component smoke:
  `env VIBE_THREAD_COMPONENT_PROBE_OUT=/tmp/vibe-thread-probe/thread.component.wasm moon test src/runtime_compile --target native --filter '*component unsafe Threads spawn runtime imports*'`
- Component validation/compile:
  `wasm-tools validate --features all /tmp/vibe-thread-probe/thread.component.wasm`
  and `target/release/wasmtime compile -W shared-everything-threads=y /tmp/vibe-thread-probe/thread.component.wasm`
- Fork CLI unsafe smoke:
  `thread-worker-record-channel-probe(0)` returned `820`, proving the child
  decoded the parent-allocated `{ score: 2, word: "green" }` record as `205`

### T70: Canonical `thread.spawn-ref` Shared Probe

Status: done

Goal: close the `canon thread.spawn-ref shared?` gap enough for local
shared-everything experiments without changing the Vibe worker ABI away from
the existing trampoline/table dispatch path.

Implementation:

- added `ThreadSpawnRef` through component translation, DFG/info trampolines,
  Cranelift libcall generation, the component builtin list, and the runtime
  component async store trait
- reused `VMFuncRef` to call a direct start function and share the existing
  cooperative `thread.new` plus immediate resume behavior
- allowed concrete shared function references in the core type converter while
  keeping shared arrays/structs/exceptions unsupported
- added a `spawn-ref` unsafe OS-thread validation path that does not require a
  start table and reuses the sibling-Store OS-thread backend
- added cooperative and unsafe preemptive WAST probes for `thread.spawn-ref`

Tests:

- `cargo +1.93.0 test --test wast -- thread-spawn-ref`
- `env WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/debug/wasmtime wast -Ccache=n -W threads=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y tests/misc_testsuite/component-model-threading/thread-spawn-ref-preemptive-smoke.wast`
- `cargo +1.93.0 test -p wasmtime runtime::component::threading::tests::`

### T71: Internal Canonical `shared?` Flag Plumbing

Status: done

Goal: stop hard-wiring the unsafe OS-thread decision only at the runtime
environment-variable boundary. The runtime should carry a canonical
shared/preemptive bit through the component trampoline path so future parser
support for the Component Model `shared?` immediate can feed the same backend
without another ABI rewrite.

Implementation:

- kept the public `wasmparser::CanonicalFunction` shape unchanged, because
  external workspace crates such as `wasmprinter` and `wasm-encoder` still
  compile against that API and the current `wast` text parser does not expose
  `(canon thread.spawn-* shared ...)`
- treats the current legacy local parser output as `shared: true` when building
  Wasmtime's `LocalInitializer`
- added `shared: bool` to the Wasmtime component initializer, DFG trampoline,
  serialized component info trampoline, Cranelift libcall lowering, VM libcall,
  and runtime component async-store trait
- gates the unsafe sibling-Store OS-thread backend on both `shared == true`
  and `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`
- gates `thread.available-parallelism` host parallelism reporting on
  `shared == true`; non-shared calls stay cooperative and report `1`
- refactored the fork-local `wasmparser` validator helpers so spawn type/table
  validation accepts a sharedness parameter internally, even though the current
  reader still passes the legacy shared value

Remaining gap:

- the actual Component Model binary/text `shared?` immediate is still not read
  through this workspace's `wasmparser`/`wast` surface. Adding that requires a
  coordinated wasm-tools API update or local vendoring of the affected crates,
  because changing `wasmparser::CanonicalFunction` directly breaks downstream
  workspace crates.

Tests:

- `cargo +1.93.0 check -p wasmtime --lib`
- `cargo +1.93.0 test --test wast -- thread-spawn-ref`
- `cargo +1.93.0 test --test wast -- thread-available-parallelism`
- `cargo +1.93.0 test --test wast -- thread-spawn-indirect`
- `env WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/debug/wasmtime wast -Ccache=n -W threads=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y tests/misc_testsuite/component-model-threading/thread-spawn-ref-preemptive-smoke.wast`
- `cargo +1.93.0 test -p wasmtime runtime::component::threading::tests::`

### T72: Internal Non-Shared Thread Flag Regression

Status: done

Goal: prove the new internal canonical shared/preemptive flag is not just
metadata. Even if the unsafe OS-thread environment opt-in is enabled,
non-shared canonical thread builtins must stay on the cooperative path and must
not report host parallelism.

Implementation:

- factored the unsafe OS-thread gating into a pure helper:
  `component_thread_unsafe_os_spawn_allowed_for(shared, unsafe_enabled)`
- factored `thread.available-parallelism` sizing into a pure helper:
  `component_thread_available_parallelism_for(shared, unsafe_enabled)`
- kept runtime dispatch using the real environment-variable reader, but made
  tests call the pure helpers directly so the test suite does not mutate global
  process environment

Tests:

- `cargo +1.93.0 test -p wasmtime runtime::component::concurrent::tests::component_thread_shared_flag_gates_unsafe_os_spawn`
- `cargo +1.93.0 test -p wasmtime runtime::component::concurrent::tests::component_thread_available_parallelism_requires_shared`
- `cargo +1.93.0 test -p wasmtime runtime::component::threading::tests::`

### T73: Parser Surface Strategy For `shared?`

Status: done

Goal: avoid another false start on the Component Model `shared?` immediate.
The fork needs to know whether it can safely add binary/text parser support in
only the local `wasmparser` fork, or whether it must patch the whole wasm-tools
surface used by this workspace.

Findings:

- the current workspace only forks `wasmparser`; `wast`, `wasm-encoder`, and
  `wasmprinter` are still external workspace dependencies
- adding `shared` fields directly to `wasmparser::CanonicalFunction` breaks
  downstream crates that exhaustively match the current public enum
- `wast` parses `(canon thread.spawn-ref $ft)`,
  `(canon thread.spawn-indirect $ft (table $tbl))`, and
  `(canon thread.available_parallelism)` with no `shared` token
- `wasm-encoder::CanonicalFunctionSection` emits the same legacy local binary
  shape, with no `shared` argument
- `wasmprinter` prints the same legacy text shape, with no `shared` token
- a reader-only compatibility shim is not sound enough to rely on: the legacy
  local encodings for `thread.spawn-ref` and `thread.spawn-indirect` begin with
  a type index, while the proposal encoding begins with `shared?`; common
  legacy type indices `0` and `1` are byte-identical to valid `shared?`
  immediates, and canonical function entries are not independently
  length-delimited

Decision:

- do not add a heuristic parser that tries to guess legacy-vs-proposal
  encoding from raw bytes
- keep the current Wasmtime-internal `shared` flag plumbing and mark legacy
  local parser output as `shared: true`
- when implementing the real parser surface, patch or vendor the affected
  wasm-tools crates together: `wasmparser`, `wast`, `wasm-encoder`, and
  `wasmprinter`
- only after that, update WAST fixtures from the legacy local text shape to
  explicit proposal text forms such as `(canon thread.spawn-ref shared $ft)`

### T74: Proposal-Aligned Vibe Thread Implementation Checklist

Status: in progress

Goal: keep the fork-local Vibe thread backend aligned with the current
shared-everything threads / Component Model thread proposal shape. Vibe should
build language-level task, channel, completion, and cancellation semantics on
top of proposal primitives rather than treating fork-local diagnostics as new
canonical builtins.

Checklist:

- [x] Treat canonical `thread.spawn-*` return values as component thread-table
  indices, not Vibe join/result handles.
- [x] Keep Vibe task completion, terminal status, and result payloads in
  generated shared-memory slots synchronized with linear-memory wait/notify.
- [x] Use the canonical start parameter as a single context pointer into the
  generated Vibe slot ABI.
- [x] Keep `thread.available-parallelism` as a sizing/reporting hint; only
  report host parallelism when the internal canonical shared/preemptive flag and
  unsafe fork opt-in are both set.
- [x] Keep the fork-local shared `thread.index` compatibility path diagnostic;
  do not make Vibe semantics depend on it as a durable thread id.
- [x] Keep `ComponentModelUnsafeOsThreads` distinct from any future
  proposal-complete `ComponentModelShared` backend.
- [x] Add a focused WAST probe where the spawned thread writes completion
  through a context-pointer slot that is deliberately disjoint from the returned
  canonical thread-table index.
- [x] Keep broadening Vibe channel/task payload support through the same
  shared-value ABI rather than by extending canonical `thread.spawn-*`.
- [x] Add a guard documenting why this fork must not implement a reader-only
  `shared?` parser shim.
- [ ] Revisit real parser/text/binary support for the `shared?` immediate only
  as a coordinated wasm-tools surface update (`wasmparser`, `wast`,
  `wasm-encoder`, and `wasmprinter`).

Implementation notes:

- `ThreadTask[T]` remains a Vibe type-layer/runtime ABI wrapper over a
  generated task slot. It must not be collapsed into the canonical
  thread-table index.
- `ThreadChannel[T]` remains a Vibe runtime queue/cell abstraction backed by
  shared memory and typed by the Vibe checker. It is not a Component Model
  `future`/`stream` builtin.
- The context-pointer trampoline is the migration point if the proposal later
  gains arbitrary thread-entry parameters.

### T75: Context Pointer Is Not The Spawn Handle

Status: done

Goal: prove that the unsafe fork path can run a proposal-shaped
`thread.spawn-indirect` while Vibe-level completion is addressed by the
single `i32` context pointer, not by the returned canonical thread-table index.

Implementation:

- added
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-context-pointer-not-handle.wast`
- the child start function receives an `i32` context pointer, writes completion
  to that shared-memory slot, and notifies the parent
- the parent stores the canonical spawn return value separately and asserts it
  is not the Vibe slot pointer or result payload
- registered the fixture as an unsafe-only expected-fail in the normal WAST
  runner, matching the existing OS-thread probes
- added an ignored CLI smoke test,
  `cli_tests::run_component_thread_context_pointer_probe`, that executes the
  WAST with `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`

Tests:

- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/debug/wasmtime wast -Ccache=n -W threads=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-context-pointer-not-handle.wast`
- `cargo +1.93.0 test --test wast -- thread-spawn-indirect-os-context-pointer-not-handle`
- `cargo +1.93.0 test --test all run_component_thread_context_pointer_probe -- --ignored`

### T76: Full 64-bit Shared-Value Payload Path

Status: done

Goal: keep broadening Vibe channel/task payload support through the generated
shared-value ABI instead of treating canonical `thread.spawn-*` as a Vibe
payload/result transport.

Implementation:

- added
  `tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-value-payload.wast`
- the parent writes a high-bit `i64` tagged value into an aligned
  channel-like shared cell
- the child receives only the `i32` context pointer, loads the `i64` cell, and
  publishes a derived high-bit `i64` through the task slot payload
- the parent asserts that the high 32 bits survive both the channel-like cell
  and the task slot payload path, while the canonical spawn return remains a
  separate thread-table index

Tests:

- `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1 target/debug/wasmtime wast -Ccache=n -W threads=y -W component-model=y -W component-model-async=y -W component-model-threading=y -W gc=y -W function-references=y -W shared-everything-threads=y tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-shared-value-payload.wast`
- `cargo +1.93.0 test --test wast -- thread-spawn-indirect-os-shared-value-payload`
- `cargo +1.93.0 test --test all run_component_thread_shared_value_payload_probe -- --ignored`

### T77: `shared?` Parser Surface Boundary Guard

Status: done

Goal: keep the remaining `shared?` immediate work honest. The current proposal
text defines `canon thread.spawn-ref shared?`, `canon thread.spawn-indirect
shared?`, and `canon thread.available-parallelism shared?`, but this workspace
currently patches only `wasmparser`. `wast`, `wasm-encoder`, and
`wasmprinter` still come from the external wasm-tools release. Changing only
`wasmparser::CanonicalFunction` to carry `shared` would break these downstream
crates because they match the current public enum shape.

Findings:

- `Cargo.toml` patches only `wasmparser` to `crates/forks/wasmparser`
- external `wast` parses only the legacy local text forms:
  `(canon thread.spawn-ref $ft)`,
  `(canon thread.spawn-indirect $ft (table $tbl))`, and
  `(canon thread.available_parallelism)`
- external `wasm-encoder` re-encodes `wasmparser::CanonicalFunction` using the
  same legacy local binary shape
- external `wasmprinter` prints the same legacy local text shape
- the proposal binary shape adds a `shared?` immediate before the function type
  index, but legacy local encodings whose type index is `0` or `1` are
  byte-ambiguous with that immediate

Decision:

- do not add a reader-only or heuristic `shared?` parser shim
- keep the Wasmtime-internal `shared` flag plumbing from T71
- keep current local legacy thread builtins marked as `shared: true` for the
  unsafe fork probes
- implement real `shared?` support only by updating or forking the coordinated
  wasm-tools surface: `wasmparser`, `wast`, `wasm-encoder`, and
  `wasmprinter`

Guard:

- `cargo +1.93.0 test --test all component_thread_shared_immediate_tooling_scope_stays_documented`

## Current Next Step

Continue after T40. The parent now has a real thread-table placeholder, an
internal completion record, distinct trap behavior for cooperative resume
builtins, parent-driven cleanup and join for completed OS-owned placeholders, a
typed child-store data factory, shared table/global rebinding for table-based
start dispatch, lifecycle accounting for outstanding OS-owned children, a
fork-local diagnostic contract for the unsafe backend, and a fork-local
embedder cancel/status/try-join/blocking-join cleanup contract. Cancel can now interrupt
already-running child Wasm when epoch interruption is enabled, and it can wake
child Wasm blocked in `memory.atomic.wait32/64` on rebound shared memory.
Rebound shared memory now has an explicit bidirectional wait/notify
synchronization probe. Imported runtime start-table growth is now allowed only
for the table-only owner shape; direct defined growable shared-table starts and
growable shared table owners that define functions are rejected by both unit and
runtime WAST probes. Component Model resources and GC canonical options are now
explicitly rejected by the unsafe preemptive path. Direct immutable defined
shared-global starts are allowed, and direct mutable defined shared-global
starts now flush the child owner value back to the parent definition after the
start function returns. OS-owned child Stores now install a Store-local current
guest thread whose `instance_rep` mirrors the parent-visible returned
thread-table index. Fork-local public diagnostic cleanup APIs now prove consumed
OS-owned indices are immediately removed from lookup. Public unsafe index lookup
now ignores non-OS cooperative index collisions, rejects OS-owned collisions as
ambiguous, and proves consumed numeric indices can be reused for later OS-owned
threads. Fork-local
completion-report diagnostics now preserve child setup/start/panic failures as
`Failed` values for host diagnostics. Shared start functions can now import the
fork-local shared `canon thread.index` compatibility path and observe the
parent-visible transient thread-table index. The canonical spawn return is not
a stable guest join handle. Vibe-level normal completion now has a
trampoline-managed shared-state wait/notify probe, and producer-level
completed/cancelled/failed-as-value terminal codes now have a matching
trampoline status probe. Real Wasm traps in OS-owned child code are now pinned
as host diagnostic failures rather than Vibe terminal status values. In-flight
Vibe-level cancellation now has a producer-owned shared cancel flag plus
wait/notify probe, so host-driven cancellation remains a diagnostic boundary
rather than part of the generated guest ABI. The unsafe preemptive validator now
has an explicit positive Vibe shared ownership subset and rejects unrelated
growable shared tables instead of allowing them just because the owner has no
functions. The separate completion/status/cancel trampoline probes now have a
single consolidated Vibe runtime ABI probe that joins, observes terminal status,
handles failure-as-value, cancels in-flight work, and aggregates completed
payloads without using the canonical spawn return value. A multi-worker queue
probe now validates four OS-owned children sharing a start gate, contending on
an atomic job counter, and aggregating per-worker counts/checksums. A phased
barrier/reduction probe now validates child-to-child rendezvous before a second
phase consumes the complete first-phase reduction. ABI-shaped serial and
parallel speedup probes now validate the same slot layout with a CPU workload
and direct local timing still shows about `2.75x` wall-clock speedup. The
proposal conformance map now separates proposal-defined pieces from
proposal-aligned subsets, fork-local diagnostics, and gaps. The Vibe slot ABI
now has a drift guard against the Wasmtime WAST fixtures/docs plus a manual
unsafe CLI smoke test for the consolidated ABI probe. The Vibe-generated
component now reaches the fork-local OS-thread path through
`ComponentModelUnsafeOsThreads`, with `__heap_ptr` emitted as a shared mutable
global and the CLI store-data factory installed for `Store<Host>`. Vibe's
standard shared-backend allocation helpers now reserve heap space through a
shared-memory atomic cursor. A diagnostic Vibe-generated allocation workload
now spawns one or four OS-owned `alloc-probe` children that contend on that
cursor and report positive payload checksums through the Vibe slot ABI. The
generated worker-dispatch probe now keeps non-exported capture-free top-level
`ThreadChannel[Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]] -> Int|String|Array[Int]|Array[String]|Tuple[Int|String, ...]|Record[Int|String fields]`
Vibe workers alive through
`Threads::spawn("worker", ch)`, compiles them as shared core functions,
dispatches them from the OS-owned child trampoline, and publishes their tagged
results through the slot payload. Worker name
resolution now has an explicit reserved `"noop"` smoke path and rejects unknown
string-literal worker names instead of silently compiling a no-op task. The
generated worker path now also proves that the slot input channel handle is
usable in the OS-owned child by reading `Int` and `String` payloads with
`Threads::recv(ch)`, deriving `42` and `5`, and returning either `Int` or
`String` results through the slot payload; it now also covers a child-allocated
`Array[Int]` or `Array[String]` result plus parent-allocated `Array[Int]` and
`Array[String]` channel payloads.
The Vibe checker now links that channel handle through `ThreadChannel[T]`, so
`Threads::send` and `Threads::recv` share one payload type instead of using an
unconstrained `recv` result, and Vibe's generated/public builtin contract docs
now describe that typed channel surface rather than the old bare-`Int` channel
API. The Vibe checker also links task handles through `ThreadTask[T]`, so
`Threads::spawn("name", ch)` returns a worker-result-shaped `ThreadTask[R]`,
and `Threads::wait` no longer accepts arbitrary `Int` values even though the
runtime handle remains the tagged slot pointer. Vibe codegen also now rejects plain
`(Int) -> Int` workers for thread spawn, even though that shape has the same
current wasm representation as a channel handle, and it rejects dynamic
worker-name expressions rather than silently compiling a no-worker task. Vibe
now also rejects `enable_rc=true` with the shared-thread backend instead of
compiling the current RC/free-list heap path into OS-owned children. Builder
grow and bulk grow now also avoid the heap-tip in-place fast path under the
shared-thread backend and route growth through full realloc plus the atomic
shared allocator. Vibe's JS Preview2 host runner now refuses the shared-memory
`__heap_ptr` allocation fallback and requires exported `cabi_realloc` for that
path. Shared channel cells and the Vibe component-thread slot payload now use
`i64.atomic.load/store`, preserving the full Vibe tagged value instead of
truncating through an i32 cell, and the allocations that contain those i64
atomic fields are now 8-byte aligned under the shared atomic allocator.
Ordinary shared atomic heap allocations now also return 4-byte aligned Vibe
object pointers, and a Vibe worker can allocate a new `String`, `Array[Int]`,
`Array[String]`, scalar-field tuple, or scalar-field record in the OS-owned
child, return it through `ThreadTask[String]`, `ThreadTask[Array[Int]]`,
`ThreadTask[Array[String]]`, `ThreadTask[(Int, String)]`, or
`ThreadTask[{...}]`, and have the parent read it after `wait`. The parent can
also send a narrow `Array[Int]`, `Array[String]`, or `(Int, String)` payload to
an OS-owned child through `ThreadChannel[Array[Int]]`,
`ThreadChannel[Array[String]]`, `ThreadChannel[(Int, String)]`, or
`ThreadChannel[{ score: Int, word: String }]`. The Vibe
checker/codegen guards for that temporary subset are now consolidated around
one shared-value concept, nested arrays remain explicitly rejected, and
tuple-returning thread workers bypass Vibe's multi-value tuple-return
optimization so the trampoline still receives one tagged `i64` payload. The
fork now also translates and runs direct `canon thread.spawn-ref` start
functions in cooperative mode and through the unsafe sibling-Store OS-thread
path for the constrained shared subset, but Vibe still uses the
spawn-indirect/trampoline ABI for worker dispatch and result slots. The
component trampoline path now carries an internal canonical shared/preemptive
flag from initializer metadata through Cranelift and the runtime libcall, and
the unsafe OS-thread backend plus `thread.available-parallelism` host
parallelism reporting are gated by that flag. Non-shared internal calls now
have pure regression coverage proving they ignore the unsafe OS-thread opt-in
and report cooperative parallelism as `1`. The current parser still cannot
surface the actual Component Model `shared?` immediate, so legacy fork-local
thread spawn builtins are marked as shared internally until the wasm-tools
surface catches up. A reader-only parser shim is intentionally rejected because
legacy type indices `0` and `1` are ambiguous with the proposal's `shared?`
byte; real support needs coordinated wasm-tools surface changes. The next gap
is broadening that worker path into real Vibe thread semantics:
function values or closures if desired, structured heap-object ownership beyond
the current narrow array/tuple/record payload/result probes, plus a future
thread-safe RC/free-list protocol if Vibe wants RC in shared-thread components.
Any additional embedder-specific host allocation helpers also need to route
through `cabi_realloc` or the atomic shared cursor rather than treating
`__heap_ptr` as the live allocator.
