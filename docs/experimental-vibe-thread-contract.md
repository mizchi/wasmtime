# Experimental Vibe backend boundary

Status: fork-local integration boundary
Date: 2026-08-22

This document records the current boundary between this Wasmtime fork and
`mizchi/vibe-lang`. It follows the active Vibe design in
`docs/concurrency.md`, `docs/compiler-parallelism.md`, and
`docs/wasm_threads_requirements.md` as inspected at `038f07e4c`.

This is not an upstream Wasmtime contract. It is also not a guest ABI for Vibe.
The older Vibe prototype based on `Threads::*`, `ThreadTask[T]`,
`ThreadChannel[T]`, raw integer handles, string-selected workers, and a fixed
32-byte shared slot has been retired. In particular, `Threads::*` is not a Vibe API.

## Vibe semantics own the contract

Vibe exposes shared-nothing structured concurrency. Its contract is expressed
in terms of `TaskGroup`, region-bound tasks and channels, structural `Send`,
task-local handler evidence, cancellation, and deterministic convergence of a
task group. Those semantics must be identical across cooperative, host-worker,
WASI Component Model, and any future shared-everything lowering.

Wasmtime intrinsics and host diagnostics must not encode Vibe task, channel, join, or cancellation semantics. Component thread-table indices are transient
engine values, not Vibe task identities. Likewise,
`thread.available-parallelism` is a sizing hint rather than a semantic promise.

## Current production-shaped boundary

The production-shaped native/WASI backend is host-owned parallelism:

- the embedder owns a bounded worker pool;
- each worker has an independent `Store` and `Instance` and an independent
  linear or GC heap;
- jobs and outcomes cross the host boundary as copied, typed values;
- the coordinator owns diagnostics, cancellation policy, publication, and
  deterministic commit order;
- Vibe's cooperative scheduler remains the semantic oracle for differential
  backend tests.

This design needs ordinary stable Wasmtime embedding APIs. It does not depend
on this fork's unsafe Component Model OS-thread implementation.

## Role of this fork

The shared-everything work remains a feature-detected, opt-in experiment. It
answers engine and proposal questions:

- whether shared component start functions validate and execute;
- whether shared memory, tables, globals, wait/notify, and function references
  retain their required identity across execution contexts;
- whether execution is genuinely parallel;
- whether unsupported resources, GC roots, and ownership shapes are rejected;
- whether host diagnostic cancellation and completion remain internally
  consistent.

The unsafe runtime implementation and its embedder APIs are compiled only with
the off-by-default Cargo feature `experimental-component-threads`. Enabling
Component Model async support alone does not compile the sibling-Store
rebinding module or expose its diagnostic APIs. Runtime execution additionally
requires `WASMTIME_UNSAFE_COMPONENT_THREAD_OS_SPAWN=1`.

The fixed diagnostic slots in the WAST probes are test scaffolding for those
questions. Despite legacy filenames containing `vibe-abi`, they are not a
current Vibe ABI and must not be consumed by Vibe code generation.

## Promotion requirements

A Vibe shared-everything lowering may be reconsidered only when all of these
hold:

1. the proposal grammar and intrinsic names match an upstream, versioned
   Wasmtime release;
2. required shared heap types, functions, tables, composites, and Component
   Model intrinsics are implemented without this unsafe environment variable;
3. Vibe has a sound concurrency rule beyond `Send` for values simultaneously
   reachable by multiple workers, or restricts sharing to immutable
   publish-once regions;
4. allocation, collection, finalization, cancellation, and task-local evidence
   preserve the Vibe structured-concurrency contract;
5. the backend passes the same normalized conformance traces as the cooperative
   implementation.

Until then, the integration point is capability detection plus diagnostics,
not a language-level ABI.
