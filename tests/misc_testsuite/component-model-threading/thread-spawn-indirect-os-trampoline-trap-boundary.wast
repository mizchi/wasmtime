;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread language-runtime trap-boundary probe.
;;
;; This pins a negative part of the Vibe-facing trampoline contract: a real
;; Wasm trap inside the spawned start function is not caught and lowered into
;; the trampoline-owned shared status slot. Only producer-generated status
;; writes are Vibe guest ABI. Host diagnostics still record the child trap as a
;; failed unsafe OS thread and parent cleanup surfaces that failure. The guest
;; shared slot remains non-terminal and should not be treated as a Vibe
;; `failed` value.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $wait-timeout-ty (func (param i32) (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Slot layout:
    ;;   +0: state, 0 = empty, 1 = running, 2 = terminal
    ;;   +4: terminal code/result payload, written only by generated runtime code

    (func $slot-addr (type $slot-addr-ty)
      (i32.add (local.get 0) (local.get 1)))

    (func $thread-trampoline (type $start-func-ty)
      (i32.atomic.store
        (call $slot-addr (local.get 0) (i32.const 0))
        (i32.const 1))
      unreachable)
    (export "thread-trampoline" (func $thread-trampoline))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-trampoline))

    (func $wait-timeout (type $wait-timeout-ty)
      (local $state i32)
      (local $attempts i32)
      (loop $again
        (local.set $state
          (i32.atomic.load (call $slot-addr (local.get 0) (i32.const 0))))
        (if (i32.eq (local.get $state) (i32.const 2))
          (then
            (return (i32.const 0))))
        (if (i32.ge_u (local.get $attempts) (i32.const 20))
          (then
            (return (i32.const 1))))
        (drop (memory.atomic.wait32
          (call $slot-addr (local.get 0) (i32.const 0))
          (local.get $state)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 1))

    (func (export "run") (type $run-ty)
      (i32.atomic.store (i32.const 0) (i32.const 0))
      (i32.atomic.store (i32.const 4) (i32.const 0))

      ;; The canonical spawn return value is intentionally ignored. A real trap
      ;; in the child must not synthesize a Vibe terminal status in shared
      ;; memory.
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))

      (if (i32.eqz (call $wait-timeout (i32.const 0)))
        (then
          (return (i32.const 0))))

      (i32.and
        (i32.eq (i32.atomic.load (i32.const 0)) (i32.const 1))
        (i32.eq (i32.atomic.load (i32.const 4)) (i32.const 0)))))

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

(assert_trap (invoke "run") "unsafe Component Model OS thread failed")
