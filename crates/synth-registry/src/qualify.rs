// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::part::{Part, PartId};
use crate::registry::Registry;

pub const SCHEMA_VERSION: &str = "synth.registry.qualify.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadSide {
    Front,
    Back,
    Both,
    NoCopper,
}

impl PadSide {
    pub fn has_copper(self) -> bool {
        !matches!(self, Self::NoCopper)
    }

    pub fn includes_front(self) -> bool {
        matches!(self, Self::Front | Self::Both)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadFacts {
    pub number: String,
    pub center_mm: (f64, f64),
    pub size_mm: (f64, f64),
    pub side: PadSide,
    pub is_npth: bool,
}

impl PadFacts {
    pub fn is_signal_pad(&self) -> bool {
        !self.is_npth && self.side.has_copper() && !self.number.trim().is_empty()
    }
}

pub trait FootprintFacts {
    fn pads(&self, lib_id: &str) -> Option<Vec<PadFacts>>;
}

pub trait SymbolFacts {
    fn pin_numbers(&self, lib_id: &str) -> Option<BTreeSet<String>>;
}

#[derive(Debug, Default)]
pub struct NoFacts;

impl FootprintFacts for NoFacts {
    fn pads(&self, _lib_id: &str) -> Option<Vec<PadFacts>> {
        None
    }
}

impl SymbolFacts for NoFacts {
    fn pin_numbers(&self, _lib_id: &str) -> Option<BTreeSet<String>> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Unknown,
    NotApplicable,
}

impl CheckStatus {
    pub fn is_pass(self) -> bool {
        matches!(self, Self::Pass | Self::NotApplicable)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unknown => "unknown",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingLevel {
    Review,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub level: FindingLevel,
    pub message: String,
    pub expected: String,
    pub found: String,
}

/// Blocking codes that say a part definition is simply wrong about the
/// physical package, rather than being a judgement call.
///
/// These are the only findings that refuse a fab export with no override, so
/// an allow-list rather than a deny-list: a new code has to be added here
/// deliberately instead of inheriting that power by default.
///
/// - `001` a declared pin is not a pad, or lands on a pad with no copper
/// - `002` the footprint has copper pads no pin declares
/// - `003` two pins claim one pad number
/// - `004` two pins share one logical name
/// - `005` a declared pin is not on the referenced KiCad symbol
/// - `011` a do-not-connect pin is marked required, so every design using the
///   part is forced to connect a pin the manufacturer says must float
/// - `012` copper pads sit on the back layer on a top-side part
/// - `013` a pin carries no pad number at all
///
/// [`NON_STRUCTURAL_BLOCKING`] carries the blocking codes deliberately left
/// out, with the reason. Between them the two lists must cover every blocking
/// code the engine emits; `every_blocking_code_is_classified` enforces that,
/// because the omission of `013` from an earlier version of this list was
/// invisible until a reviewer read it.
pub const STRUCTURAL_CODES: [&str; 8] = [
    "E-SYNTH-QUAL-001",
    "E-SYNTH-QUAL-002",
    "E-SYNTH-QUAL-003",
    "E-SYNTH-QUAL-004",
    "E-SYNTH-QUAL-005",
    "E-SYNTH-QUAL-011",
    "E-SYNTH-QUAL-012",
    "E-SYNTH-QUAL-013",
];

/// Blocking codes that are deliberately not structural, and why.
///
/// `006` and `007` judge `footprint_dimensions`, which is declared metadata the
/// exporter consults only when it has to synthesize a footprint. When a real
/// footprint resolves — the case for anything heading to fabrication — those
/// values touch no copper, so refusing a fab submission over them, under a
/// message about the pin map, blames the wrong thing.
///
/// `008` and `015` are provenance: nobody has reviewed the part, or the review
/// metadata is incomplete. That is a judgement call about trust, and it already
/// has its own gate in `--allow-unverified-parts`.
pub const NON_STRUCTURAL_BLOCKING: [&str; 4] = [
    "E-SYNTH-QUAL-006",
    "E-SYNTH-QUAL-007",
    "E-SYNTH-QUAL-008",
    "E-SYNTH-QUAL-015",
];

impl Finding {
    pub fn is_structural(&self) -> bool {
        STRUCTURAL_CODES.contains(&self.code.as_str())
    }

    fn blocking(
        code: &str,
        message: impl Into<String>,
        expected: impl Into<String>,
        found: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            level: FindingLevel::Blocking,
            message: message.into(),
            expected: expected.into(),
            found: found.into(),
        }
    }

    fn review(
        code: &str,
        message: impl Into<String>,
        expected: impl Into<String>,
        found: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            level: FindingLevel::Review,
            message: message.into(),
            expected: expected.into(),
            found: found.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

impl Check {
    fn pass(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: CheckStatus::Pass,
            unknown_reason: None,
            findings: Vec::new(),
        }
    }

    fn not_applicable(name: &str, reason: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            status: CheckStatus::NotApplicable,
            unknown_reason: Some(reason.into()),
            findings: Vec::new(),
        }
    }

    fn unknown(name: &str, reason: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            status: CheckStatus::Unknown,
            unknown_reason: Some(reason.into()),
            findings: Vec::new(),
        }
    }

    fn from_findings(name: &str, findings: Vec<Finding>) -> Self {
        let status = if findings.iter().any(|f| f.level == FindingLevel::Blocking) {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        };
        Self {
            name: name.to_string(),
            status,
            unknown_reason: None,
            findings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartQualification {
    pub part_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kicad_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kicad_footprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datasheet_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    pub status: CheckStatus,
    pub checks: Vec<Check>,
}

impl PartQualification {
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.checks.iter().flat_map(|c| c.findings.iter())
    }

    pub fn blocking_findings(&self) -> impl Iterator<Item = &Finding> {
        self.findings()
            .filter(|f| f.level == FindingLevel::Blocking)
    }

    pub fn review_findings(&self) -> impl Iterator<Item = &Finding> {
        self.findings().filter(|f| f.level == FindingLevel::Review)
    }

    pub fn is_fabrication_safe(&self) -> bool {
        self.status.is_pass()
    }

    pub fn structural_defects(&self) -> impl Iterator<Item = &Finding> {
        self.findings()
            .filter(|f| f.level == FindingLevel::Blocking && f.is_structural())
    }

    pub fn has_structural_defect(&self) -> bool {
        self.structural_defects().next().is_some()
    }

    pub fn unknown_checks(&self) -> impl Iterator<Item = &Check> {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub total: usize,
    pub qualified: usize,
    pub blocked: usize,
    pub unproven: usize,
    pub with_review_findings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationReport {
    pub schema_version: String,
    pub tool_version: String,
    pub summary: Summary,
    pub parts: Vec<PartQualification>,
}

impl QualificationReport {
    pub fn is_clean(&self) -> bool {
        self.summary.blocked == 0 && self.summary.unproven == 0
    }

    pub fn part(&self, id: &str) -> Option<&PartQualification> {
        self.parts.iter().find(|p| p.part_id == id)
    }

    pub fn unsafe_for_fabrication(&self) -> impl Iterator<Item = &PartQualification> {
        self.parts.iter().filter(|p| !p.is_fabrication_safe())
    }

    pub fn structurally_defective(&self) -> impl Iterator<Item = &PartQualification> {
        self.parts.iter().filter(|p| p.has_structural_defect())
    }
}

/// Mils in one millimetre. Not 25.4 — that is millimetres per inch, and
/// using it here meant a real mil/mm mix-up produced a 39.37x ratio that the
/// hint band never matched, while a genuine inch/mm error was mislabelled.
const MILS_PER_MM: f64 = 39.370_078_740_157_48;

/// Millimetres in one inch, for the other direction of the same mistake.
const MM_PER_INCH: f64 = 25.4;
const DIMENSION_RATIO_FLOOR: f64 = 0.2;
const DIMENSION_RATIO_CEILING: f64 = 8.0;
const MAX_COURTYARD_MARGIN_MM: f64 = 5.0;

pub fn qualify_registry(
    registry: &Registry,
    footprints: &dyn FootprintFacts,
    symbols: &dyn SymbolFacts,
    tool_version: &str,
) -> QualificationReport {
    let mut ordered: Vec<(&PartId, &Part)> = registry.iter().collect();
    ordered.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));

    let parts: Vec<PartQualification> = ordered
        .into_iter()
        .map(|(_, part)| qualify_part(part, footprints, symbols))
        .collect();

    let mut summary = Summary {
        total: parts.len(),
        qualified: 0,
        blocked: 0,
        unproven: 0,
        with_review_findings: 0,
    };
    for part in &parts {
        match part.status {
            CheckStatus::Pass | CheckStatus::NotApplicable => summary.qualified += 1,
            CheckStatus::Fail => summary.blocked += 1,
            CheckStatus::Unknown => summary.unproven += 1,
        }
        if part.review_findings().next().is_some() {
            summary.with_review_findings += 1;
        }
    }

    QualificationReport {
        schema_version: SCHEMA_VERSION.to_string(),
        tool_version: tool_version.to_string(),
        summary,
        parts,
    }
}

pub fn qualify_part(
    part: &Part,
    footprints: &dyn FootprintFacts,
    symbols: &dyn SymbolFacts,
) -> PartQualification {
    let pads = part
        .kicad_footprint
        .as_deref()
        .and_then(|lib_id| footprints.pads(lib_id));

    let checks = vec![
        check_pin_numbering(part),
        check_pin_pad_coverage(part, pads.as_deref()),
        check_pad_pin_coverage(part, pads.as_deref()),
        check_footprint_side(part, pads.as_deref()),
        check_symbol_pin_coverage(part, symbols),
        check_declared_dimensions(part),
        check_package_dimensions(part, pads.as_deref()),
        check_pin_classification(part),
        check_provenance(part),
    ];

    let status = if checks.iter().any(|c| c.status == CheckStatus::Fail) {
        CheckStatus::Fail
    } else if checks.iter().any(|c| c.status == CheckStatus::Unknown) {
        CheckStatus::Unknown
    } else {
        CheckStatus::Pass
    };

    PartQualification {
        part_id: part.id.0.clone(),
        kind: part.kind.clone(),
        mpn: part.mpn.clone(),
        kicad_symbol: part.kicad_symbol.clone(),
        kicad_footprint: part.kicad_footprint.clone(),
        datasheet_url: part
            .provenance
            .as_ref()
            .and_then(|p| p.datasheet_url.clone()),
        reviewed_by: part.provenance.as_ref().and_then(|p| p.reviewed_by.clone()),
        reviewed_at: part.provenance.as_ref().and_then(|p| p.reviewed_at.clone()),
        status,
        checks,
    }
}

fn check_pin_numbering(part: &Part) -> Check {
    const NAME: &str = "pin_numbering";
    let mut findings = Vec::new();

    let mut by_number: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut by_name: BTreeMap<&str, usize> = BTreeMap::new();
    let mut blank = Vec::new();

    for pin in &part.pins {
        let number = pin.number.0.trim();
        if number.is_empty() {
            blank.push(pin.name.as_str());
        } else {
            by_number.entry(number).or_default().push(pin.name.as_str());
        }
        *by_name.entry(pin.name.as_str()).or_default() += 1;
    }

    for (number, names) in &by_number {
        if names.len() > 1 {
            findings.push(Finding::blocking(
                "E-SYNTH-QUAL-003",
                "two pins share one pad number",
                format!("each pin of `{}` to claim a distinct pad", part.id.0),
                format!("pad {number} claimed by {}", names.join(", ")),
            ));
        }
    }

    for (name, count) in &by_name {
        if *count > 1 {
            findings.push(Finding::blocking(
                "E-SYNTH-QUAL-004",
                "two pins share one logical name",
                format!("each pin name of `{}` to be unique", part.id.0),
                format!("`{name}` declared {count} times"),
            ));
        }
    }

    if !blank.is_empty() {
        findings.push(Finding::blocking(
            "E-SYNTH-QUAL-013",
            "pin has no pad number",
            "every pin to carry the pad number printed in the datasheet",
            format!("blank number on: {}", blank.join(", ")),
        ));
    }

    Check::from_findings(NAME, findings)
}

fn check_pin_pad_coverage(part: &Part, pads: Option<&[PadFacts]>) -> Check {
    const NAME: &str = "pin_pad_coverage";
    let Some(lib_id) = part.kicad_footprint.as_deref() else {
        return Check::not_applicable(NAME, "part declares no kicad_footprint");
    };
    let Some(pads) = pads else {
        return Check::unknown(
            NAME,
            format!(
                "footprint `{lib_id}` could not be read; install KiCad or set KICAD_FOOTPRINT_DIR"
            ),
        );
    };

    let signal_pads: BTreeSet<&str> = pads
        .iter()
        .filter(|p| p.is_signal_pad())
        .map(|p| p.number.trim())
        .collect();
    let inert_pads: BTreeSet<&str> = pads
        .iter()
        .filter(|p| !p.is_signal_pad())
        .map(|p| p.number.trim())
        .collect();

    let declared: Vec<&str> = part
        .pins
        .iter()
        .map(|p| p.number.0.trim())
        .filter(|n| !n.is_empty())
        .collect();

    // A pin wired to a mechanical hole is a pin-map error, not a missing pad,
    // and saying so is the difference between a fixable report and a puzzle.
    let on_inert: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|n| !signal_pads.contains(n) && inert_pads.contains(n))
        .collect();
    let missing: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|n| !signal_pads.contains(n) && !inert_pads.contains(n))
        .collect();

    let mut findings = Vec::new();
    if !missing.is_empty() {
        findings.push(Finding::blocking(
            "E-SYNTH-QUAL-001",
            "declared pin number does not exist as a pad",
            format!("every pin of `{}` to be a pad of `{lib_id}`", part.id.0),
            format!(
                "{} of {} pin(s) unmatched: {}",
                missing.len(),
                part.pins.len(),
                sample(&missing)
            ),
        ));
    }
    if !on_inert.is_empty() {
        findings.push(Finding::blocking(
            "E-SYNTH-QUAL-001",
            "declared pin is mapped to a pad with no copper",
            format!(
                "every pin of `{}` to land on a copper pad of `{lib_id}`",
                part.id.0
            ),
            format!(
                "{} pin(s) map to a non-plated or copperless pad: {} — a net routed there \
                 connects to nothing",
                on_inert.len(),
                sample(&on_inert)
            ),
        ));
    }
    Check::from_findings(NAME, findings)
}

fn check_pad_pin_coverage(part: &Part, pads: Option<&[PadFacts]>) -> Check {
    const NAME: &str = "pad_pin_coverage";
    let Some(lib_id) = part.kicad_footprint.as_deref() else {
        return Check::not_applicable(NAME, "part declares no kicad_footprint");
    };
    let Some(pads) = pads else {
        return Check::unknown(NAME, format!("footprint `{lib_id}` could not be read"));
    };

    let declared: BTreeSet<&str> = part.pins.iter().map(|p| p.number.0.trim()).collect();
    let orphans: Vec<&str> = pads
        .iter()
        .filter(|p| p.is_signal_pad())
        .map(|p| p.number.trim())
        .filter(|n| !declared.contains(n))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    if orphans.is_empty() {
        return Check::pass(NAME);
    }
    Check::from_findings(
        NAME,
        vec![Finding::blocking(
            "E-SYNTH-QUAL-002",
            "footprint has copper pads no pin declares",
            format!("every copper pad of `{lib_id}` to be declared by `{}`", part.id.0),
            format!(
                "{} undeclared pad(s): {} — an exposed or thermal pad left out of the pin map is a short waiting to happen",
                orphans.len(),
                sample(&orphans)
            ),
        )],
    )
}

fn check_footprint_side(part: &Part, pads: Option<&[PadFacts]>) -> Check {
    const NAME: &str = "footprint_side";
    let Some(lib_id) = part.kicad_footprint.as_deref() else {
        return Check::not_applicable(NAME, "part declares no kicad_footprint");
    };
    let Some(pads) = pads else {
        return Check::unknown(NAME, format!("footprint `{lib_id}` could not be read"));
    };

    let copper: Vec<&PadFacts> = pads.iter().filter(|p| p.side.has_copper()).collect();
    if copper.is_empty() {
        return Check::pass(NAME);
    }

    // Any pad without front copper is unsolderable on a top-side part. Asking
    // only whether *some* pad reaches F.Cu passed the realistic case — a
    // partial mirror from a bad edit or merge, where a subset of pads moved.
    let back_only: Vec<&str> = copper
        .iter()
        .filter(|p| !p.side.includes_front())
        .map(|p| p.number.trim())
        .collect();
    if back_only.is_empty() {
        return Check::pass(NAME);
    }

    let all = back_only.len() == copper.len();
    Check::from_findings(
        NAME,
        vec![Finding::blocking(
            "E-SYNTH-QUAL-012",
            if all {
                "every copper pad sits on the back layer"
            } else {
                "some copper pads sit only on the back layer"
            },
            format!("`{lib_id}` to present every pad on F.Cu for a top-side placement"),
            format!(
                "{} of {} copper pad(s) are B.Cu only, which is how a {} footprint reads: {}",
                back_only.len(),
                copper.len(),
                if all {
                    "mirrored"
                } else {
                    "partially mirrored"
                },
                sample(&back_only)
            ),
        )],
    )
}

fn check_symbol_pin_coverage(part: &Part, symbols: &dyn SymbolFacts) -> Check {
    const NAME: &str = "symbol_pin_coverage";
    let Some(lib_id) = part.kicad_symbol.as_deref() else {
        return Check::not_applicable(
            NAME,
            "part declares no kicad_symbol; the exporter synthesizes the symbol, so there is no external pin map to disagree with",
        );
    };
    let Some(symbol_pins) = symbols.pin_numbers(lib_id) else {
        return Check::unknown(
            NAME,
            format!("symbol `{lib_id}` could not be read; install KiCad or set KICAD_SYMBOL_DIR"),
        );
    };

    let missing: Vec<&str> = part
        .pins
        .iter()
        .map(|p| p.number.0.trim())
        .filter(|n| !n.is_empty() && !symbol_pins.contains(*n))
        .collect();

    if missing.is_empty() {
        return Check::pass(NAME);
    }
    Check::from_findings(
        NAME,
        vec![Finding::blocking(
            "E-SYNTH-QUAL-005",
            "declared pin number does not exist on the KiCad symbol",
            format!("every pin of `{}` to exist on `{lib_id}`", part.id.0),
            format!("{} unmatched: {}", missing.len(), sample(&missing)),
        )],
    )
}

/// Dimension values judged on their own, with no footprint involved.
///
/// Kept separate from the pad cross-check because it needs no pads. Folding
/// the two together made a part with an implausible courtyard *qualify* while
/// the same part with a sane one stayed unproven: the pads-unavailable branch
/// had to choose one status for two unrelated questions, and a finding was
/// enough to make it a Pass.
fn check_declared_dimensions(part: &Part) -> Check {
    const NAME: &str = "declared_dimensions";
    let Some(declared) = part.footprint_dimensions.as_ref() else {
        return Check::not_applicable(NAME, "part declares no footprint_dimensions");
    };

    let mut findings = Vec::new();

    if declared.width_mm <= 0.0 || declared.height_mm <= 0.0 {
        findings.push(Finding::blocking(
            "E-SYNTH-QUAL-007",
            "declared package dimensions are not positive",
            "width_mm and height_mm to be positive millimetre values",
            format!(
                "width_mm = {}, height_mm = {}",
                declared.width_mm, declared.height_mm
            ),
        ));
    }

    if let Some(margin) = declared.courtyard_margin_mm {
        if margin < 0.0 || margin > MAX_COURTYARD_MARGIN_MM {
            findings.push(Finding::review(
                "E-SYNTH-QUAL-014",
                "courtyard margin is outside the plausible range",
                format!("0 .. {MAX_COURTYARD_MARGIN_MM} mm"),
                format!("{margin} mm"),
            ));
        }
    }

    Check::from_findings(NAME, findings)
}

fn check_package_dimensions(part: &Part, pads: Option<&[PadFacts]>) -> Check {
    const NAME: &str = "package_dimensions";
    let Some(declared) = part.footprint_dimensions.as_ref() else {
        return Check::not_applicable(NAME, "part declares no footprint_dimensions");
    };
    if part.kicad_footprint.is_none() {
        return Check::not_applicable(NAME, "part declares no kicad_footprint to cross-check");
    }
    let Some(pads) = pads else {
        return Check::unknown(
            NAME,
            "footprint pads unavailable, so declared dimensions cannot be cross-checked",
        );
    };

    let mut findings = Vec::new();

    if let Some((pad_w, pad_h)) = pad_extent(pads) {
        for (axis, declared_mm, actual_mm) in [
            ("width", declared.width_mm, pad_w),
            ("height", declared.height_mm, pad_h),
        ] {
            if actual_mm <= 0.0 || declared_mm <= 0.0 {
                continue;
            }
            let ratio = declared_mm / actual_mm;
            if (DIMENSION_RATIO_FLOOR..=DIMENSION_RATIO_CEILING).contains(&ratio) {
                continue;
            }
            findings.push(Finding::blocking(
                "E-SYNTH-QUAL-006",
                "declared package size disagrees with the footprint's pad extent",
                format!(
                    "declared {axis} within {DIMENSION_RATIO_FLOOR}x..{DIMENSION_RATIO_CEILING}x of the {actual_mm:.3} mm pad extent"
                ),
                format!(
                    "declared {declared_mm} mm ({ratio:.2}x){}",
                    unit_hint(ratio)
                ),
            ));
        }
    }

    Check::from_findings(NAME, findings)
}

fn check_pin_classification(part: &Part) -> Check {
    const NAME: &str = "pin_classification";
    let mut findings = Vec::new();

    let mut unclassified = Vec::new();
    let mut required_dnc = Vec::new();

    for pin in &part.pins {
        match pin.electrical_type {
            crate::capability::ElectricalType::Unclassified => unclassified.push(pin.name.as_str()),
            crate::capability::ElectricalType::DoNotConnect if pin.required => {
                required_dnc.push(pin.name.as_str());
            }
            _ => {}
        }
    }

    if !required_dnc.is_empty() {
        findings.push(Finding::blocking(
            "E-SYNTH-QUAL-011",
            "no-connect pin is marked required",
            "a do_not_connect pin to stay floating, never required",
            format!("required no-connect pin(s): {}", required_dnc.join(", ")),
        ));
    }

    if !unclassified.is_empty() {
        findings.push(Finding::review(
            "E-SYNTH-QUAL-010",
            "pin electrical type is unclassified",
            "every pin to declare an electrical_type",
            format!(
                "{} unclassified pin(s): {}",
                unclassified.len(),
                sample(&unclassified)
            ),
        ));
    }

    Check::from_findings(NAME, findings)
}

fn check_provenance(part: &Part) -> Check {
    const NAME: &str = "provenance";
    let mut findings = Vec::new();

    let Some(provenance) = part.provenance.as_ref() else {
        return Check::from_findings(
            NAME,
            vec![Finding::blocking(
                "E-SYNTH-QUAL-008",
                "part carries no provenance",
                "a [provenance] table naming the reviewer and datasheet",
                "no provenance recorded".to_string(),
            )],
        );
    };

    let reviewer = provenance
        .reviewed_by
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());

    if reviewer.is_none() {
        findings.push(Finding::blocking(
            "E-SYNTH-QUAL-008",
            "part has no reviewer",
            "[provenance].reviewed_by to name whoever checked this part against its datasheet",
            "reviewed_by is empty".to_string(),
        ));
    }

    match provenance
        .datasheet_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        None => findings.push(Finding::review(
            "E-SYNTH-QUAL-009",
            "part has no datasheet URL",
            "[provenance].datasheet_url so a reviewer can re-derive the pin map",
            "datasheet_url is empty".to_string(),
        )),
        Some(url) if !(url.starts_with("http://") || url.starts_with("https://")) => {
            findings.push(Finding::review(
                "E-SYNTH-QUAL-009",
                "datasheet URL is not resolvable",
                "an http(s) URL",
                format!("`{url}`"),
            ));
        }
        Some(_) => {}
    }

