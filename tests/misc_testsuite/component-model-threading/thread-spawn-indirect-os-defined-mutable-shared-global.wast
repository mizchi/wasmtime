;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread probe.
;;
;; The shared start function is defined in the same core instance as a mutable
;; shared global and writes that global directly. The unsafe OS-thread child
;; runs in a sibling Store, so the fork must flush the child's direct defined
;; global value back to the parent definition after the start function returns.

(component
  (core module $state
    (memory (export "mem") 1 1 shared)
    (global $shared-global (export "shared-global")
      (shared mut i32) (i32.const 7))
    (table (export "__indirect_function_table") shared 1 1
      (ref null (shared func)))
    (type $start-func-ty (shared (func (param i32))))
    (func $thread-start (type $start-func-ty)
      (global.set $shared-global (local.get 0))
      (i32.atomic.store (i32.const 0) (i32.const 1)))
    (elem (table 0) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start)))

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

    (func (export "run") (type $run-ty)
      (local $i i32)
      (i32.store (i32.const 0) (i32.const 0))
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 42)))
      (loop $again
        (if (i32.ne (global.get $shared-global) (i32.const 42))
          (then
            (drop (call $thread-yield))
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (br_if $again (i32.lt_u (local.get $i) (i32.const 1000))))))
      (global.get $shared-global)))

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

(assert_return (invoke "run") (u32.const 42))
