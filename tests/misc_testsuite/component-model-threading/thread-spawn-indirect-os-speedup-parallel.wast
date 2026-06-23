;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $available-parallelism-ty (shared (func (result i32))))
    (type $result-addr-ty (func (param i32) (result i32)))
    (type $store-result-ty (func (param i32 i32)))
    (type $load-result-ty (func (param i32) (result i32)))
    (type $spawn-slot-ty (func (param i32)))
    (type $wait-done-ty (func (param i32)))
    (type $sum-results-ty (func (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "" "thread.available-parallelism"
      (func $thread-available-parallelism (type $available-parallelism-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    (func $result-addr (type $result-addr-ty)
      (i32.add
        (i32.const 1024)
        (i32.shl (local.get 0) (i32.const 2))))

    (func $store-result (type $store-result-ty)
      (i32.atomic.store (call $result-addr (local.get 0)) (local.get 1)))

    (func $load-result (type $load-result-ty)
      (i32.atomic.load (call $result-addr (local.get 0))))

    (func $spawn-slot (type $spawn-slot-ty)
      (drop (call $thread-spawn-indirect (i32.const 0) (local.get 0))))

    (func $wait-done (type $wait-done-ty)
      (local $done i32)
      (local $attempts i32)
      (loop $again
        (local.set $done (i32.atomic.load (i32.const 0)))
        (if (i32.ge_u (local.get $done) (local.get 0))
          (then return))
        (if (i32.ge_u (local.get $attempts) (i32.const 1000))
          (then return))
        (drop (memory.atomic.wait32
          (i32.const 0)
          (local.get $done)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again)))

    (func $sum-results (type $sum-results-ty)
      (i32.add
        (i32.add
          (call $load-result (i32.const 0))
          (call $load-result (i32.const 1)))
        (i32.add
          (call $load-result (i32.const 2))
          (call $load-result (i32.const 3)))))

    (func $thread-start (type $start-func-ty)
      (local $slot i32)
      (local $i i32)
      (local $x i32)

      (local.set $slot (local.get 0))
      (local.set $i (i32.const 0))
      (local.set $x (i32.add (local.get $slot) (i32.const 1)))
      (loop $again
        (local.set $x
          (i32.add
            (i32.mul
              (local.get $x)
              (i32.const 1664525))
            (i32.const 1013904223)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $again (i32.lt_u (local.get $i) (i32.const 50000000))))

      (i32.atomic.store
        (i32.add
          (i32.const 1024)
          (i32.shl (local.get $slot) (i32.const 2)))
        (local.get $x))
      (drop (i32.atomic.rmw.add (i32.const 0) (i32.const 1)))
      (drop (memory.atomic.notify (i32.const 0) (i32.const 1))))
    (export "thread-start" (func $thread-start))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "available-positive") (type $run-ty)
      (i32.gt_u (call $thread-available-parallelism) (i32.const 0)))

    (func (export "parallel") (type $run-ty)
      (i32.atomic.store (i32.const 0) (i32.const 0))
      (call $store-result (i32.const 0) (i32.const 0))
      (call $store-result (i32.const 1) (i32.const 0))
      (call $store-result (i32.const 2) (i32.const 0))
      (call $store-result (i32.const 3) (i32.const 0))

      (call $spawn-slot (i32.const 0))
      (call $spawn-slot (i32.const 1))
      (call $spawn-slot (i32.const 2))
      (call $spawn-slot (i32.const 3))
      (call $wait-done (i32.const 4))
      (call $sum-results)))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))
  (alias core export $libc "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))
  (core func $thread-available-parallelism
    (canon thread.available_parallelism))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))
        (export "thread.available-parallelism" (func $thread-available-parallelism))))
      (with "libc" (instance
        (export "mem" (memory $mem))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "available-positive") async (result u32)
    (canon lift (core func $i "available-positive")))
  (func (export "parallel") async (result u32)
    (canon lift (core func $i "parallel"))))

(assert_return (invoke "available-positive") (u32.const 1))
(assert_return (invoke "parallel") (u32.const 1106140682))
