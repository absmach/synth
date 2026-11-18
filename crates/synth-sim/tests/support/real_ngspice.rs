// SPDX-License-Identifier: Apache-2.0

//! The one lookup used by tests that need the real engine: `NGSPICE`, then
//! `PATH`, then `/usr/bin/ngspice`. The tests add that last fallback; the tool
//! uses only `NGSPICE`, then `PATH`, so a failing lookup names every place the
//! tests searched.

use std::path::{Path, PathBuf};

pub const SKIP_MESSAGE: &str = "skipping: ngspice not found (searched NGSPICE, PATH and \
                                /usr/bin/ngspice; sudo apt install ngspice)";

pub fn real_ngspice() -> Option<PathBuf> {
    real_ngspice_from(std::env::var_os("NGSPICE"), std::env::var_os("PATH"))
}

/// [`real_ngspice`] for given `NGSPICE` and `PATH` values.
pub fn real_ngspice_from(
    ngspice: Option<std::ffi::OsString>,
    path: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    if let Some(configured) = ngspice {
        return Some(PathBuf::from(configured));
    }
    if let Some(path) = path
        && let Some(found) = std::env::split_paths(&path)
            .map(|dir| dir.join("ngspice"))
            .find(|candidate| candidate.is_file())
    {
        return Some(found);
    }
    let system = Path::new("/usr/bin/ngspice");
    system.is_file().then(|| system.to_path_buf())
}
