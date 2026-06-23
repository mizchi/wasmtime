;;! threads = true
;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

;; Fork-local `canon thread.spawn-ref` preemptive smoke probe.
;;
;; This is the direct-function-reference variant of
;; `thread-spawn-indirect-preemptive-smoke.wast`: the child writes to shared
;; memory and notifies the parent. It only returns 1 when the unsafe sibling
;; Store OS-thread backend runs the child while the parent is blocked.

(component
  (core module $libc
    (memory (export "mem") 1 1 shared))

  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-ref-ty
      (shared (func (param (ref null $start-func-ty) i32) (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-ref"
      (func $thread-spawn-ref (type $spawn-ref-ty)))
    (import "libc" "mem" (memory $mem 1 1 shared))

    (func $thread-start (type $start-func-ty)
      (i32.atomic.store (i32.const 0) (local.get 0))
      (drop (memory.atomic.notify (i32.const 0) (i32.const 1))))
    (export "thread-start" (func $thread-start))

    (func (export "run") (type $run-ty)
      (drop (call $thread-spawn-ref
        (ref.func $thread-start)
        (i32.const 1)))
      (drop (memory.atomic.wait32
        (i32.const 0)
        (i32.const 0)
        (i64.const 10000000)))
      (i32.atomic.load (i32.const 0))))

  (core instance $libc (instantiate $libc))
  (core type $start-func-ty (shared (func (param i32))))
  (alias core export $libc "mem" (core memory $mem))

  (core func $thread-spawn-ref
    (canon thread.spawn-ref $start-func-ty))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-ref" (func $thread-spawn-ref))))
      (with "libc" (instance
        (export "mem" (memory $mem))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 1))
