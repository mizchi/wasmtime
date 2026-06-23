use std::fs;

const SHARED_IMMEDIATE_TOOLING_CRATES: &[&str] =
    &["wasmparser", "wast", "wasm-encoder", "wasmprinter"];

struct VibeSlotAbi {
    stride: i32,
    state_offset: i32,
    terminal_code_offset: i32,
    payload_offset: i32,
    input_offset: i32,
    cancel_offset: i32,
    mode_offset: i32,
    worker_func_offset: i32,
}

const VIBE_SLOT_ABI: VibeSlotAbi = VibeSlotAbi {
    stride: 32,
    state_offset: 0,
    terminal_code_offset: 4,
    payload_offset: 8,
    input_offset: 16,
    cancel_offset: 20,
    mode_offset: 24,
    worker_func_offset: 28,
};

const VIBE_ABI_WAST_FIXTURES: &[&str] = &[
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
fn vibe_runtime_slot_abi_wast_fixtures_stay_in_sync() {
    for path in VIBE_ABI_WAST_FIXTURES {
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
            VIBE_SLOT_ABI.stride,
            VIBE_SLOT_ABI.stride * 2,
            VIBE_SLOT_ABI.stride * 3,
        ] {
            assert_contains(path, &source, &format!("i32.const {slot}"));
        }

        for offset in [
            VIBE_SLOT_ABI.state_offset,
            VIBE_SLOT_ABI.terminal_code_offset,
            VIBE_SLOT_ABI.payload_offset,
            VIBE_SLOT_ABI.input_offset,
            VIBE_SLOT_ABI.cancel_offset,
            VIBE_SLOT_ABI.mode_offset,
        ] {
            assert_contains(path, &source, &format!("i32.const {offset}"));
        }
    }
}

#[test]
fn vibe_runtime_slot_abi_docs_stay_in_sync() {
    let docs = [
        "docs/experimental-vibe-thread-contract.md",
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
            &format!("| `state` | `{}` |", VIBE_SLOT_ABI.state_offset),
        );
        assert_contains(
            path,
            &source,
            &format!(
                "| `terminal_code` | `{}` |",
                VIBE_SLOT_ABI.terminal_code_offset
            ),
        );
        assert_contains(
            path,
            &source,
            &format!("| `payload` | `{}` |", VIBE_SLOT_ABI.payload_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `input` | `{}` |", VIBE_SLOT_ABI.input_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `cancel` | `{}` |", VIBE_SLOT_ABI.cancel_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `mode` | `{}` |", VIBE_SLOT_ABI.mode_offset),
        );
        assert_contains(
            path,
            &source,
            &format!("| `worker_func` | `{}` |", VIBE_SLOT_ABI.worker_func_offset),
        );
    }
}

fn assert_contains(path: &str, source: &str, needle: &str) {
    assert!(source.contains(needle), "{path} should contain {needle:?}");
}
