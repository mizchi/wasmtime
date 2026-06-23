;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local unsafe OS-thread thread-table index probe.
;;
;; An OS-owned child runs in a sibling Store, but `thread.index` still needs to
;; return the transient component thread-table index that the parent received
;; from `thread.spawn-indirect`.
;; This covers the fork-local shared `canon thread.index` compatibility path
;; and the child-side remapping of shared-table start functions to sibling
;; Store VMFuncRefs.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $thread-index-ty (shared (func (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "" "thread.index" (func $thread-index (type $thread-index-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    ;; Layout:
    ;;   0: child-observed thread.index
    ;;   4: child done flag

    (func $wait-done (result i32)
      (local $cur i32)
      (local $attempts i32)
      (loop $again
        (local.set $cur (i32.atomic.load (i32.const 4)))
        (if (i32.eq (local.get $cur) (i32.const 1))
          (then
            (return (i32.const 1))))
        (if (i32.ge_u (local.get $attempts) (i32.const 1000))
          (then
            (return (i32.const 0))))
        (drop (memory.atomic.wait32
          (i32.const 4)
          (local.get $cur)
          (i64.const 1000000)))
        (local.set $attempts (i32.add (local.get $attempts) (i32.const 1)))
        (br $again))
      (i32.const 0))

    (func $thread-start (type $start-func-ty)
      (i32.atomic.store (i32.const 0) (call $thread-index))
      (i32.atomic.store (i32.const 4) (i32.const 1))
      (drop (memory.atomic.notify (i32.const 4) (i32.const 1))))
    (export "thread-start" (func $thread-start))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "run") (type $run-ty)
      (local $thread i32)
      (i32.atomic.store (i32.const 0) (i32.const -1))
      (i32.atomic.store (i32.const 4) (i32.const 0))
      (local.set $thread
        (call $thread-spawn-indirect (i32.const 0) (i32.const 0)))
      (if (i32.eqz (call $wait-done))
        (then
          (return (i32.const 0))))
      (i32.eq (local.get $thread) (i32.atomic.load (i32.const 0)))))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))
  (alias core export $libc "__indirect_function_table" (core table $indirect-function-table))

  (core func $thread-spawn-indirect
    (canon thread.spawn-indirect $start-func-ty (table $indirect-function-table)))
  (core func $thread-index (canon thread.index))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-indirect" (func $thread-spawn-indirect))
        (export "thread.index" (func $thread-index))))
      (with "libc" (instance
        (export "mem" (memory $mem))
        (export "__indirect_function_table" (table $indirect-function-table))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 1))
