;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local Vibe ABI guard.
;;
;; The canonical `thread.spawn-indirect` return value is a component
;; thread-table index. The Vibe task/result slot is instead selected by the
;; single `i32` context pointer passed to the start function.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $finish-ty (shared (func (param i32 i32))))
    (type $wait-state-ty (func (param i32) (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Slot layout selected by the context argument:
    ;;   +0: state, 0 = empty, 1 = terminal
    ;;   +4: payload, here just context + 7

    (func $slot-addr (type $slot-addr-ty)
      (i32.add (local.get 0) (local.get 1)))

    (func $finish (type $finish-ty)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 4))
        (local.get 1))
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))
      (drop (memory.atomic.notify
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))))

    (func $thread-trampoline (type $start-func-ty)
      (call $finish
        (local.get 0)
        (i32.add (local.get 0) (i32.const 7))))
    (export "thread-trampoline" (func $thread-trampoline))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-trampoline))

    (func $wait-state (type $wait-state-ty)
      (local $state i32)
      (local $attempts i32)
      (loop $again
        (local.set $state (i32.atomic.load (local.get 0)))
        (if (i32.eq (local.get $state) (i32.const 1))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 200))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (local.get 0)
          (local.get $state)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func (export "run") (type $run-ty)
      (local $handle i32)
      (local $slot i32)
      (local.set $slot (i32.const 128))
      (i32.atomic.store (local.get $slot) (i32.const 0))
      (i32.atomic.store
        (call $slot-addr (local.get $slot) (i32.const 4))
        (i32.const 0))

      (local.set $handle
        (call $thread-spawn-indirect (i32.const 0) (local.get $slot)))

      ;; This is the actual guard: the canonical handle and the Vibe slot
      ;; pointer are separate domains. Completion must come from the slot.
      (if (i32.eq (local.get $handle) (local.get $slot))
        (then
          (return (i32.const 10))))

      (if (i32.eqz (call $wait-state (local.get $slot)))
        (then
          (return (i32.const 20))))

      (if
        (i32.ne
          (i32.atomic.load
            (call $slot-addr (local.get $slot) (i32.const 4)))
          (i32.add (local.get $slot) (i32.const 7)))
        (then
          (return (i32.const 30))))
      (if
        (i32.eq
          (local.get $handle)
          (i32.atomic.load
            (call $slot-addr (local.get $slot) (i32.const 4))))
        (then
          (return (i32.const 40))))
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
