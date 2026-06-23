;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread rejection probe.
;;
;; The growable shared start table is owned by one core instance, while the
;; selected start function is defined by another. The table owner still defines
;; a helper function that can directly observe the owner's inline
;; VMTableDefinition, so the unsafe OS-thread path must reject this shape before
;; spawning a sibling Store.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table $table (export "__indirect_function_table") shared 1
      (ref null (shared func)))
    (type $start-func-ty (shared (func (param i32))))
    (func $helper (export "helper") (type $start-func-ty)
      (drop (table.size $table))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "helper" (func $helper (type $start-func-ty)))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 (ref null (shared func))))

    (func $thread-start (type $start-func-ty)
      (call $helper (local.get 0)))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "run") (type $run-ty)
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (i32.const 0)))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))
  (alias core export $libc "helper" (core func $helper))
  (alias core export $libc "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))))
      (with "libc" (instance
        (export "mem" (memory $mem))
        (export "helper" (func $helper))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_trap
  (invoke "run")
  "component thread preemptive spawn rejected: growable shared table owner")
