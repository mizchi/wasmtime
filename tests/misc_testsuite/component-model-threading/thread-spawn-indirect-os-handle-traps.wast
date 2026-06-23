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
    (type $to-suspended-ty (func (param i32) (result i32)))
    (type $unsuspend-ty (func (param i32)))
    (type $run-ty (func))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "" "thread.yield-to-suspended"
      (func $thread-yield-to-suspended (type $to-suspended-ty)))
    (import "" "thread.suspend-to-suspended"
      (func $thread-suspend-to-suspended (type $to-suspended-ty)))
    (import "" "thread.unsuspend"
      (func $thread-unsuspend (type $unsuspend-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    (func $thread-start (type $start-func-ty))
    (export "thread-start" (func $thread-start))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "yield-to-suspended") (type $run-ty)
      (drop
        (call $thread-yield-to-suspended
          (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))))

    (func (export "suspend-to-suspended") (type $run-ty)
      (drop
        (call $thread-suspend-to-suspended
          (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))))

    (func (export "unsuspend") (type $run-ty)
      (call $thread-unsuspend
        (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))
  (alias core export $libc "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))
  (core func $thread-yield-to-suspended (canon thread.yield-to-suspended))
  (core func $thread-suspend-to-suspended (canon thread.suspend-to-suspended))
  (core func $thread-unsuspend (canon thread.unsuspend))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))
        (export "thread.yield-to-suspended" (func $thread-yield-to-suspended))
        (export "thread.suspend-to-suspended" (func $thread-suspend-to-suspended))
        (export "thread.unsuspend" (func $thread-unsuspend))))
      (with "libc" (instance
        (export "mem" (memory $mem))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "yield-to-suspended") async
    (canon lift (core func $i "yield-to-suspended")))
  (func (export "suspend-to-suspended") async
    (canon lift (core func $i "suspend-to-suspended")))
  (func (export "unsuspend") async
    (canon lift (core func $i "unsuspend"))))

(assert_trap (invoke "yield-to-suspended") "cannot resume thread which is not suspended")
