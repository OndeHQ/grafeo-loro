// TODO: L2 — define FuzzInput with Arbitrary
// TODO: L3 — random Loro op generator + Grafeo consistency invariants

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // TODO: L2 — decode data into FuzzInput
    // TODO: L3 — apply ops, check invariants
    let _ = data;
});
