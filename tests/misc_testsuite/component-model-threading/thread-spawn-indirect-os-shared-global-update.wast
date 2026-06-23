;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

(component
  (core module $state
    (memory (export "mem") 1 1 shared)
    (global (export "shared-global") (shared mut i32) (i32.const 0))
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $yield-ty (func (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "" "thread.yield" (func $thread-yield (type $yield-ty)))
    (import "state" "mem" (memory $mem 1 1 shared))
    (import "state" "shared-global" (global $shared-global (shared mut i32)))
    (import "state" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    (func $thread-start (type $start-func-ty)
      (i32.store (i32.const 0) (global.get $shared-global)))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "run") (type $run-ty)
      (local $i i32)
      (i32.store (i32.const 0) (i32.const 0))
      (global.set $shared-global (i32.const 37))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (loop $again
        (if (i32.eqz (i32.load (i32.const 0)))
          (then
            (drop (call $thread-yield))
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (br_if $again (i32.lt_u (local.get $i) (i32.const 1000))))))
      (i32.load (i32.const 0))))

  (core instance $state (instantiate $state))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $state "mem" (core memory $mem))
  (alias core export $state "shared-global" (core global $shared-global))
  (alias core export $state "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))
  (core func $thread-yield (canon thread.yield))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))
        (export "thread.yield" (func $thread-yield))))
      (with "state" (instance
        (export "mem" (memory $mem))
        (export "shared-global" (global $shared-global))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 37))
