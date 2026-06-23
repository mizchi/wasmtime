;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread Vibe runtime ABI probe.
;;
;; This consolidates the separate completion/status/cancellation probes into one
;; generated-trampoline shape. The parent drops every canonical
;; `thread.spawn-indirect` return value and observes only trampoline-owned shared
;; slots for join, terminal status, result payloads, failure-as-a-value, and
;; in-flight cooperative cancellation.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $finish-ty (shared (func (param i32 i32 i32))))
    (type $wait-cancel-ty (shared (func (param i32) (result i32))))
    (type $wait-state-ty (func (param i32 i32) (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Slot layout, addressed by the context argument:
    ;;   +0: state, 0 = empty, 1 = running, 2 = terminal
    ;;   +4: terminal code, 0 = completed, 1 = cancelled, 2 = failed
    ;;   +8: payload, result for completed or error code for failed
    ;;  +16: input
    ;;  +20: cancel request flag
    ;;  +24: mode, 0 = compute, 1 = fail-as-value, 2 = wait-cancel

    (func $slot-addr (type $slot-addr-ty)
      (i32.add (local.get 0) (local.get 1)))

    (func $finish (type $finish-ty)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 4))
        (local.get 1))
      (i64.atomic.store
        (call $slot-addr (local.get 0) (i32.const 8))
        (i64.extend_i32_u (local.get 2)))
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 2))
      (drop (memory.atomic.notify
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))))

    (func $wait-cancel (type $wait-cancel-ty)
      (local $flag i32)
      (local $attempts i32)
      (loop $again
        (local.set $flag
          (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 20))))
        (if (i32.ne (local.get $flag) (i32.const 0))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 200))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (call $slot-addr (local.get 0) (i32.const 20))
          (i32.const 0)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func $thread-trampoline (type $start-func-ty)
      (local $input i32)
      (local $mode i32)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))
      (drop (memory.atomic.notify
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1)))

      (if
        (i32.ne
          (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 20)))
          (i32.const 0))
        (then
          (call $finish (local.get 0) (i32.const 1) (i32.const 0))
          (return)))

      (local.set $input
        (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 16))))
      (local.set $mode
        (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 24))))

      (if (i32.eq (local.get $mode) (i32.const 1))
        (then
          (call $finish
            (local.get 0)
            (i32.const 2)
            (i32.add (i32.const 1000) (local.get $input)))
          (return)))

      (if (i32.eq (local.get $mode) (i32.const 2))
        (then
          (if (i32.eqz (call $wait-cancel (local.get 0)))
            (then
              (call $finish (local.get 0) (i32.const 2) (i32.const 9000))
              (return)))
          (call $finish (local.get 0) (i32.const 1) (i32.const 0))
          (return)))

      (call $finish
        (local.get 0)
        (i32.const 0)
        (i32.add
          (i32.mul (local.get $input) (local.get $input))
          (i32.const 7))))
    (export "thread-trampoline" (func $thread-trampoline))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-trampoline))

    (func $wait-state (type $wait-state-ty)
      (local $state i32)
      (local $attempts i32)
      (loop $again
        (local.set $state
          (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 0))))
        (if (i32.eq (local.get $state) (local.get 1))
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

    (func $init-slot (param $slot i32) (param $input i32) (param $mode i32)
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 0)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 4)) (i32.const 0))
      (i64.atomic.store (call $slot-addr (local.get $slot) (i32.const 8)) (i64.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 16)) (local.get $input))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 20)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 24)) (local.get $mode)))

    (func (export "run") (type $run-ty)
      ;; slot 0: completed, input 11, payload 128
      (call $init-slot (i32.const 0) (i32.const 11) (i32.const 0))
      ;; slot 1: completed, input 5, payload 32
      (call $init-slot (i32.const 32) (i32.const 5) (i32.const 0))
      ;; slot 2: failed-as-value, input 13, payload 1013
      (call $init-slot (i32.const 64) (i32.const 13) (i32.const 1))
      ;; slot 3: in-flight cooperative cancellation
      (call $init-slot (i32.const 96) (i32.const 17) (i32.const 2))

      ;; The canonical spawn return values are intentionally ignored. Vibe-level
      ;; completion and cancellation are represented by the trampoline slots.
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 32)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 64)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 96)))

      (if (i32.eqz (call $wait-state (i32.const 96) (i32.const 1)))
        (then
          (return (i32.const 0))))
      (i32.atomic.store (i32.const 116) (i32.const 1))
      (drop (memory.atomic.notify (i32.const 116) (i32.const 1)))

      (if (i32.eqz (call $wait-state (i32.const 0) (i32.const 2)))
        (then
          (return (i32.const 0))))
      (if (i32.eqz (call $wait-state (i32.const 32) (i32.const 2)))
        (then
          (return (i32.const 0))))
      (if (i32.eqz (call $wait-state (i32.const 64) (i32.const 2)))
        (then
          (return (i32.const 0))))
      (if (i32.eqz (call $wait-state (i32.const 96) (i32.const 2)))
        (then
          (return (i32.const 0))))

      (i32.and
        (i32.and
          (i32.and
            (i32.eq (i32.atomic.load (i32.const 4)) (i32.const 0))
            (i64.eq (i64.atomic.load (i32.const 8)) (i64.const 128)))
          (i32.and
            (i32.eq (i32.atomic.load (i32.const 36)) (i32.const 0))
            (i64.eq (i64.atomic.load (i32.const 40)) (i64.const 32))))
        (i32.and
          (i32.and
            (i32.eq (i32.atomic.load (i32.const 68)) (i32.const 2))
            (i64.eq (i64.atomic.load (i32.const 72)) (i64.const 1013)))
          (i32.and
            (i32.eq (i32.atomic.load (i32.const 100)) (i32.const 1))
            (i32.eq
              (i32.add
                (i32.wrap_i64 (i64.atomic.load (i32.const 8)))
                (i32.wrap_i64 (i64.atomic.load (i32.const 40))))
              (i32.const 160)))))))

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
