// SPDX-License-Identifier: Apache-2.0

//! The vetted reference-subcircuit library (`registry/modules/`).
//!
//! A reference module is a checked `.synth` `module` copied from a data
//! sheet or application note, with its provenance, the parameters it lets a
//! design vary, the values its fixture validates, and a fixture board. Each
//! lives in its own directory:
//!
//! ```text
//! registry/modules/<id>/
//!   module.synth   the `module "Name" (…) { … }` declaration
//!   module.toml    id, version, provenance, ports, params, notes
//!   fixture.synth  a board that instantiates it and must validate clean
//! ```
//!
//! A parameter marked `fixed` is part of the reference. Overriding it at a
//! `use` needs a `deviation "reason"`, and validate lists every deviation in
//! one place (`E-SYNTH-MODULE-009`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where a reference module was copied from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleProvenance {
    /// The document: data sheet, application note, specification.
    pub source: String,
    /// The section, table or figure in it.
    pub section: String,
    /// The reference design the circuit copies.
    pub reference_design: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// One port of a reference module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModulePort {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub description: String,
}

/// One parameter of a reference module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleParam {
    pub name: String,
    pub default: String,
    /// Part of the reference: an override needs a deviation.
    #[serde(default)]
    pub fixed: bool,
    /// The range a design may vary it over, for a parameter that is not fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(default)]
    pub description: String,
}

/// A set of values a fixture validates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedValues {
    /// The fixture file that checks them.
    pub case: String,
    pub values: std::collections::BTreeMap<String, String>,
}

/// A reference module, read from its directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceModule {
    pub id: String,
    /// The `module "Name"` the source declares.
    pub module: String,
    pub version: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub provenance: ModuleProvenance,
    #[serde(default)]
    pub ports: Vec<ModulePort>,
    #[serde(default)]
    pub params: Vec<ModuleParam>,
    #[serde(default)]
    pub validated: Vec<ValidatedValues>,
    /// The `module.synth` text.
    #[serde(skip_deserializing, default)]
    pub source: String,
    /// The `fixture.synth` text.
    #[serde(skip_deserializing, default)]
    pub fixture: String,
}

/// Why a module directory could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ModuleLoadError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

/// The library shipped in this repository: `registry/modules/`.
#[must_use]
pub fn default_modules_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/modules")
}

/// Read every module under `dir`, sorted by id.
///
/// # Errors
///
/// Returns the first directory that is missing a file or whose metadata
/// does not parse or does not match its source.
pub fn load_modules(dir: &Path) -> Result<Vec<ReferenceModule>, ModuleLoadError> {
    let entries = std::fs::read_dir(dir).map_err(|source| ModuleLoadError::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.iter().map(|d| load_module(d)).collect()
}

/// Read one module directory.
///
/// # Errors
///
/// See [`load_modules`].
pub fn load_module(dir: &Path) -> Result<ReferenceModule, ModuleLoadError> {
    let read = |name: &str| {
        let path = dir.join(name);
        std::fs::read_to_string(&path).map_err(|source| ModuleLoadError::Read { path, source })
    };
    let meta_path = dir.join("module.toml");
    let mut module: ReferenceModule =
        toml::from_str(&read("module.toml")?).map_err(|e| ModuleLoadError::Invalid {
            path: meta_path.clone(),
            message: e.to_string(),
        })?;
    module.source = read("module.synth")?;
    module.fixture = read("fixture.synth")?;
    let invalid = |message: String| ModuleLoadError::Invalid {
        path: meta_path.clone(),
        message,
    };
    if dir.file_name().and_then(|n| n.to_str()) != Some(module.id.as_str()) {
        return Err(invalid(format!(
            "id `{}` does not match its directory",
            module.id
        )));
    }
    if !module
        .source
        .contains(&format!("module \"{}\"", module.module))
    {
        return Err(invalid(format!(
            "module.synth does not declare `module \"{}\"`",
            module.module
        )));
    }
    for param in &module.params {
        let fixed_in_source = source_param_line(&module.source, &param.name)
            .ok_or_else(|| invalid(format!("param `{}` is not in module.synth", param.name)))?
            .split_whitespace()
            .any(|w| w == "fixed");
        if fixed_in_source != param.fixed {
            return Err(invalid(format!(
                "param `{}` is fixed in one of module.toml and module.synth but not the other",
                param.name
            )));
        }
    }
    if module.provenance.source.trim().is_empty() || module.provenance.section.trim().is_empty() {
        return Err(invalid(
            "provenance needs a source and a section".to_string(),
        ));
    }
    Ok(module)
}

fn source_param_line<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    source.lines().find(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("param")
            && words
                .next()
                .is_some_and(|w| w.trim_end_matches(':') == name)
    })
}

