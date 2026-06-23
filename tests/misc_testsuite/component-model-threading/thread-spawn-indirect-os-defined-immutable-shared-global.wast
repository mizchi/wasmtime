;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread probe for a start function defined in the same
;; core instance as an immutable shared global. The immutable value is copied
;; into the child sibling instance and can be read directly without live shared
;; global mutation.

(component
  (core module $state
    (memory (export "mem") 1 1 shared)
    (global $shared-global (shared i32) (i32.const 7))
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func)))

    (type $start-func-ty (shared (func (param i32))))
    (func $thread-start (type $start-func-ty)
      (i32.atomic.store (i32.const 0) (global.get $shared-global))
      (drop (memory.atomic.notify (i32.const 0) (i32.const 1))))

    (elem (table 0) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start)))

  (core module $m
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "state" "mem" (memory $mem 1 1 shared))

    (func (export "run") (type $run-ty)
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (drop (memory.atomic.wait32
        (i32.const 0)
        (i32.const 0)
        (i64.const 10000000)))
      (i32.atomic.load (i32.const 0))))

  (core instance $state (instantiate $state))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $state "mem" (core memory $mem))
  (alias core export $state "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))))
      (with "state" (instance
        (export "mem" (memory $mem))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 7))