    let review_date = provenance
        .reviewed_at
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());

    if reviewer.is_some() {
        match review_date {
            None => findings.push(Finding::blocking(
                "E-SYNTH-QUAL-015",
                "review metadata is incomplete",
                "[provenance].reviewed_at to date the review that reviewed_by claims",
                "reviewed_by is set but reviewed_at is empty".to_string(),
            )),
            Some(date) if !is_iso_8601_date(date) => findings.push(Finding::blocking(
                "E-SYNTH-QUAL-015",
                "review date is not a usable ISO-8601 date",
                "an ISO-8601 date such as 2026-10-02",
                format!("`{date}`"),
            )),
            Some(_) => {}
        }
    }

    Check::from_findings(NAME, findings)
}

fn pad_extent(pads: &[PadFacts]) -> Option<(f64, f64)> {
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    let mut seen = false;

    for pad in pads.iter().filter(|p| p.side.has_copper()) {
        seen = true;
        let (cx, cy) = pad.center_mm;
        let (w, h) = pad.size_mm;
        min_x = min_x.min(cx - w / 2.0);
        max_x = max_x.max(cx + w / 2.0);
        min_y = min_y.min(cy - h / 2.0);
        max_y = max_y.max(cy + h / 2.0);
    }

    seen.then_some((max_x - min_x, max_y - min_y))
}

