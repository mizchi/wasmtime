;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread language-runtime completion probe.
;;
;; This models the Vibe-facing direction after re-reading the Component Model
;; threading contract: the value returned by `thread.spawn-indirect` is a
;; transient component thread-table index, so this probe drops it. Completion is
;; instead produced by a generated trampoline that writes a shared-memory slot
;; and notifies parent waiters.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $worker-ty (shared (func (param i32) (result i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $wait-completed-ty (func (param i32) (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Slot layout, addressed by the context argument:
    ;;   +0: state, 0 = empty, 1 = running, 2 = completed
    ;;   +4: result
    ;;   +8: input
    ;;  +12: reserved

    (func $slot-addr (type $slot-addr-ty)
      (i32.add (local.get 0) (local.get 1)))

    (func $worker (type $worker-ty)
      (local $input i32)
      (local.set $input
        (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 8))))
      (i32.add
        (i32.mul (local.get $input) (local.get $input))
        (i32.const 7)))

    (func $thread-trampoline (type $start-func-ty)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 4))
        (call $worker (local.get 0)))
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 2))
      (drop (memory.atomic.notify
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))))
    (export "thread-trampoline" (func $thread-trampoline))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-trampoline))

    (func $wait-completed (type $wait-completed-ty)
      (local $state i32)
      (local $attempts i32)
      (loop $again
        (local.set $state
          (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 0))))
        (if (i32.eq (local.get $state) (i32.const 2))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 200))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (call $slot-addr (local.get 0) (i32.const 0))
          (local.get $state)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func (export "run") (type $run-ty)
      ;; slot 0, input 11
      (i32.atomic.store (i32.const 0) (i32.const 0))
      (i32.atomic.store (i32.const 4) (i32.const 0))
      (i32.atomic.store (i32.const 8) (i32.const 11))
      ;; slot 1, input 13
      (i32.atomic.store (i32.const 16) (i32.const 0))
      (i32.atomic.store (i32.const 20) (i32.const 0))
      (i32.atomic.store (i32.const 24) (i32.const 13))

      ;; The canonical spawn return values are intentionally ignored. Vibe-level
      ;; completion is represented by the trampoline-owned shared slots.
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 16)))

      (if (i32.eqz (call $wait-completed (i32.const 0)))
        (then
          (return (i32.const 0))))
      (if (i32.eqz (call $wait-completed (i32.const 16)))
        (then
          (return (i32.const 0))))

      (i32.eq
        (i32.add
          (i32.atomic.load (i32.const 4))
          (i32.atomic.load (i32.const 20)))
        (i32.const 304))))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))
  (alias core export $libc "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))))
      (with "libc" (instance
        (export "mem" (memory $mem))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 1))
