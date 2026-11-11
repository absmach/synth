// SPDX-License-Identifier: Apache-2.0

//! The real-ngspice lookup order shared by the integration tests.

#[path = "support/real_ngspice.rs"]
mod real_ngspice;

#[test]
fn the_lookup_prefers_ngspice_then_path_then_usr_bin() {
    let configured = std::env::temp_dir().join(format!("synth-ngspice-env-{}", std::process::id()));
    std::env::set_var("NGSPICE", &configured);
    assert_eq!(real_ngspice::real_ngspice(), Some(configured.clone()));
    std::env::remove_var("NGSPICE");

    let path_dir = std::env::temp_dir().join(format!("synth-ngspice-path-{}", std::process::id()));
    std::fs::create_dir_all(&path_dir).expect("fake PATH dir");
    let fake = path_dir.join("ngspice");
    std::fs::write(&fake, "").expect("fake ngspice");
    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", &path_dir);
    assert_eq!(real_ngspice::real_ngspice(), Some(fake));
    match old_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    let _ = std::fs::remove_dir_all(&path_dir);

    assert!(real_ngspice::SKIP_MESSAGE.contains("NGSPICE"));
    assert!(real_ngspice::SKIP_MESSAGE.contains("PATH"));
    assert!(real_ngspice::SKIP_MESSAGE.contains("/usr/bin/ngspice"));
}
