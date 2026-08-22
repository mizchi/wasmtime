use std::fs;

const SHARED_IMMEDIATE_TOOLING_CRATES: &[&str] =
    &["wasmparser", "wast", "wasm-encoder", "wasmprinter"];

struct DiagnosticSlotAbi {
    stride: i32,
    state_offset: i32,
    terminal_code_offset: i32,
    payload_offset: i32,
    input_offset: i32,
    cancel_offset: i32,
    mode_offset: i32,
    worker_func_offset: i32,
}

const DIAGNOSTIC_SLOT_ABI: DiagnosticSlotAbi = DiagnosticSlotAbi {
    stride: 32,
    state_offset: 0,
    terminal_code_offset: 4,
    payload_offset: 8,
    input_offset: 16,
    cancel_offset: 20,
    mode_offset: 24,
    worker_func_offset: 28,
};

const DIAGNOSTIC_SLOT_WAST_FIXTURES: &[&str] = &[
    "tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi.wast",
    "tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-serial.wast",
    "tests/misc_testsuite/component-model-threading/thread-spawn-indirect-os-trampoline-vibe-abi-speedup-parallel.wast",
];

#[test]
fn component_thread_shared_immediate_tooling_scope_stays_documented() {
    let docs_path = "docs/plans/thread-impl.md";
    let docs = fs::read_to_string(docs_path).unwrap();
    for name in SHARED_IMMEDIATE_TOOLING_CRATES {
        assert_contains(docs_path, &docs, &format!("`{name}`"));
    }

    let cargo = fs::read_to_string("Cargo.toml").unwrap();
    assert_contains(
        "Cargo.toml",
        &cargo,
        "wasmparser = { path = \"crates/forks/wasmparser\" }",
    );
    for name in ["wast =", "wasm-encoder =", "wasmprinter ="] {
        assert_contains("Cargo.toml", &cargo, name);
    }
}

#[test]
fn fork_diagnostic_slot_wast_fixtures_stay_in_sync() {
    for path in DIAGNOSTIC_SLOT_WAST_FIXTURES {
        let source = fs::read_to_string(path).unwrap();

        assert_contains(path, &source, ";; Slot layout");
        assert_contains(path, &source, "+0: state");
        assert_contains(path, &source, "+4: terminal code");
        assert_contains(path, &source, "+8: payload");
        assert_contains(path, &source, "+16: input");
        assert_contains(path, &source, "+20: cancel");
        assert_contains(path, &source, "+24: mode");

        for slot in [
            0,
            DIAGNOSTIC_SLOT_ABI.stride,
            DIAGNOSTIC_SLOT_ABI.stride * 2,
            DIAGNOSTIC_SLOT_ABI.stride * 3,
        ] {
            assert_contains(path, &source, &format!("i32.const {slot}"));
        }

        for offset in [
            DIAGNOSTIC_SLOT_ABI.state_offset,
            DIAGNOSTIC_SLOT_ABI.terminal_code_offset,
            DIAGNOSTIC_SLOT_ABI.payload_offset,
            DIAGNOSTIC_SLOT_ABI.input_offset,
            DIAGNOSTIC_SLOT_ABI.cancel_offset,
            DIAGNOSTIC_SLOT_ABI.mode_offset,
        ] {
            assert_contains(path, &source, &format!("i32.const {offset}"));
        }
    }
}

#[test]
fn fork_diagnostic_slot_docs_stay_in_sync() {
    let docs = [
        "docs/experimental-shared-everything-conformance.md",
        "docs/experimental-component-thread-speedup.md",
        "docs/plans/thread-impl.md",
    ];

    for path in docs {
        let source = fs::read_to_string(path).unwrap();

        assert_contains(path, &source, "| Slot stride | `32` bytes |");
        assert_contains(
            path,
            &source,
            &format!("| `state` | `{}` |", DIAGNOSTIC_SLOT_ABI.state_offset),
        );
        assert_contains(
            path,
            &source,
            &format!(
                "| `terminal_code` | `{}` |",
                DIAGNOSTIC_SLOT_ABI.terminal_code_offset
            ),
        );
        assert_contains(
            path,
            &source,
            &format!("| `payload` | `{}` |", DIAGNOSTIC_SLOT_ABI.payload_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `input` | `{}` |", DIAGNOSTIC_SLOT_ABI.input_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `cancel` | `{}` |", DIAGNOSTIC_SLOT_ABI.cancel_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `mode` | `{}` |", DIAGNOSTIC_SLOT_ABI.mode_offset),
        );
        assert_contains(
            path,
            &source,
            &format!(
                "| `worker_func` | `{}` |",
                DIAGNOSTIC_SLOT_ABI.worker_func_offset
            ),
        );
    }
}

#[test]
fn current_vibe_backend_contract_rejects_the_retired_guest_abi() {
    let path = "docs/experimental-vibe-thread-contract.md";
    let source = fs::read_to_string(path).unwrap();

    for contract in [
        "`Threads::*` is not a Vibe API",
        "`TaskGroup`",
        "`Send`",
        "independent `Store` and `Instance`",
        "must not encode Vibe task, channel, join, or cancellation semantics",
        "feature-detected, opt-in experiment",
    ] {
        assert_contains(path, &source, contract);
    }
}

#[test]
fn unsafe_component_threads_require_an_off_by_default_compile_feature() {
    let root_path = "Cargo.toml";
    let root = fs::read_to_string(root_path).unwrap();
    assert_contains(
        root_path,
        &root,
        "experimental-component-threads = [\"wasmtime/experimental-component-threads\"]",
    );

    let crate_path = "crates/wasmtime/Cargo.toml";
    let wasmtime = fs::read_to_string(crate_path).unwrap();
    assert_contains(
        crate_path,
        &wasmtime,
        "experimental-component-threads = [\"component-model-async\", \"threads\"]",
    );

    let default_features = root
        .split("default = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap();
    assert!(!default_features.contains("experimental-component-threads"));

    let component_module_path = "crates/wasmtime/src/runtime/component/mod.rs";
    let component_module = fs::read_to_string(component_module_path).unwrap();
    assert_contains(
        component_module_path,
        &component_module,
        "#[cfg(feature = \"experimental-component-threads\")]\nmod threading;",
    );
}

fn assert_contains(path: &str, source: &str, needle: &str) {
    assert!(source.contains(needle), "{path} should contain {needle:?}");
}
