;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 2 2 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $yield-ty (func (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "" "thread.yield" (func $thread-yield (type $yield-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 2 2 (ref null (shared func))))

    (func $thread-start-old (type $start-func-ty)
      (i32.store (i32.const 0) (i32.const 1)))
    (func $thread-start-new (type $start-func-ty)
      (i32.store (i32.const 0) (i32.const 2)))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func))
      (ref.func $thread-start-old)
      (ref.func $thread-start-new))

    (func (export "run") (type $run-ty)
      (local $i i32)
      (i32.store (i32.const 0) (i32.const 0))
      (table.set $indirect-function-table
        (i32.const 0)
        (table.get $indirect-function-table (i32.const 1)))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (loop $again
        (if (i32.eqz (i32.load (i32.const 0)))
          (then
            (drop (call $thread-yield))
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (br_if $again (i32.lt_u (local.get $i) (i32.const 1000))))))
      (i32.load (i32.const 0))))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))
  (alias core export $libc "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))
  (core func $thread-yield (canon thread.yield))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))
        (export "thread.yield" (func $thread-yield))))
      (with "libc" (instance
        (export "mem" (memory $mem))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 2))
