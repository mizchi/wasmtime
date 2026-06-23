;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread phased barrier/reduction probe.
;;
;; Four OS-owned component threads publish phase-1 contributions, rendezvous at
;; a sense-reversing barrier, then enter phase 2 only after every worker can see
;; the complete phase-1 reduction. This pins a child-to-child cooperation shape,
;; not just parent-child wakeups.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $wait-at-least-ty (shared (func (param i32 i32) (result i32))))
    (type $slot-addr-ty (shared (func (param i32 i32) (result i32))))
    (type $barrier-ty (shared (func (result i32))))
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
    ;;  12: barrier arrival count
    ;;  16: barrier generation
    ;;  20: phase-1 total
    ;;  24: phase-2 total
    ;;
    ;; Worker slots start at 1024, 16 bytes per worker:
    ;;  +0: phase-1 contribution
    ;;  +4: phase-2 contribution

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

    (func $barrier (type $barrier-ty)
      (local $generation i32)
      (local $arrival i32)
      (local.set $generation (i32.atomic.load (i32.const 16)))
      (local.set $arrival (i32.atomic.rmw.add (i32.const 12) (i32.const 1)))
      (if (i32.eq (local.get $arrival) (i32.const 3))
        (then
          (i32.atomic.store (i32.const 12) (i32.const 0))
          (i32.atomic.store
            (i32.const 16)
            (i32.add (local.get $generation) (i32.const 1)))
          (drop (memory.atomic.notify (i32.const 16) (i32.const 4)))
          (return (i32.const 1))))
      (call $wait-at-least
        (i32.const 16)
        (i32.add (local.get $generation) (i32.const 1))))

    (func $thread-start (type $start-func-ty)
      (local $worker i32)
      (local $phase1 i32)
      (local $phase2 i32)

      (local.set $worker (local.get 0))
      (drop (i32.atomic.rmw.add (i32.const 4) (i32.const 1)))
      (drop (memory.atomic.notify (i32.const 4) (i32.const 1)))

      (if (i32.eqz (call $wait-at-least (i32.const 0) (i32.const 1)))
        (then
          (return)))

      ;; Contributions are 10, 20, 30, 40; phase-1 total is 100.
      (local.set $phase1
        (i32.mul
          (i32.add (local.get $worker) (i32.const 1))
          (i32.const 10)))
      (i32.atomic.store
        (call $slot-addr (local.get $worker) (i32.const 0))
        (local.get $phase1))
      (drop (i32.atomic.rmw.add (i32.const 20) (local.get $phase1)))

      (if (i32.eqz (call $barrier))
        (then
          (return)))

      ;; Every worker must observe the complete phase-1 total after the barrier.
      ;; Sum phase2 = 4*100 + 100 + (0+1+2+3) = 506.
      (local.set $phase2
        (i32.add
          (i32.add
            (i32.atomic.load (i32.const 20))
            (local.get $phase1))
          (local.get $worker)))
      (i32.atomic.store
        (call $slot-addr (local.get $worker) (i32.const 4))
        (local.get $phase2))
      (drop (i32.atomic.rmw.add (i32.const 24) (local.get $phase2)))
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
      (i32.atomic.store (i32.const 24) (i32.const 0))
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
          (i32.eq (i32.atomic.load (i32.const 12)) (i32.const 0))
          (i32.eq (i32.atomic.load (i32.const 16)) (i32.const 1)))
        (i32.and
          (i32.and
            (i32.eq (i32.atomic.load (i32.const 20)) (i32.const 100))
            (i32.eq (call $sum-slots (i32.const 0)) (i32.const 100)))
          (i32.and
            (i32.eq (i32.atomic.load (i32.const 24)) (i32.const 506))
            (i32.eq (call $sum-slots (i32.const 4)) (i32.const 506)))))))

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
