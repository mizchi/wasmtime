;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local Vibe shared-value ABI guard.
;;
;; The spawned start function receives only an `i32` context pointer. It reads a
;; full 64-bit tagged value from a channel-like shared cell and publishes a full
;; 64-bit tagged value through the task slot payload. No canonical thread
;; builtin carries the channel value or the task result.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $finish-ty (shared (func (param i32 i64))))
    (type $wait-state-ty (func (param i32) (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Slot layout, addressed by the context argument:
    ;;   +0: state, 0 = empty, 1 = running, 2 = terminal
    ;;   +4: terminal code
    ;;   +8: payload, full 64-bit tagged Vibe value
    ;;  +16: input, here an i32 pointer to a channel-like 64-bit cell
    ;;  +20: cancel
    ;;  +24: mode
    ;;  +28: worker_func

    (func $slot-addr (type $slot-addr-ty)
      (i32.add (local.get 0) (local.get 1)))

    (func $finish (type $finish-ty)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 4))
        (i32.const 0))
      (i64.atomic.store
        (call $slot-addr (local.get 0) (i32.const 8))
        (local.get 1))
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 2))
      (drop (memory.atomic.notify
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))))

    (func $thread-trampoline (type $start-func-ty)
      (local $cell i32)
      (local $value i64)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))
      (local.set $cell
        (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 16))))
      (local.set $value (i64.atomic.load (local.get $cell)))
      (call $finish
        (local.get 0)
        (i64.add (local.get $value) (i64.const 0x100000000))))
    (export "thread-trampoline" (func $thread-trampoline))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-trampoline))

    (func $wait-state (type $wait-state-ty)
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
      (local $handle i32)
      (local $slot i32)
      (local $cell i32)
      (local.set $slot (i32.const 128))
      (local.set $cell (i32.const 256))

      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 0)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 4)) (i32.const 0))
      (i64.atomic.store (call $slot-addr (local.get $slot) (i32.const 8)) (i64.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 16)) (local.get $cell))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 20)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 24)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (local.get $slot) (i32.const 28)) (i32.const 0))

      ;; This high-bit value would fail the probe if either the channel-like
      ;; cell or task slot payload were accidentally truncated to i32.
      (i64.atomic.store (local.get $cell) (i64.const 0x200000034))

      (local.set $handle
        (call $thread-spawn-indirect (i32.const 0) (local.get $slot)))

      (if (i32.eq (local.get $handle) (local.get $slot))
        (then
          (return (i32.const 10))))
      (if (i32.eqz (call $wait-state (local.get $slot)))
        (then
          (return (i32.const 20))))
      (if
        (i32.ne
          (i32.atomic.load (call $slot-addr (local.get $slot) (i32.const 4)))
          (i32.const 0))
        (then
          (return (i32.const 30))))
      (if
        (i64.ne
          (i64.atomic.load (call $slot-addr (local.get $slot) (i32.const 8)))
          (i64.const 0x300000034))
        (then
          (return (i32.const 40))))
      (if
        (i32.eq
          (local.get $handle)
          (i32.wrap_i64
            (i64.atomic.load (call $slot-addr (local.get $slot) (i32.const 8)))))
        (then
          (return (i32.const 50))))
      (i32.const 1)))

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
