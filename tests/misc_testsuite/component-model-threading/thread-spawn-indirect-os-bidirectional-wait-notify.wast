;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread synchronization probe.
;;
;; This fixes the narrow guest-visible contract that a parent component thread
;; and an OS-owned child component thread share the same futex wait queues for a
;; rebound shared memory. The child proves it can wake a parent waiter, the
;; parent proves it can observe and wake a child waiter, and the child proves the
;; final parent wait was also observed before completion is published.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $wait-until-ty (shared (func (param i32 i32) (result i32))))
    (type $notify-until-waiter-ty (shared (func (param i32) (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Layout:
    ;;   0: ready flag, child -> parent
    ;;   4: gate flag, parent -> child
    ;;   8: done flag, child -> parent
    ;;  12: scratch futex used only for short timeout-based yielding
    ;;  16: child observed a parent waiter on ready
    ;;  20: parent observed a child waiter on gate
    ;;  24: child observed a parent waiter on done

    (func $wait-until (type $wait-until-ty)
      (local $cur i32)
      (local $attempts i32)
      (loop $again
        (local.set $cur (i32.atomic.load (local.get 0)))
        (if (i32.eq (local.get $cur) (local.get 1))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 200))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (local.get 0)
          (local.get $cur)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func $notify-until-waiter (type $notify-until-waiter-ty)
      (local $woken i32)
      (local $attempts i32)
      (loop $again
        (local.set $woken (memory.atomic.notify (local.get 0) (i32.const 1)))
        (if (i32.gt_u (local.get $woken) (i32.const 0))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 500))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (i32.const 12)
          (i32.const 0)
          (i64.const 100000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func $thread-start (type $start-func-ty)
      (if (i32.eqz (call $notify-until-waiter (i32.const 0)))
        (then
          (return)))
      (i32.atomic.store (i32.const 16) (i32.const 1))
      (i32.atomic.store (i32.const 0) (i32.const 1))
      (drop (memory.atomic.notify (i32.const 0) (i32.const 1)))

      (if (i32.eqz (call $wait-until (i32.const 4) (i32.const 1)))
        (then
          (return)))

      (if (i32.eqz (call $notify-until-waiter (i32.const 8)))
        (then
          (return)))
      (i32.atomic.store (i32.const 24) (i32.const 1))
      (i32.atomic.store (i32.const 8) (i32.const 1))
      (drop (memory.atomic.notify (i32.const 8) (i32.const 1))))
    (export "thread-start" (func $thread-start))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "run") (type $run-ty)
      (i32.atomic.store (i32.const 0) (i32.const 0))
      (i32.atomic.store (i32.const 4) (i32.const 0))
      (i32.atomic.store (i32.const 8) (i32.const 0))
      (i32.atomic.store (i32.const 12) (i32.const 0))
      (i32.atomic.store (i32.const 16) (i32.const 0))
      (i32.atomic.store (i32.const 20) (i32.const 0))
      (i32.atomic.store (i32.const 24) (i32.const 0))

      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))

      (if (i32.eqz (call $wait-until (i32.const 0) (i32.const 1)))
        (then
          (return (i32.const 0))))

      (if (i32.eqz (call $notify-until-waiter (i32.const 4)))
        (then
          (return (i32.const 0))))
      (i32.atomic.store (i32.const 20) (i32.const 1))
      (i32.atomic.store (i32.const 4) (i32.const 1))
      (drop (memory.atomic.notify (i32.const 4) (i32.const 1)))

      (if (i32.eqz (call $wait-until (i32.const 8) (i32.const 1)))
        (then
          (return (i32.const 0))))

      (i32.and
        (i32.and
          (i32.atomic.load (i32.const 16))
          (i32.atomic.load (i32.const 20)))
        (i32.and
          (i32.atomic.load (i32.const 24))
          (i32.atomic.load (i32.const 8))))))

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
