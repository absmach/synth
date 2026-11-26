// SPDX-License-Identifier: Apache-2.0

//! Versioned agent and skill assets installed by `synth init`.

use std::path::{Path, PathBuf};

use clap::ValueEnum;

const REVIEW_AGENT: &str = include_str!("../assets/agents/synth-design-review.md");
const BUILD_AGENT: &str = include_str!("../assets/agents/synth-design-build.md");
const REVIEW_SKILL: &str = include_str!("../assets/skills/synth-design-review/SKILL.md");
const BUILD_SKILL: &str = include_str!("../assets/skills/synth-design-build/SKILL.md");

/// Which client's asset directory `synth init` writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Client {
    Claude,
    Codex,
}

impl Client {
    fn root(self, home: &Path) -> PathBuf {
        match self {
            Self::Claude => home.join(".claude"),
            Self::Codex => home.join(".agents"),
        }
    }

    fn supports_agents(self) -> bool {
        matches!(self, Self::Claude)
    }
}

/// One file the installer owns.
struct Asset {
    relative: PathBuf,
    content: &'static str,
}

fn assets(client: Client) -> Vec<Asset> {
    let mut files = vec![
        Asset {
            relative: PathBuf::from("skills/synth-design-review/SKILL.md"),
            content: REVIEW_SKILL,
        },
        Asset {
            relative: PathBuf::from("skills/synth-design-build/SKILL.md"),
            content: BUILD_SKILL,
        },
    ];
    if client.supports_agents() {
        files.extend([
            Asset {
                relative: PathBuf::from("agents/synth-design-review.md"),
                content: REVIEW_AGENT,
            },
            Asset {
                relative: PathBuf::from("agents/synth-design-build.md"),
                content: BUILD_AGENT,
            },
        ]);
    }
    files
}

/// How an installed asset compares with the binary's copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Missing,
    Current,
    Modified,
}

/// Install every asset for `client`, writing only files whose content differs.
///
/// # Errors
///
/// A directory or file cannot be created or written.
pub fn install(client: Client, home: &Path) -> Result<Vec<PathBuf>, String> {
    let root = client.root(home);
    let mut written = Vec::new();
    for asset in assets(client) {
        let path = root.join(&asset.relative);
        if std::fs::read_to_string(&path).ok().as_deref() == Some(asset.content) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, asset.content)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Report each owned asset's state.
#[must_use]
pub fn status(client: Client, home: &Path) -> Vec<(PathBuf, State)> {
    let root = client.root(home);
    assets(client)
        .into_iter()
        .map(|asset| {
            let path = root.join(&asset.relative);
            let state = match std::fs::read_to_string(&path) {
                Err(_) => State::Missing,
                Ok(text) if text == asset.content => State::Current,
                Ok(_) => State::Modified,
            };
            (path, state)
        })
        .collect()
}

/// Remove every asset this binary installed for `client`.
///
/// # Errors
///
/// An existing file cannot be removed.
pub fn uninstall(client: Client, home: &Path) -> Result<Vec<PathBuf>, String> {
    let root = client.root(home);
    let mut removed = Vec::new();
    for asset in assets(client) {
        let path = root.join(&asset.relative);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("could not remove {}: {e}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "synth-agent-assets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        path
    }

    #[test]
    fn install_is_idempotent_and_status_detects_drift() {
        let dir = scratch();
        let first = install(Client::Claude, &dir).expect("install");
        assert!(!first.is_empty());
        let second = install(Client::Claude, &dir).expect("reinstall");
        assert!(second.is_empty(), "a second install is a no-op");
        assert!(
            status(Client::Claude, &dir)
                .iter()
                .all(|(_, state)| *state == State::Current)
        );

        let review = dir.join(".claude/agents/synth-design-review.md");
        std::fs::write(&review, "edited").expect("edit");
        assert!(
            status(Client::Claude, &dir)
                .iter()
                .any(|(path, state)| path == &review && *state == State::Modified)
        );
    }

    #[test]
    fn codex_installs_skills_without_claude_agents() {
        let dir = scratch();
        install(Client::Codex, &dir).expect("install");
        assert!(
            &dir.join(".agents/skills/synth-design-review/SKILL.md")
                .is_file()
        );
        assert!(!dir.join(".agents/agents").exists());
        uninstall(Client::Codex, &dir).expect("uninstall");
        assert!(
            !&dir
                .join(".agents/skills/synth-design-review/SKILL.md")
                .exists()
        );
    }
}
