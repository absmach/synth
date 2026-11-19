// SPDX-License-Identifier: Apache-2.0

//! The real-ngspice lookup order shared by the integration tests.

#[path = "support/real_ngspice.rs"]
mod real_ngspice;

#[test]
fn the_lookup_prefers_ngspice_then_path_then_usr_bin() {
    let configured = std::env::temp_dir().join(format!("synth-ngspice-env-{}", std::process::id()));
    let path_dir = std::env::temp_dir().join(format!("synth-ngspice-path-{}", std::process::id()));
    std::fs::create_dir_all(&path_dir).expect("fake PATH dir");
    let fake = path_dir.join("ngspice");
    std::fs::write(&fake, "").expect("fake ngspice");

    assert_eq!(
        real_ngspice::real_ngspice_from(
            Some(configured.clone().into_os_string()),
            Some(path_dir.clone().into_os_string())
        ),
        Some(configured),
        "NGSPICE wins over PATH"
    );
    assert_eq!(
        real_ngspice::real_ngspice_from(None, Some(path_dir.clone().into_os_string())),
        Some(fake),
        "PATH is searched when NGSPICE is unset"
    );
    let _ = std::fs::remove_dir_all(&path_dir);

    // The wrapper the tests call reads this process's own environment.
    assert_eq!(
        real_ngspice::real_ngspice(),
        real_ngspice::real_ngspice_from(std::env::var_os("NGSPICE"), std::env::var_os("PATH"))
    );

    assert!(real_ngspice::SKIP_MESSAGE.contains("NGSPICE"));
    assert!(real_ngspice::SKIP_MESSAGE.contains("PATH"));
    assert!(real_ngspice::SKIP_MESSAGE.contains("/usr/bin/ngspice"));
}
