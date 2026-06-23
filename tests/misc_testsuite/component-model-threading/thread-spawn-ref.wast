;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

(component
  (core module $m
    (type $start-func-ty (shared (func (param i32))))
    (type $spawn-ref-ty
      (shared (func (param (ref null $start-func-ty) i32) (result i32))))
    (type $yield-ty (func (result i32)))
    (type $run-ty (func (result i32)))
    (import "" "thread.spawn-ref"
      (func $thread-spawn-ref (type $spawn-ref-ty)))
    (import "" "thread.yield" (func $thread-yield (type $yield-ty)))

    (global $g (shared mut i32) (i32.const 0))

    (func $thread-start (type $start-func-ty)
      (global.set $g (local.get 0)))
    (export "thread-start" (func $thread-start))

    (func (export "run") (type $run-ty)
      (drop (call $thread-spawn-ref (ref.func $thread-start) (i32.const 77)))
      (drop (call $thread-yield))
      (global.get $g)))

  (core type $start-func-ty (shared (func (param i32))))

  (core func $thread-spawn-ref
    (canon thread.spawn-ref $start-func-ty))
  (core func $thread-yield (canon thread.yield))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.spawn-ref" (func $thread-spawn-ref))
        (export "thread.yield" (func $thread-yield))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 77))