fn unit_hint(ratio: f64) -> &'static str {
    let near = |target: f64| (ratio / target - 1.0).abs() < 0.25;
    if near(MILS_PER_MM) {
        " — about 39.4x, which is a millimetre measurement recorded as mils"
    } else if near(1.0 / MILS_PER_MM) {
        " — about 1/39.4x, which is a mil measurement recorded as millimetres"
    } else if near(MM_PER_INCH) {
        " — about 25.4x, which is an inch measurement recorded as millimetres"
    } else if near(1.0 / MM_PER_INCH) {
        " — about 1/25.4x, which is a millimetre measurement recorded as inches"
    } else if near(10.0) || near(0.1) {
        " — about a factor of ten, which is a misplaced decimal point"
    } else {
        ""
    }
}

fn is_iso_8601_date(value: &str) -> bool {
    let date = value.split(['T', ' ']).next().unwrap_or(value);
    let mut parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let digits = |s: &str, len: usize| s.len() == len && s.chars().all(|c| c.is_ascii_digit());
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return false;
    }
    let month: u32 = month.parse().unwrap_or(0);
    let day: u32 = day.parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn sample(items: &[&str]) -> String {
    const LIMIT: usize = 6;
    let shown = items
        .iter()
        .take(LIMIT)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    let rest = items.len().saturating_sub(LIMIT);
    if rest == 0 {
        shown
    } else {
        format!("{shown}, +{rest} more")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::ElectricalType;
    use crate::part::{Pin, PinNumber, Provenance};

    struct Pads(Vec<PadFacts>);

    impl FootprintFacts for Pads {
        fn pads(&self, _lib_id: &str) -> Option<Vec<PadFacts>> {
            Some(self.0.clone())
        }
    }

    struct Symbol(BTreeSet<String>);

    impl SymbolFacts for Symbol {
        fn pin_numbers(&self, _lib_id: &str) -> Option<BTreeSet<String>> {
            Some(self.0.clone())
        }
    }

    fn pad(number: &str, cx: f64, cy: f64) -> PadFacts {
        PadFacts {
            number: number.to_string(),
            center_mm: (cx, cy),
            size_mm: (0.5, 0.5),
            side: PadSide::Front,
            is_npth: false,
        }
    }

    fn pin(name: &str, number: &str) -> Pin {
        Pin {
            name: name.to_string(),
            number: PinNumber(number.to_string()),
            electrical_type: ElectricalType::Bidirectional,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn reviewed() -> Provenance {
        Provenance {
            reviewed_by: Some("a reviewer".into()),
            reviewed_at: Some("2026-10-02".into()),
            datasheet_url: Some("https://example.invalid/ds.pdf".into()),
            ..Provenance::default()
        }
    }

    fn part_with(pins: Vec<Pin>) -> Part {
        Part {
            id: crate::part::PartId("subject".into()),
            kind: "mcu".into(),
            description: None,
            version: 0,
            lifecycle: crate::part::Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: Some("SUBJECT-1".into()),
            lcsc_pn: None,
            provenance: Some(reviewed()),
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: Some("Lib:SUBJECT".into()),
            kicad_footprint: Some("Lib:SUBJECT".into()),
            footprint_dimensions: Some(crate::part::FootprintDimensions {
                width_mm: PAD_EXTENT_MM.0,
                height_mm: PAD_EXTENT_MM.1,
                courtyard_margin_mm: Some(0.25),
                mating_face: None,
                label_side: None,
                label_hidden: None,
            }),
            operating_conditions: None,
        }
    }

    const PAD_EXTENT_MM: (f64, f64) = (1.5, 0.5);

    fn two_pads() -> Pads {
        Pads(vec![pad("1", -0.5, 0.0), pad("2", 0.5, 0.0)])
    }

    fn two_pin_symbol() -> Symbol {
        Symbol(["1".to_string(), "2".to_string()].into_iter().collect())
    }

    fn findings(q: &PartQualification) -> Vec<&str> {
        q.findings().map(|f| f.code.as_str()).collect()
    }

    #[test]
    fn a_coherent_part_qualifies() {
        let part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert_eq!(q.status, CheckStatus::Pass, "{:#?}", q.checks);
        assert!(q.is_fabrication_safe());
    }

    #[test]
    fn a_missing_pad_blocks() {
        let part = part_with(vec![pin("a", "1"), pin("b", "2"), pin("c", "99")]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-001"),
            "{:#?}",
            q.checks
        );
        assert_eq!(q.status, CheckStatus::Fail);
        assert!(!q.is_fabrication_safe());
    }

    #[test]
    fn an_undeclared_exposed_pad_blocks() {
        let pads = Pads(vec![
            pad("1", -0.5, 0.0),
            pad("2", 0.5, 0.0),
            pad("3", 0.0, 0.0),
        ]);
        let part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        let q = qualify_part(&part, &pads, &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-002"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn a_mechanical_npth_pad_is_not_an_orphan() {
        let mut npth = pad("MP", 0.0, 1.0);
        npth.is_npth = true;
        npth.side = PadSide::NoCopper;
        let pads = Pads(vec![pad("1", -0.5, 0.0), pad("2", 0.5, 0.0), npth]);
        let part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        let q = qualify_part(&part, &pads, &two_pin_symbol());
        assert!(
            !findings(&q).contains(&"E-SYNTH-QUAL-002"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn swapped_pins_sharing_a_pad_block() {
        let part = part_with(vec![pin("a", "1"), pin("b", "1")]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-003"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn a_duplicate_pin_name_blocks() {
        let part = part_with(vec![pin("a", "1"), pin("a", "2")]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-004"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn stray_whitespace_in_a_pad_number_is_not_a_pin_map_error() {
        let mut part = part_with(vec![pin("a", "1 "), pin("b", "2")]);
        part.pins[0].number = PinNumber(" 1 ".to_string());
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        let codes: Vec<&str> = q.findings().map(|f| f.code.as_str()).collect();
        assert!(
            !codes.contains(&"E-SYNTH-QUAL-001") && !codes.contains(&"E-SYNTH-QUAL-002"),
            "a space must not fabricate a structural defect: {codes:?}\n{:#?}",
            q.checks
        );
        assert!(!q.has_structural_defect());
    }

    #[test]
    fn a_pin_mapped_to_a_copperless_pad_blocks() {
        let mut hole = pad("MP1", 0.0, 1.0);
        hole.is_npth = true;
        hole.side = PadSide::NoCopper;
        let pads = Pads(vec![pad("1", -0.5, 0.0), pad("2", 0.5, 0.0), hole]);

        let part = part_with(vec![pin("a", "1"), pin("b", "2"), pin("shield", "MP1")]);
        let q = qualify_part(&part, &pads, &two_pin_symbol());
        let f: Vec<&Finding> = q
            .findings()
            .filter(|f| f.code == "E-SYNTH-QUAL-001")
            .collect();
        assert!(
            f.iter().any(|f| f.found.contains("non-plated")),
            "a pin on a mechanical hole must be named as such: {:#?}",
            q.checks
        );
        assert!(q.has_structural_defect());
    }

    #[test]
    fn a_partially_mirrored_footprint_blocks() {
        let mut pads = Pads(vec![
            pad("1", -0.5, -0.5),
            pad("2", 0.5, -0.5),
            pad("3", 0.5, 0.5),
            pad("4", -0.5, 0.5),
        ]);
        pads.0[2].side = PadSide::Back;
        pads.0[3].side = PadSide::Back;

        let part = part_with(vec![
            pin("a", "1"),
            pin("b", "2"),
            pin("c", "3"),
            pin("d", "4"),
        ]);
        let q = qualify_part(&part, &pads, &two_pin_symbol());
        let f: Vec<&Finding> = q
            .findings()
            .filter(|f| f.code == "E-SYNTH-QUAL-012")
            .collect();
        assert_eq!(f.len(), 1, "{:#?}", q.checks);
        assert!(f[0].found.contains("partially mirrored"), "{:#?}", f[0]);
        assert!(f[0].found.contains("2 of 4"), "{:#?}", f[0]);
    }

    /// Structural is an allow-list so a future code cannot inherit the power
    /// to refuse a fab export with no override.
    /// Every blocking code the engine emits must be classified, in exactly one
    /// of STRUCTURAL_CODES or NON_STRUCTURAL_BLOCKING.
    ///
    /// The emitted set is read out of this file rather than hand-listed, so a
    /// new `Finding::blocking` fails the test until somebody decides whether it
    /// refuses a fab export. E-SYNTH-QUAL-013 was emitted as blocking and
    /// missing from the allow-list, which silently made a blank pad number
    /// overrideable; nothing cross-checked the two, so only a reviewer caught
    /// it.
    #[test]
    fn every_blocking_code_is_classified() {
        let source = include_str!("qualify.rs");
        let mut emitted: BTreeSet<&str> = BTreeSet::new();
        let mut rest = source;
        while let Some(at) = rest.find("Finding::blocking(") {
            rest = &rest[at + "Finding::blocking(".len()..];
            let Some(open) = rest.find('"') else { break };
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else { break };
            let code = &after[..close];
            if code.starts_with("E-SYNTH-QUAL-") {
                emitted.insert(code);
            }
        }

        assert!(
            emitted.len() >= 10,
            "the scan should find every blocking site, found {}: {emitted:?}",
            emitted.len()
        );

        for code in &emitted {
            let structural = STRUCTURAL_CODES.contains(code);
            let excluded = NON_STRUCTURAL_BLOCKING.contains(code);
            assert!(
                structural || excluded,
                "{code} is emitted as blocking but classified nowhere. Add it to \
                 STRUCTURAL_CODES if a wrong part definition must refuse a fab \
                 export with no override, or to NON_STRUCTURAL_BLOCKING with the \
                 reason it should not."
            );
            assert!(
                !(structural && excluded),
                "{code} is in both classification lists"
            );
        }

        for code in STRUCTURAL_CODES {
            assert!(
                emitted.contains(code),
                "{code} is listed as structural but never emitted as blocking"
            );
        }
    }

    /// A blank pad number is a pin-map defect, so it refuses a fab export.
    /// Regression for the omission reported on #78.
    #[test]
    fn a_blank_pad_number_is_structural() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.pins[1].number = PinNumber("   ".to_string());
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        let codes: Vec<&str> = q.structural_defects().map(|f| f.code.as_str()).collect();
        assert!(
            codes.contains(&"E-SYNTH-QUAL-013"),
            "a pin with no pad number must block a fab export: {:#?}",
            q.checks
        );
        assert!(q.has_structural_defect());
    }

    /// A do-not-connect pin marked required forces every design to connect a
    /// pin the manufacturer says must float, so it refuses a fab export too.
    #[test]
    fn a_required_no_connect_pin_is_structural() {
        let mut dnc = pin("nc", "2");
        dnc.electrical_type = ElectricalType::DoNotConnect;
        dnc.required = true;
        let part = part_with(vec![pin("a", "1"), dnc]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        let codes: Vec<&str> = q.structural_defects().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"E-SYNTH-QUAL-011"), "{:#?}", q.checks);
    }

    #[test]
    fn only_pin_map_codes_are_structural() {
        for code in STRUCTURAL_CODES {
            let f = Finding::blocking(code, "m", "e", "f");
            assert!(f.is_structural(), "{code} should be structural");
        }
        for code in [
            "E-SYNTH-QUAL-006",
            "E-SYNTH-QUAL-007",
            "E-SYNTH-QUAL-008",
            "E-SYNTH-QUAL-014",
            "E-SYNTH-QUAL-999",
        ] {
            let f = Finding::blocking(code, "m", "e", "f");
            assert!(!f.is_structural(), "{code} must not block fab export");
        }
    }

    #[test]
    fn a_pin_absent_from_the_symbol_blocks() {
        let part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        let symbol = Symbol(["1".to_string()].into_iter().collect());
        let q = qualify_part(&part, &two_pads(), &symbol);
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-005"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn a_mirrored_footprint_blocks() {
        let mut back = two_pads();
        for p in &mut back.0 {
            p.side = PadSide::Back;
        }
        let part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        let q = qualify_part(&part, &back, &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-012"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn millimetres_recorded_as_mils_block_with_a_unit_hint() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.footprint_dimensions.as_mut().unwrap().width_mm = PAD_EXTENT_MM.0 * MILS_PER_MM;
        part.footprint_dimensions.as_mut().unwrap().height_mm = PAD_EXTENT_MM.1 * MILS_PER_MM;
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        let hinted: Vec<&Finding> = q
            .findings()
            .filter(|f| f.code == "E-SYNTH-QUAL-006")
            .collect();
        assert!(!hinted.is_empty(), "{:#?}", q.checks);
        assert!(
            hinted.iter().any(|f| f.found.contains("recorded as mils")),
            "{hinted:#?}"
        );
    }

    #[test]
    fn a_tenfold_dimension_typo_blocks() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.footprint_dimensions.as_mut().unwrap().width_mm = PAD_EXTENT_MM.0 * 10.0;
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            q.findings()
                .any(|f| f.code == "E-SYNTH-QUAL-006" && f.found.contains("factor of ten")),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn a_required_no_connect_pin_blocks() {
        let mut dnc = pin("nc", "2");
        dnc.electrical_type = ElectricalType::DoNotConnect;
        dnc.required = true;
        let part = part_with(vec![pin("a", "1"), dnc]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-011"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn an_unreviewed_part_blocks() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.provenance.as_mut().unwrap().reviewed_by = None;
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-008"),
            "{:#?}",
            q.checks
        );
        assert!(!q.is_fabrication_safe());
    }

    #[test]
    fn stale_review_metadata_blocks() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.provenance.as_mut().unwrap().reviewed_at = None;
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-015"),
            "{:#?}",
            q.checks
        );

        let mut garbled = part_with(vec![pin("a", "1"), pin("b", "2")]);
        garbled.provenance.as_mut().unwrap().reviewed_at = Some("last tuesday".into());
        let q = qualify_part(&garbled, &two_pads(), &two_pin_symbol());
        assert!(
            findings(&q).contains(&"E-SYNTH-QUAL-015"),
            "{:#?}",
            q.checks
        );
    }

    #[test]
    fn a_missing_datasheet_is_review_not_blocking() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.provenance.as_mut().unwrap().datasheet_url = None;
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        let f: Vec<&Finding> = q
            .findings()
            .filter(|f| f.code == "E-SYNTH-QUAL-009")
            .collect();
        assert_eq!(f.len(), 1, "{:#?}", q.checks);
        assert_eq!(f[0].level, FindingLevel::Review);
        assert_eq!(
            q.status,
            CheckStatus::Pass,
            "a documentation gap is reported, not failed: {:#?}",
            q.checks
        );
        assert!(
            q.is_fabrication_safe(),
            "a review gap must not make a part unfabricable"
        );
        assert_eq!(q.review_findings().count(), 1);
        assert_eq!(q.blocking_findings().count(), 0);
    }

    #[test]
    fn review_only_findings_leave_a_part_qualified() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.provenance.as_mut().unwrap().datasheet_url = None;
        part.pins[1].electrical_type = ElectricalType::Unclassified;

        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        let codes: Vec<&str> = q.review_findings().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"E-SYNTH-QUAL-009"), "{codes:?}");
        assert!(codes.contains(&"E-SYNTH-QUAL-010"), "{codes:?}");
        assert_eq!(q.status, CheckStatus::Pass, "{:#?}", q.checks);

        let mut registry = Registry::new();
        registry.insert(part);
        let report = qualify_registry(&registry, &two_pads(), &two_pin_symbol(), "test");
        assert_eq!(report.summary.blocked, 0, "{report:#?}");
        assert_eq!(report.summary.qualified, 1, "{report:#?}");
        assert_eq!(report.summary.with_review_findings, 1, "{report:#?}");
        assert!(
            report.is_clean(),
            "review gaps must not make the command exit non-zero"
        );
    }

    #[test]
    fn a_blocking_finding_beside_a_review_one_still_fails() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.provenance.as_mut().unwrap().datasheet_url = None;
        part.provenance.as_mut().unwrap().reviewed_by = None;

        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert_eq!(q.status, CheckStatus::Fail, "{:#?}", q.checks);
        assert!(q.review_findings().any(|f| f.code == "E-SYNTH-QUAL-009"));
        assert!(q.blocking_findings().any(|f| f.code == "E-SYNTH-QUAL-008"));
    }

    /// Regression: the pads-unavailable branch of the dimensions check used to
    /// pick one status for two unrelated questions, so a part with an
    /// implausible courtyard qualified while the same part with a sane one
    /// stayed unproven. Adding a defect must never improve a verdict.
    #[test]
    fn adding_a_defect_never_improves_a_verdict() {
        let mut sane = part_with(vec![pin("a", "1"), pin("b", "2")]);
        sane.kicad_footprint = None;
        sane.kicad_symbol = None;
        sane.footprint_dimensions
            .as_mut()
            .unwrap()
            .courtyard_margin_mm = Some(0.25);
        let a = qualify_part(&sane, &NoFacts, &NoFacts);

        let mut broken = part_with(vec![pin("a", "1"), pin("b", "2")]);
        broken.kicad_footprint = None;
        broken.kicad_symbol = None;
        broken
            .footprint_dimensions
            .as_mut()
            .unwrap()
            .courtyard_margin_mm = Some(50.0);
        let b = qualify_part(&broken, &NoFacts, &NoFacts);

        eprintln!(
            "sane courtyard        -> {:?} fab_safe={}",
            a.status,
            a.is_fabrication_safe()
        );
        eprintln!(
            "implausible courtyard -> {:?} fab_safe={}",
            b.status,
            b.is_fabrication_safe()
        );
        assert!(
            !b.is_fabrication_safe() || a.is_fabrication_safe(),
            "adding a defect flipped the part from unproven to fabrication-safe"
        );
        assert_eq!(
            a.status, b.status,
            "a review-level finding must not change the part's status"
        );
        assert!(
            b.review_findings().any(|f| f.code == "E-SYNTH-QUAL-014"),
            "the implausible courtyard must still be reported: {:#?}",
            b.checks
        );
    }

    #[test]
    fn a_review_finding_survives_an_unreadable_footprint() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.footprint_dimensions
            .as_mut()
            .unwrap()
            .courtyard_margin_mm = Some(99.0);
        let q = qualify_part(&part, &NoFacts, &NoFacts);
        let codes: Vec<&str> = q.findings().map(|f| f.code.as_str()).collect();
        assert!(
            codes.contains(&"E-SYNTH-QUAL-014"),
            "an implausible courtyard must still be reported when pads are unavailable: {:#?}",
            q.checks
        );
        assert_eq!(
            q.status,
            CheckStatus::Unknown,
            "a declared footprint that cannot be read leaves the part unproven: {:#?}",
            q.checks
        );
        assert!(!q.is_fabrication_safe());
    }

    #[test]
    fn an_unreadable_footprint_is_unknown_not_pass() {
        let part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        let q = qualify_part(&part, &NoFacts, &NoFacts);
        assert_eq!(q.status, CheckStatus::Unknown, "{:#?}", q.checks);
        assert!(
            !q.is_fabrication_safe(),
            "an unproven part must not be fabrication safe"
        );
        assert!(q.findings().next().is_none(), "unknown is not a finding");
        let names: Vec<&str> = q.unknown_checks().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"pin_pad_coverage"), "{names:?}");
        assert!(names.contains(&"symbol_pin_coverage"), "{names:?}");
    }

    #[test]
    fn the_report_orders_parts_and_counts_outcomes() {
        let mut registry = Registry::new();
        let mut good = part_with(vec![pin("a", "1"), pin("b", "2")]);
        good.id = crate::part::PartId("zeta".into());
        let mut bad = part_with(vec![pin("a", "1"), pin("b", "1")]);
        bad.id = crate::part::PartId("alpha".into());
        registry.insert(good);
        registry.insert(bad);

        let report = qualify_registry(&registry, &two_pads(), &two_pin_symbol(), "test 0.0.1");
        assert_eq!(
            report
                .parts
                .iter()
                .map(|p| p.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        assert_eq!(report.summary.total, 2);
        assert_eq!(report.summary.qualified, 1);
        assert_eq!(report.summary.blocked, 1);
        assert!(!report.is_clean());
        assert_eq!(report.unsafe_for_fabrication().count(), 1);
    }

    #[test]
    fn provenance_findings_are_not_structural_defects() {
        let mut part = part_with(vec![pin("a", "1"), pin("b", "2")]);
        part.provenance.as_mut().unwrap().reviewed_by = None;
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert_eq!(q.status, CheckStatus::Fail);
        assert!(
            !q.has_structural_defect(),
            "an unreviewed part is not structurally wrong: {:#?}",
            q.structural_defects().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_wrong_pin_map_is_a_structural_defect() {
        let part = part_with(vec![pin("a", "1"), pin("b", "99")]);
        let q = qualify_part(&part, &two_pads(), &two_pin_symbol());
        assert!(q.has_structural_defect());
        let codes: Vec<&str> = q.structural_defects().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"E-SYNTH-QUAL-001"), "{codes:?}");
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let mut registry = Registry::new();
        registry.insert(part_with(vec![pin("a", "1"), pin("b", "2")]));
        let report = qualify_registry(&registry, &two_pads(), &two_pin_symbol(), "test 0.0.1");
        let json = serde_json::to_string(&report).expect("serialize");
        let back: QualificationReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, back);
        assert_eq!(back.schema_version, SCHEMA_VERSION);
    }
}
