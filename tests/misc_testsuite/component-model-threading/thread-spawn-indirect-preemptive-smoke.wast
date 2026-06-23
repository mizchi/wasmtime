;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; This is a fork-local Red probe for true preemptive Component Model threads.
;;
;; The spawned thread writes to shared memory and notifies the main thread. A
;; cooperative implementation will not run the spawned thread while the main
;; thread is blocked in `memory.atomic.wait32`, so the wait times out and the
;; final load returns 0. A real OS-thread implementation can run the spawned
;; thread concurrently and returns 1.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared)
    (table (export "__indirect_function_table") shared 1 1 (ref null (shared func))))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-indirect-ty (shared (func (param i32 i32) (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-indirect"
      (func $thread-spawn-indirect (type $spawn-indirect-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))
    (import "libc" "__indirect_function_table"
      (table $indirect-function-table shared 1 1 (ref null (shared func))))

    (func $thread-start (type $start-func-ty)
      (i32.atomic.store (i32.const 0) (local.get 0))
      (drop (memory.atomic.notify (i32.const 0) (i32.const 1))))
    (export "thread-start" (func $thread-start))

    (elem (table $indirect-function-table) (i32.const 0)
      (ref null (shared func)) (ref.func $thread-start))

    (func (export "run") (type $run-ty)
      (drop (call $thread-spawn-indirect (i32.const 0) (i32.const 1)))
      (drop (memory.atomic.wait32
        (i32.const 0)
        (i32.const 0)
        (i64.const 10000000)))
      (i32.atomic.load (i32.const 0))))

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
