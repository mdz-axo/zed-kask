//! Opt-in live check that VRAM auto-detection reads this machine's amdgpu
//! sysfs nodes — the 7700S dGPU (~8 GiB) must win largest-wins over the
//! 890M iGPU's BIOS carve-out (512 MiB), with the winning card path carried
//! as provenance. Absent `HKASK_P0_SYSFS` the test skips: machines without
//! an amdgpu dGPU are the probe's expected `None` case, not this test's.
//!
//! ```sh
//! HKASK_P0_SYSFS=1 \
//!   cargo test -p hkask-private-inference --test live_sysfs -- --ignored --nocapture
//! ```

use hkask_private_inference::local_leg::placement::{VramSource, probe_hardware};

#[test]
#[ignore]
fn detects_the_dgpu_vram_from_sysfs() {
    if std::env::var_os("HKASK_P0_SYSFS").is_none() {
        eprintln!("HKASK_P0_SYSFS unset; skipping the live sysfs VRAM probe");
        return;
    }
    let hw = probe_hardware().expect("live probe of this machine");
    let vram = hw.vram.expect("this machine has an amdgpu dGPU");
    let VramSource::Sysfs { card_path, bytes } = &vram else {
        panic!("live VRAM must be sysfs-detected, got: {vram:?}");
    };
    assert!(
        card_path
            .to_string_lossy()
            .starts_with("/sys/class/drm/card"),
        "detected card must be a drm card node, got: {card_path:?}"
    );
    assert!(
        *bytes > 7 * 1024 * 1024 * 1024,
        "the dGPU candidate must clear the iGPU carve-out, got: {bytes} bytes"
    );
    println!(
        "detected dGPU VRAM: {bytes} bytes at {}",
        card_path.display()
    );
}
