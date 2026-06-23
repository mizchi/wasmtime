;;! component_model_async = true
;;! component_model_threading = true
;;! gc = true
;;! function_references = true
;;! shared_everything_threads = true

(component
  (core func $thread-available-parallelism
    (canon thread.available_parallelism))

  (core module $m
    (type $thread-available-parallelism-ty (shared (func (result i32))))
    (type $run-ty (func (result i32)))
    (import "" "thread.available-parallelism"
      (func $thread-available-parallelism (type $thread-available-parallelism-ty)))

    (func (export "run") (type $run-ty)
      (call $thread-available-parallelism)))

  (core instance $i
    (instantiate $m
      (with "" (instance
        (export "thread.available-parallelism" (func $thread-available-parallelism))))))

  (func (export "run") async (result u32) (canon lift (core func $i "run"))))

(assert_return (invoke "run") (u32.const 1))