/// Modules whose id, title, description or tags contain every word of
/// `query`, case-insensitively. An empty query returns them all.
#[must_use]
pub fn search<'a>(modules: &'a [ReferenceModule], query: &str) -> Vec<&'a ReferenceModule> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    modules
        .iter()
        .filter(|m| {
            let hay = format!(
                "{} {} {} {} {}",
                m.id,
                m.module,
                m.title,
                m.description,
                m.tags.join(" ")
            )
            .to_ascii_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

/// A request to place a reference module in a design.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instantiation {
    /// The instance label: `use "…" as <label>`.
    pub label: String,
    /// `(port, net name)` bindings.
    pub bindings: Vec<(String, String)>,
    /// `(param, value)` overrides, such as `("c_in", "22uF")`.
    #[serde(default)]
    pub params: Vec<(String, String)>,
    /// `(param, reason)` for each override of a fixed parameter, or
    /// `("", reason)` for an instance-level deviation.
    #[serde(default)]
    pub deviations: Vec<(String, String)>,
}

/// Why an instantiation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstantiateError {
    #[error("the design has no `board \"…\" {{` to add the module to")]
    NoBoard,
    #[error("module `{module}` has no port `{port}`")]
    UnknownPort { module: String, port: String },
    #[error("port `{port}` of `{module}` is not bound")]
    UnboundPort { module: String, port: String },
    #[error("module `{module}` has no parameter `{param}`")]
    UnknownParam { module: String, param: String },
    #[error("`{param}` is fixed by the reference; overriding it needs a deviation reason")]
    FixedWithoutDeviation { param: String },
    #[error("instance label `{0}` is already used in the design")]
    DuplicateLabel(String),
    #[error(
        "`{0}` is not a valid instance label (letters, digits and `_`, not starting with a digit)"
    )]
    BadLabel(String),
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[allow(clippy::too_many_lines)]
/// The design text with `module` placed in it: the module declaration (once,
/// stamped with its id and version) at the top of the board, and a `use`
/// block at the end.
///
/// # Errors
///
/// Refuses an unknown or unbound port, an unknown parameter, a fixed
/// parameter overridden without a deviation, a duplicate label, and a design
/// with no board.
pub fn instantiate(
    design: &str,
    module: &ReferenceModule,
    request: &Instantiation,
) -> Result<String, InstantiateError> {
    let label = &request.label;
    let valid_label = label
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_label {
        return Err(InstantiateError::BadLabel(label.clone()));
    }
    if design
        .lines()
        .any(|l| l.trim_start().starts_with("use ") && l.contains(&format!(" as {label}")))
    {
        return Err(InstantiateError::DuplicateLabel(label.clone()));
    }
    for (port, _) in &request.bindings {
        let head = port.split('.').next().unwrap_or(port);
        if !module.ports.iter().any(|p| p.name == head) {
            return Err(InstantiateError::UnknownPort {
                module: module.module.clone(),
                port: port.clone(),
            });
        }
    }
    for p in &module.ports {
        if !request
            .bindings
            .iter()
            .any(|(port, _)| port.split('.').next() == Some(p.name.as_str()))
        {
            return Err(InstantiateError::UnboundPort {
                module: module.module.clone(),
                port: p.name.clone(),
            });
        }
    }
    let mut overrides = Vec::new();
    for (name, value) in &request.params {
        let Some(param) = module.params.iter().find(|p| &p.name == name) else {
            return Err(InstantiateError::UnknownParam {
                module: module.module.clone(),
                param: name.clone(),
            });
        };
        let reason = request
            .deviations
            .iter()
            .find(|(p, _)| p == name)
            .map(|(_, r)| r);
        if param.fixed && reason.is_none() {
            return Err(InstantiateError::FixedWithoutDeviation {
                param: name.clone(),
            });
        }
        overrides.push(match reason {
            Some(reason) => format!("{name} = {value} deviation {}", quote(reason)),
            None => format!("{name} = {value}"),
        });
    }
    overrides.extend(
        request
            .deviations
            .iter()
            .filter(|(p, _)| p.is_empty())
            .map(|(_, reason)| format!("deviation {}", quote(reason))),
    );
    // The `board "…" {` that opens the design: the first line whose code,
    // not a comment, starts with `board`.
    let mut offset = 0;
    let mut board_at = None;
    for line in design.split_inclusive('\n') {
        if line.trim_start().starts_with("board ") || line.trim_start().starts_with("board\"") {
            board_at = Some(offset);
            break;
        }
        offset += line.len();
    }
    let Some(open) = board_at.and_then(|b| design[b..].find('{').map(|o| b + o + 1)) else {
        return Err(InstantiateError::NoBoard);
    };
    let Some(close) = design.rfind('}') else {
        return Err(InstantiateError::NoBoard);
    };
    let mut out = String::with_capacity(design.len() + module.source.len() + 256);
    out.push_str(&design[..open]);
    let declared = design.contains(&format!("module \"{}\"", module.module));
    if !declared {
        let _ = writeln!(
            out,
            "\n  // Reference module {} {} from registry/modules/{}.",
            module.id, module.version, module.id
        );
        for line in module.source.lines() {
            if line.trim().is_empty() {
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.push_str(&design[open..close]);
    let body = out.trim_end().len();
    out.truncate(body);
    let _ = write!(out, "\n  use {} as {label}", quote(&module.module));
    if !overrides.is_empty() {
        let _ = write!(out, " ({})", overrides.join(", "));
    }
    out.push_str(" {\n");
    for (port, net) in &request.bindings {
        let _ = writeln!(out, "    {port} -> {}", quote(net));
    }
    out.push_str("  }\n");
    out.push_str(&design[close..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Vec<ReferenceModule> {
        load_modules(&default_modules_dir()).expect("the shipped library loads")
    }

    #[test]
    fn the_library_has_at_least_five_modules_with_provenance() {
        let modules = library();
        assert!(modules.len() >= 5, "{}", modules.len());
        for m in &modules {
            assert!(!m.provenance.reference_design.is_empty(), "{}", m.id);
            assert!(!m.validated.is_empty(), "{} validates no values", m.id);
            assert!(
                m.fixture.contains(&format!("use \"{}\"", m.module)),
                "{}",
                m.id
            );
        }
    }

    #[test]
    fn search_matches_every_word() {
        let modules = library();
        let hits: Vec<&str> = search(&modules, "buck 5v")
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(hits, vec!["buck_lmr33640_5v"]);
        assert_eq!(search(&modules, "").len(), modules.len());
    }

    #[test]
    fn instantiation_adds_the_declaration_once_and_a_use_block() {
        let modules = library();
        let ldo = modules
            .iter()
            .find(|m| m.id == "ldo_ams1117_3v3")
            .expect("ldo");
        let design = "board \"b\" {\n  layers 2\n}\n";
        let request = Instantiation {
            label: "LDO1".to_string(),
            bindings: vec![
                ("vin".to_string(), "VIN".to_string()),
                ("vout".to_string(), "+3V3".to_string()),
                ("gnd".to_string(), "GND".to_string()),
            ],
            ..Instantiation::default()
        };
        let once = instantiate(design, ldo, &request).expect("instantiates");
        let commented = format!("// a board for the bench\n{design}");
        let with_comment = instantiate(&commented, ldo, &request).expect("instantiates");
        assert!(
            with_comment.starts_with("// a board for the bench\nboard \"b\" {\n"),
            "{with_comment}"
        );
        assert_eq!(once.matches("module \"LdoAms1117\"").count(), 1, "{once}");
        assert!(once.contains("use \"LdoAms1117\" as LDO1 {"), "{once}");
        let second = Instantiation {
            label: "LDO2".to_string(),
            ..request.clone()
        };
        let twice = instantiate(&once, ldo, &second).expect("instantiates again");
        assert_eq!(twice.matches("module \"LdoAms1117\"").count(), 1, "{twice}");
        assert!(matches!(
            instantiate(&twice, ldo, &request),
            Err(InstantiateError::DuplicateLabel(_))
        ));
    }

    #[test]
    fn a_fixed_override_needs_a_reason() {
        let modules = library();
        let ldo = modules
            .iter()
            .find(|m| m.id == "ldo_ams1117_3v3")
            .expect("ldo");
        let mut request = Instantiation {
            label: "LDO1".to_string(),
            bindings: vec![
                ("vin".to_string(), "VIN".to_string()),
                ("vout".to_string(), "+3V3".to_string()),
                ("gnd".to_string(), "GND".to_string()),
            ],
            params: vec![("c_out".to_string(), "47uF".to_string())],
            ..Instantiation::default()
        };
        assert_eq!(
            instantiate("board \"b\" {\n}\n", ldo, &request),
            Err(InstantiateError::FixedWithoutDeviation {
                param: "c_out".to_string()
            })
        );
        request.deviations = vec![("c_out".to_string(), "low-ESR polymer part".to_string())];
        let out = instantiate("board \"b\" {\n}\n", ldo, &request).expect("instantiates");
        assert!(
            out.contains("(c_out = 47uF deviation \"low-ESR polymer part\")"),
            "{out}"
        );
    }
}
