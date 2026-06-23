(module
  (import "" "memory" (memory $mem 1 1 shared))
  (import "wasi" "thread-spawn" (func $thread_spawn (param i32) (result i32)))

  (global $iters i64 (i64.const 50000000))

  (func $work (param $seed i64) (result i64)
    (local $i i64)
    (local $x i64)

    (local.set $i (i64.const 0))
    (local.set $x (i64.add (local.get $seed) (i64.const 1)))
    (loop $again
      (local.set $x
        (i64.add
          (i64.mul
            (local.get $x)
            (i64.const 6364136223846793005))
          (i64.const 1442695040888963407)))
      (local.set $i (i64.add (local.get $i) (i64.const 1)))
      (br_if $again (i64.lt_u (local.get $i) (global.get $iters))))

    (local.get $x))

  (func $result_addr (param $slot i32) (result i32)
    (i32.add
      (i32.const 1024)
      (i32.shl (local.get $slot) (i32.const 3))))

  (func $store_result (param $slot i32) (param $value i64)
    (i64.atomic.store (call $result_addr (local.get $slot)) (local.get $value)))

  (func $load_result (param $slot i32) (result i64)
    (i64.atomic.load (call $result_addr (local.get $slot))))

  (func $spawn_slot (param $slot i32)
    (local $tid i32)
    (local.set $tid (call $thread_spawn (local.get $slot)))
    (if (i32.lt_s (local.get $tid) (i32.const 0))
      (then unreachable)))

  (func $wait_done (param $target i32)
    (local $done i32)
    (loop $again
      (local.set $done (i32.atomic.load (i32.const 0)))
      (if (i32.lt_u (local.get $done) (local.get $target))
        (then
          (drop (memory.atomic.wait32
            (i32.const 0)
            (local.get $done)
            (i64.const 100000000)))
          (br $again)))))

  (func $sum_results (result i64)
    (i64.add
      (i64.add
        (call $load_result (i32.const 0))
        (call $load_result (i32.const 1)))
      (i64.add
        (call $load_result (i32.const 2))
        (call $load_result (i32.const 3)))))

  (func (export "wasi_thread_start") (param $tid i32) (param $start_arg i32)
    (call $store_result
      (local.get $start_arg)
      (call $work (i64.extend_i32_u (local.get $start_arg))))
    (drop (i32.atomic.rmw.add (i32.const 0) (i32.const 1)))
    (drop (memory.atomic.notify (i32.const 0) (i32.const 1))))

  (func (export "serial") (result i64)
    (i64.add
      (i64.add
        (call $work (i64.const 0))
        (call $work (i64.const 1)))
      (i64.add
        (call $work (i64.const 2))
        (call $work (i64.const 3)))))

  (func (export "parallel") (result i64)
    (i32.atomic.store (i32.const 0) (i32.const 0))
    (call $store_result (i32.const 0) (i64.const 0))
    (call $store_result (i32.const 1) (i64.const 0))
    (call $store_result (i32.const 2) (i64.const 0))
    (call $store_result (i32.const 3) (i64.const 0))

    (call $spawn_slot (i32.const 0))
    (call $spawn_slot (i32.const 1))
    (call $spawn_slot (i32.const 2))
    (call $spawn_slot (i32.const 3))
    (call $wait_done (i32.const 4))
    (call $sum_results))

  (export "memory" (memory $mem)))
