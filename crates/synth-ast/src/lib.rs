// SPDX-License-Identifier: Apache-2.0

//! Abstract syntax tree for SynthSpec.
//!
//! Enum AST nodes are `#[non_exhaustive]` so callers must handle
//! additions explicitly. Structs are left ordinary so cross-crate
//! constructors work; new fields will be added in minor releases.
//!
//! Every node carries a [`Span`] for diagnostic provenance.
//!
//! AST types derive `serde::Serialize` and `serde::Deserialize` so the
//! same type acts as both the parser's output and the JSON ingestion
//! frontend's output. JSON in, JSON out, identical structure.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use synth_diagnostics::Span;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramAst {
    pub imports: Vec<ImportAst>,
    pub board: BoardAst,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportAst {
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardAst {
    pub name: String,
    pub statements: Vec<StatementAst>,
    pub span: Span,
}

/// Statement node. The serde tag is `stmt` rather than `kind` to avoid a
/// JSON-shape collision with [`ComponentDeclAst::kind`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stmt", rename_all = "snake_case")]
#[non_exhaustive]
pub enum StatementAst {
    Layers(LayersStmt),
    Manufacturer(ManufacturerStmt),
    Revision(RevisionStmt),
    Component(ComponentDeclAst),
    Connection(ConnectionAst),
    DiffPair(DiffPairStmt),
    Keepout(KeepoutStmt),
    Group(GroupStmt),
}

impl StatementAst {
    pub fn span(&self) -> Span {
        match self {
            StatementAst::Layers(s) => s.span,
            StatementAst::Manufacturer(s) => s.span,
            StatementAst::Revision(s) => s.span,
            StatementAst::Component(s) => s.span,
            StatementAst::Connection(s) => s.span,
            StatementAst::DiffPair(s) => s.span,
            StatementAst::Keepout(s) => s.span,
            StatementAst::Group(s) => s.span,
        }
    }
}

/// A named sub-circuit: `group "USB-C input" { ... }`.
///
/// A group is an **annotation, not a scope**. Its statements are
/// lowered exactly as if they had been written directly in the board
/// body — refdes stay board-unique and a `connect` inside a group may
/// name any component on the board. Nesting the connections that cross
/// sub-circuits is the whole point: those are the ones that become
/// inter-sheet labels once a design is split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupStmt {
    pub name: String,
    pub statements: Vec<StatementAst>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayersStmt {
    pub count: u32,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturerStmt {
    pub name: String,
    pub span: Span,
}

/// Board revision tag, carried into the schematic title block
/// (Sierra Circuits "Schematic Design Rules": the title block should
/// display the Revision).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionStmt {
    pub rev: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDeclAst {
    pub refdes: String,
    pub kind: String,
    /// `Some(mpn)` for concrete components (`component U1: mcu "rp2350"`),
    /// `None` for abstract components (introduced in Phase 2).
    pub part: Option<String>,
    /// Optional user-specified display value (`component R1: resistor "r_generic_0603" value "10k"`).
    /// Used for the schematic `Value` property and the BOM when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_hint: Option<PlacementHintAst>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementHintAst {
    pub attrs: Vec<PlacementHintAttr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementHintAttr {
    Region(String, Span),
    Edge(String, Span),
    Near(String, Span),
    Side(String, Span),
    Priority(String, Span),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionAst {
    pub from: EndpointAst,
    pub to: EndpointAst,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointAst {
    pub component: String,
    pub pin: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffPairStmt {
    pub pos: String,
    pub neg: String,
    pub attrs: Vec<DiffPairAttr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiffPairAttr {
    Impedance(ValueWithUnit),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepoutStmt {
    pub name: String,
    pub attrs: Vec<KeepoutAttr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KeepoutAttr {
    Radius(ValueWithUnit),
}

/// A numeric literal paired with an engineering unit, e.g. `90ohm`, `20mm`.
///
/// Stored as a serialized decimal string plus a unit token to preserve the
/// user's exact written form (no float rounding). Phase 2 will convert
/// these to integer base units (nanometers, microvolts, ...) inside the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueWithUnit {
    pub literal: String,
    pub unit: Unit,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Unit {
    // Length
    Mm,
    Mil,
    // Resistance
    Ohm,
    Kohm,
    Mohm,
    // Voltage
    V,
    Mv,
    // Current
    A,
    Ma,
    // Frequency
    Mhz,
    Ghz,
    // Capacitance
    Pf,
    Nf,
    Uf,
}

/// Returned by `<Unit as FromStr>::from_str` when a unit token is
/// not recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownUnit;

impl std::fmt::Display for UnknownUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unknown engineering unit")
    }
}

impl std::error::Error for UnknownUnit {}

impl std::str::FromStr for Unit {
    type Err = UnknownUnit;

    /// Parse the canonical lowercase spelling. The lexer reports unknown
    /// units as `E-SYNTH-PARSE-023`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "mm" => Unit::Mm,
            "mil" => Unit::Mil,
            "ohm" => Unit::Ohm,
            "kohm" => Unit::Kohm,
            "mohm" => Unit::Mohm,
            "v" => Unit::V,
            "mv" => Unit::Mv,
            "a" => Unit::A,
            "ma" => Unit::Ma,
            "mhz" => Unit::Mhz,
            "ghz" => Unit::Ghz,
            "pf" => Unit::Pf,
            "nf" => Unit::Nf,
            "uf" => Unit::Uf,
            _ => return Err(UnknownUnit),
        })
    }
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Mil => "mil",
            Unit::Ohm => "ohm",
            Unit::Kohm => "kohm",
            Unit::Mohm => "mohm",
            Unit::V => "v",
            Unit::Mv => "mv",
            Unit::A => "a",
            Unit::Ma => "ma",
            Unit::Mhz => "mhz",
            Unit::Ghz => "ghz",
            Unit::Pf => "pf",
            Unit::Nf => "nf",
            Unit::Uf => "uf",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_round_trip() {
        use std::str::FromStr as _;
        for u in [
            Unit::Mm,
            Unit::Mil,
            Unit::Ohm,
            Unit::Kohm,
            Unit::V,
            Unit::Mv,
            Unit::A,
            Unit::Ma,
            Unit::Mhz,
            Unit::Ghz,
            Unit::Pf,
            Unit::Nf,
            Unit::Uf,
        ] {
            assert_eq!(Unit::from_str(u.as_str()).unwrap(), u);
        }
        assert!(Unit::from_str("parsec").is_err());
    }

    #[test]
    fn ast_json_round_trip() {
        let ast = ProgramAst {
            imports: vec![ImportAst {
                path: "stdlib.synth".into(),
                span: Span::new(0, 21),
            }],
            board: BoardAst {
                name: "hello".into(),
                statements: vec![StatementAst::Layers(LayersStmt {
                    count: 4,
                    span: Span::new(40, 48),
                })],
                span: Span::new(22, 60),
            },
            span: Span::new(0, 60),
        };
        let json = serde_json::to_string(&ast).unwrap();
        let back: ProgramAst = serde_json::from_str(&json).unwrap();
        assert_eq!(ast, back);
    }

    #[test]
    fn statement_tagged_on_stmt_to_avoid_kind_collision() {
        let s = StatementAst::Layers(LayersStmt {
            count: 4,
            span: Span::new(0, 0),
        });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["stmt"], "layers");
        assert_eq!(v["count"], 4);

        // Critical: this case must not corrupt the kind field of a
        // component, which is itself called "kind" in the struct.
        let c = StatementAst::Component(ComponentDeclAst {
            refdes: "U1".into(),
            kind: "mcu".into(),
            part: Some("rp2350".into()),
            value: Some("10k".into()),
            placement_hint: None,
            span: Span::new(0, 0),
        });
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["stmt"], "component");
        assert_eq!(v["kind"], "mcu");
        assert_eq!(v["refdes"], "U1");
    }
}
