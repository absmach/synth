// SPDX-License-Identifier: Apache-2.0

//! Where the router finds the external tools it runs.
//!
//! The environment is read once, when a request is built, into [`ToolPaths`].
//! A caller with explicit settings (a CLI flag, an MCP tool argument) writes
//! them into the request's `tools` rather than exporting them, and every
//! resolver reads only the request. That keeps two concurrent requests in one
//! process from seeing each other's settings, and lets a test describe the
//! whole tool environment without touching the process environment.

use std::ffi::OsString;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The tool locations a routing run resolves its engines from.
///
/// Every field is optional: an unset field falls back to the conventional
/// locations each resolver searches (the repository's `tools/` tree, `PATH`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPaths {
    /// FreeRouting JAR. Environment: `SYNTH_FREEROUTING_JAR`.
    pub freerouting_jar: Option<PathBuf>,
    /// Java runtime that runs FreeRouting. Environment: `SYNTH_FREEROUTING_JAVA`.
    pub freerouting_java: Option<PathBuf>,
    /// Interpreter that runs the FreeRouting helper script.
    /// Environment: `SYNTH_FREEROUTING_PYTHON`.
    pub freerouting_python: Option<PathBuf>,
    /// KiCadRoutingTools checkout. Environment: `KICAD_ROUTING_TOOLS_REPO`.
    pub krt_repo: Option<PathBuf>,
    /// Interpreter that runs KiCadRoutingTools. Environment: `SYNTH_KRT_PYTHON`.
    pub krt_python: Option<PathBuf>,
    /// Extra directory holding `freerouting/` and the bundled JRE, searched
    /// before the repository's own `tools/`. Environment: `SYNTH_TOOLS_DIR`.
    pub tools_dir: Option<PathBuf>,
    /// Data directory the setup script installs KiCadRoutingTools under:
    /// `$XDG_DATA_HOME`, else `$HOME/.local/share`.
    pub data_dir: Option<PathBuf>,
    /// `kicad-cli` used for zone fill and DRC. Environment: `KICAD_CLI`;
    /// unset means `kicad-cli` on `PATH`.
    pub kicad_cli: Option<PathBuf>,
}

impl ToolPaths {
    /// Read the tool locations from the process environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var_os(key))
    }

    /// Read the tool locations through `lookup`, which answers like
    /// [`std::env::var_os`]. An empty value counts as unset.
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let path = |key: &str| {
            lookup(key)
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty())
        };
        Self {
            freerouting_jar: path("SYNTH_FREEROUTING_JAR"),
            freerouting_java: path("SYNTH_FREEROUTING_JAVA"),
            freerouting_python: path("SYNTH_FREEROUTING_PYTHON"),
            krt_repo: path("KICAD_ROUTING_TOOLS_REPO"),
            krt_python: path("SYNTH_KRT_PYTHON"),
            tools_dir: path("SYNTH_TOOLS_DIR"),
            data_dir: path("XDG_DATA_HOME")
                .or_else(|| path("HOME").map(|home| home.join(".local").join("share"))),
            kicad_cli: path("KICAD_CLI"),
        }
    }

    /// The `kicad-cli` to run: the configured one, else `kicad-cli` on `PATH`.
    #[must_use]
    pub fn kicad_cli_binary(&self) -> String {
        self.kicad_cli
            .as_ref()
            .map_or_else(|| "kicad-cli".to_string(), |p| p.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn every_variable_maps_to_its_field() {
        let tools = ToolPaths::from_lookup(lookup(&[
            ("SYNTH_FREEROUTING_JAR", "/t/fr.jar"),
            ("SYNTH_FREEROUTING_JAVA", "/t/java"),
            ("SYNTH_FREEROUTING_PYTHON", "/t/py"),
            ("KICAD_ROUTING_TOOLS_REPO", "/t/krt"),
            ("SYNTH_KRT_PYTHON", "/t/krt-py"),
            ("SYNTH_TOOLS_DIR", "/t/tools"),
            ("XDG_DATA_HOME", "/t/data"),
            ("KICAD_CLI", "/t/kicad-cli"),
        ]));
        assert_eq!(tools.freerouting_jar, Some(PathBuf::from("/t/fr.jar")));
        assert_eq!(tools.freerouting_java, Some(PathBuf::from("/t/java")));
        assert_eq!(tools.freerouting_python, Some(PathBuf::from("/t/py")));
        assert_eq!(tools.krt_repo, Some(PathBuf::from("/t/krt")));
        assert_eq!(tools.krt_python, Some(PathBuf::from("/t/krt-py")));
        assert_eq!(tools.tools_dir, Some(PathBuf::from("/t/tools")));
        assert_eq!(tools.data_dir, Some(PathBuf::from("/t/data")));
        assert_eq!(tools.kicad_cli_binary(), "/t/kicad-cli");
    }

    #[test]
    fn an_empty_value_counts_as_unset() {
        let tools = ToolPaths::from_lookup(lookup(&[("SYNTH_FREEROUTING_JAR", "")]));
        assert_eq!(tools.freerouting_jar, None);
    }

    #[test]
    fn the_data_dir_falls_back_to_home() {
        let tools = ToolPaths::from_lookup(lookup(&[("HOME", "/home/u")]));
        assert_eq!(tools.data_dir, Some(PathBuf::from("/home/u/.local/share")));
    }

    #[test]
    fn nothing_set_means_nothing_configured() {
        let tools = ToolPaths::from_lookup(|_| None);
        assert_eq!(tools, ToolPaths::default());
        assert_eq!(tools.kicad_cli_binary(), "kicad-cli");
    }
}
