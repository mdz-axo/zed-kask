//! OMC consumer pin — the MovieLabs OMC bridge module must have a consumer
//! (`media_block::enrich_with_omc_and_provenance` references `omc::tool_to_omc`).
//! Per `.rules` "Advertised invariants need enforcement points", a module with
//! no consumer is dead surface regardless of its doc comments.
#[test]
fn omc_module_present_with_consumer() {
    // The source file must exist.
    let omc_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/hkask-media/src/omc.rs");
    assert!(
        omc_path.exists(),
        "kask/crates/hkask-media/src/omc.rs must exist — it is the MovieLabs OMC bridge"
    );
    // The domain lib root must declare the module.
    let lib_root = include_str!("../../../crates/hkask-media/src/hkask_media.rs");
    let declared = lib_root.lines().any(|line| {
        let t = line.trim();
        t == "pub mod omc;" || t == "mod omc;"
    });
    assert!(declared, "the omc module must be declared in the lib root");
    // The media_block module must reference the omc module — the consumer.
    // A line-level scan avoids matching this test's own text.
    let media_block = include_str!("../../../crates/hkask-media/src/media_block.rs");
    let has_consumer = media_block
        .lines()
        .any(|line| line.contains("crate::omc") || line.contains("use crate::omc"));
    assert!(
        has_consumer,
        "media_block.rs must reference the omc module — \
         a module without a consumer is dead surface"
    );
}
