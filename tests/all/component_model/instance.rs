use wasmtime::Result;
use wasmtime::component::*;
use wasmtime::{Config, Engine, Module, Store};

#[test]
#[cfg_attr(miri, ignore)]
fn instance_exports() -> Result<()> {
    let engine = super::engine();
    let component = r#"
        (component
            (import "a" (instance $i))
            (import "b" (instance $i2 (export "m" (core module))))

            (alias export $i2 "m" (core module $m))

            (component $c
                (component $c
                    (export "m" (core module $m))
                )
                (instance $c (instantiate $c))
                (export "i" (instance $c))
            )
            (instance $c (instantiate $c))
            (export "i" (instance $c))
            (export "r" (instance $i))
            (export "r2" (instance $i2))
        )
    "#;
    let component = Component::new(&engine, component)?;
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    linker.instance("a")?;
    linker
        .instance("b")?
        .module("m", &Module::new(&engine, "(module)")?)?;
    let instance = linker.instantiate(&mut store, &component)?;

    assert!(
        instance
            .get_export(&mut store, None, "not an instance")
            .is_none()
    );
    let i = instance.get_export_index(&mut store, None, "r").unwrap();
    assert!(instance.get_export(&mut store, Some(&i), "x").is_none());
    instance.get_export(&mut store, None, "i").unwrap();
    let i2 = instance.get_export_index(&mut store, None, "r2").unwrap();
    let m = instance
        .get_export_index(&mut store, Some(&i2), "m")
        .unwrap();
    assert!(instance.get_func(&mut store, &m).is_none());
    assert!(instance.get_module(&mut store, &m).is_some());

    let i = instance.get_export_index(&mut store, None, "i").unwrap();
    let i = instance
        .get_export_index(&mut store, Some(&i), "i")
        .unwrap();
    let m = instance
        .get_export_index(&mut store, Some(&i), "m")
        .unwrap();
    instance.get_module(&mut store, &m).unwrap();

    Ok(())
}

#[test]
fn export_old_get_new() -> Result<()> {
    let engine = super::engine();
    let component = r#"
        (component
            (core module $m)
            (export "a:b/m@1.0.0" (core module $m))

            (instance $i (export "m" (core module $m)))
            (export "a:b/i@1.0.0" (instance $i))
        )
    "#;

    let component = Component::new(&engine, component)?;
    component.get_export(None, "a:b/m@1.0.1").unwrap();
    let i = component.get_export_index(None, "a:b/i@1.0.1").unwrap();
    component.get_export(Some(&i), "m").unwrap();

    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &component)?;

    instance.get_module(&mut store, "a:b/m@1.0.1").unwrap();
    instance
        .get_export(&mut store, None, "a:b/m@1.0.1")
        .unwrap();

    let i = instance
        .get_export_index(&mut store, None, "a:b/i@1.0.1")
        .unwrap();
    instance.get_export(&mut store, Some(&i), "m").unwrap();

    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn instance_pre_sibling_does_not_share_defined_shared_memory() -> Result<()> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.wasm_threads(true);
    config.shared_memory(true);
    let engine = Engine::new(&config)?;

    let component = Component::new(
        &engine,
        r#"
        (component
            (core module $m
                (memory (export "memory") 1 1 shared)
                (func (export "write") (param i32)
                    i32.const 0
                    local.get 0
                    i32.atomic.store)
                (func (export "read") (result i32)
                    i32.const 0
                    i32.atomic.load))
            (core instance $i (instantiate $m))

            (func (export "write") (param "v" u32)
                (canon lift (core func $i "write")))
            (func (export "read") (result u32)
                (canon lift (core func $i "read")))
        )
        "#,
    )?;

    let linker = Linker::new(&engine);
    let mut parent_store = Store::new(&engine, ());
    let parent = linker.instantiate(&mut parent_store, &component)?;
    let instance_pre = parent.instance_pre(&parent_store);

    let mut sibling_store = Store::new(&engine, ());
    let sibling = instance_pre.instantiate(&mut sibling_store)?;

    parent
        .get_typed_func::<(u32,), ()>(&mut parent_store, "write")?
        .call(&mut parent_store, (7,))?;

    let (sibling_value,) = sibling
        .get_typed_func::<(), (u32,)>(&mut sibling_store, "read")?
        .call(&mut sibling_store, ())?;
    assert_eq!(sibling_value, 0);

    let (parent_value,) = parent
        .get_typed_func::<(), (u32,)>(&mut parent_store, "read")?
        .call(&mut parent_store, ())?;
    assert_eq!(parent_value, 7);

    Ok(())
}

#[test]
fn export_new_get_old() -> Result<()> {
    let engine = super::engine();
    let component = r#"
        (component
            (core module $m)
            (export "a:b/m@1.0.1" (core module $m))

            (instance $i (export "m" (core module $m)))
            (export "a:b/i@1.0.1" (instance $i))
        )
    "#;

    let component = Component::new(&engine, component)?;
    component.get_export(None, "a:b/m@1.0.0").unwrap();
    let i = component.get_export_index(None, "a:b/i@1.0.0").unwrap();
    component.get_export(Some(&i), "m").unwrap();

    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &component)?;

    instance.get_module(&mut store, "a:b/m@1.0.0").unwrap();
    instance
        .get_export(&mut store, None, "a:b/m@1.0.0")
        .unwrap();

    let i = instance
        .get_export_index(&mut store, None, "a:b/i@1.0.0")
        .unwrap();
    instance.get_export(&mut store, Some(&i), "m").unwrap();

    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn export_missing_get_max() -> Result<()> {
    let engine = super::engine();
    let component = r#"
        (component
            (core module $m1)
            (core module $m2 (import "" "" (func)))
            (export "a:b/m@1.0.1" (core module $m1))
            (export "a:b/m@1.0.3" (core module $m2))
        )
    "#;

    fn assert_m2(module: &Module) {
        assert_eq!(module.imports().len(), 1);
    }
    fn assert_m1(module: &Module) {
        assert_eq!(module.imports().len(), 0);
    }

    let component = Component::new(&engine, component)?;
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine).instantiate(&mut store, &component)?;

    let tests = [
        ("a:b/m@1.0.0", assert_m2 as fn(&_)), // no exact, should pick max available
        ("a:b/m@1.0.1", assert_m1),           // exact hit
        ("a:b/m@1.0.2", assert_m2),           // no exact, should pick max available
        ("a:b/m@1.0.3", assert_m2),           // exact hit
        ("a:b/m@1.0.4", assert_m2),           // no exact, should pick max available
    ];

    for (name, test_fn) in tests {
        println!("test {name}");
        let m = component.get_export_index(None, name).unwrap();
        let m = instance.get_module(&mut store, &m).unwrap();
        test_fn(&m);

        let m = instance.get_module(&mut store, name).unwrap();
        test_fn(&m);

        let m = instance.get_export_index(&mut store, None, name).unwrap();
        let m = instance.get_module(&mut store, &m).unwrap();
        test_fn(&m);
    }

    Ok(())
}
