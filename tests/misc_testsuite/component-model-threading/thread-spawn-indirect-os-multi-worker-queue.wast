;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread multi-worker queue probe.
;;
;; Four OS-owned component threads wait on a shared start gate, then contend on
;; a shared atomic job counter. Each worker processes a non-deterministic subset
;; of 16 deterministic jobs and publishes per-worker counts/sums. The parent
;; waits for all workers and verifies both global and per-worker aggregates.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $wait-at-least-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $job-value-ty (shared (func (param i32) (result i32))))
    (type $sum-slots-ty (func (param i32) (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Global layout:
    ;;   0: start gate, 0 = blocked, 1 = released
    ;;   4: ready worker counter
    ;;   8: done worker counter
    ;;  12: next job counter
    ;;  16: global checksum
    ;;  20: global completed job count
    ;;
    ;; Worker slots start at 1024, 16 bytes per worker:
    ;;  +0: completed job count
    ;;  +4: checksum

    (func $wait-at-least (type $wait-at-least-ty)
      (local $cur i32)
      (local $attempts i32)
      (loop $again
        (local.set $cur (i32.atomic.load (local.get 0)))
        (if (i32.ge_u (local.get $cur) (local.get 1))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 500))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (local.get 0)
          (local.get $cur)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func $slot-addr (type $slot-addr-ty)
      (i32.add
        (i32.const 1024)
        (i32.add
          (i32.shl (local.get 0) (i32.const 4))
          (local.get 1))))

    ;; job^2 + 3*job + 7; sum(job=0..15) = 1712.
    (func $job-value (type $job-value-ty)
      (i32.add
        (i32.add
          (i32.mul (local.get 0) (local.get 0))
          (i32.mul (local.get 0) (i32.const 3)))
        (i32.const 7)))

    (func $thread-start (type $start-func-ty)
      (local $worker i32)
      (local $job i32)
      (local $count i32)
      (local $sum i32)
      (local $value i32)

      (local.set $worker (local.get 0))
      (drop (i32.atomic.rmw.add (i32.const 4) (i32.const 1)))
      (drop (memory.atomic.notify (i32.const 4) (i32.const 1)))

      (if (i32.eqz (call $wait-at-least (i32.const 0) (i32.const 1)))
        (then
          (return)))

      (block $done
        (loop $again
          (local.set $job
            (i32.atomic.rmw.add (i32.const 12) (i32.const 1)))
          (br_if $done (i32.ge_u (local.get $job) (i32.const 16)))
          (local.set $value (call $job-value (local.get $job)))
          (local.set $count (i32.add (local.get $count) (i32.const 1)))
          (local.set $sum (i32.add (local.get $sum) (local.get $value)))
          (br $again)))

      (i32.atomic.store
        (call $slot-addr (local.get $worker) (i32.const 0))
        (local.get $count))
      (i32.atomic.store
        (call $slot-addr (local.get $worker) (i32.const 4))
        (local.get $sum))
      (drop (i32.atomic.rmw.add (i32.const 20) (local.get $count)))
      (drop (i32.atomic.rmw.add (i32.const 16) (local.get $sum)))
      (drop (i32.atomic.rmw.add (i32.const 8) (i32.const 1)))
      (drop (memory.atomic.notify (i32.const 8) (i32.const 1))))
    (export "thread-start" (func $thread-start))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func $sum-slots (type $sum-slots-ty)
      (i32.add
        (i32.add
          (i32.atomic.load (call $slot-addr (i32.const 0) (local.get 0)))
          (i32.atomic.load (call $slot-addr (i32.const 1) (local.get 0))))
        (i32.add
          (i32.atomic.load (call $slot-addr (i32.const 2) (local.get 0)))
          (i32.atomic.load (call $slot-addr (i32.const 3) (local.get 0))))))

    (func (export "run") (type $run-ty)
      (i32.atomic.store (i32.const 0) (i32.const 0))
      (i32.atomic.store (i32.const 4) (i32.const 0))
      (i32.atomic.store (i32.const 8) (i32.const 0))
      (i32.atomic.store (i32.const 12) (i32.const 0))
      (i32.atomic.store (i32.const 16) (i32.const 0))
      (i32.atomic.store (i32.const 20) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 0) (i32.const 0)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 0) (i32.const 4)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 1) (i32.const 0)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 1) (i32.const 4)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 2) (i32.const 0)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 2) (i32.const 4)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 3) (i32.const 0)) (i32.const 0))
      (i32.atomic.store (call $slot-addr (i32.const 3) (i32.const 4)) (i32.const 0))

      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 1)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 2)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 3)))

      (if (i32.eqz (call $wait-at-least (i32.const 4) (i32.const 4)))
        (then
          (return (i32.const 0))))

      (i32.atomic.store (i32.const 0) (i32.const 1))
      (drop (memory.atomic.notify (i32.const 0) (i32.const 4)))

      (if (i32.eqz (call $wait-at-least (i32.const 8) (i32.const 4)))
        (then
          (return (i32.const 0))))

      (i32.and
        (i32.and
          (i32.eq (i32.atomic.load (i32.const 20)) (i32.const 16))
          (i32.eq (i32.atomic.load (i32.const 16)) (i32.const 1712)))
        (i32.and
          (i32.eq (call $sum-slots (i32.const 0)) (i32.const 16))
          (i32.eq (call $sum-slots (i32.const 4)) (i32.const 1712))))))

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
