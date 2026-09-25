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

use std::collections::BTreeMap;

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
    Company(CompanyStmt),
    Legends(LegendsStmt),
    Component(ComponentDeclAst),
    Variant(VariantDeclStmt),
    Connection(ConnectionAst),
    Net(NetDeclAst),
    Power(PowerDeclAst),
    Notes(NotesDeclAst),
    Module(ModuleDeclStmt),
    Interface(InterfaceDeclStmt),
    Bus(BusDeclStmt),
    Use(UseStmt),
    Bind(BindStmt),
    DiffPair(DiffPairStmt),
    Netclass(NetclassStmt),
    Keepout(KeepoutStmt),
    Group(GroupStmt),
    Sheet(SheetStmt),
}

impl StatementAst {
    pub fn span(&self) -> Span {
        match self {
            StatementAst::Layers(s) => s.span,
            StatementAst::Manufacturer(s) => s.span,
            StatementAst::Revision(s) => s.span,
            StatementAst::Company(s) => s.span,
            StatementAst::Legends(s) => s.span,
            StatementAst::Component(s) => s.span,
            StatementAst::Variant(s) => s.span,
            StatementAst::Connection(s) => s.span,
            StatementAst::Net(s) => s.span,
            StatementAst::Power(s) => s.span,
            StatementAst::Notes(s) => s.span,
            StatementAst::Module(s) => s.span,
            StatementAst::Interface(s) => s.span,
            StatementAst::Bus(s) => s.span,
            StatementAst::Use(s) => s.span,
            StatementAst::Bind(s) => s.span,
            StatementAst::DiffPair(s) => s.span,
            StatementAst::Netclass(s) => s.span,
            StatementAst::Keepout(s) => s.span,
            StatementAst::Group(s) => s.span,
            StatementAst::Sheet(s) => s.span,
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
    /// Optional attributes between the name and the body:
    /// `group "3.3V LDO" color "#c2410c" title "3.3 V regulator" { … }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<GroupAttr>,
    pub statements: Vec<StatementAst>,
    pub span: Span,
}

/// A `group` header attribute (schematic-quality plan Phase D1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum GroupAttr {
    /// Explicit box hue (`color "#c2410c"`), overriding the
    /// deterministic palette colour.
    Color(String),
    /// Pin the region to a page quadrant (`region top_left`), reusing
    /// the placement-region vocabulary.
    Region(String),
    /// Display title when it should differ from the identifier.
    Title(String),
}

/// A hierarchical sheet block: `sheet "Power" { ... }`.
///
/// Lowering flattens a sheet's statements into the board exactly like
/// a [`GroupStmt`]: refdes stay board-unique and a `connect` inside a
/// sheet may name any component on the board. Each lowered component
/// records the innermost enclosing sheet name (alongside its group).
///
/// The sheet name is a **split boundary** (§P26): a board whose
/// single-sheet content overflows A2 and whose components span two or
/// more sheets exports as a KiCad hierarchy — one `.kicad_sch` per
/// sheet plus a root carrying sheet instances — with cross-sheet
/// signal nets carried on hierarchical labels. Small boards stay a
/// single sheet, so the annotation also just lets layout cluster per
/// sheet and reviewers read the intended split.
///
/// `import` files are implicit boundaries too: an imported file's
/// statements arrive wrapped in a sheet named after the file stem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetStmt {
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
/// which by convention displays the revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionStmt {
    pub rev: String,
    pub span: Span,
}

/// Design-authority company name, carried into the schematic title
/// block's Company field, which by convention names the design
/// authority. Unlike
/// `ManufacturerStmt` (who *builds* the board), this names who
/// *designed* it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyStmt {
    pub name: String,
    pub span: Span,
}

/// Connector pin legends (`legends on|off`, default off).
/// Schematic-quality plan Phase A3: generated per-pin connector
/// legends are opt-in — the reference sheet carries a one-line prose
/// note instead of a pin dump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegendsStmt {
    pub enabled: bool,
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
    /// Do-not-populate (`component R7: resistor "r_generic_0603" dnp`).
    /// Exports `(dnp yes)` on the KiCad symbol and leaves the part
    /// out of the BOM and pick-and-place; ERC still checks it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dnp: bool,
    /// Structured component data beyond the display `value`, keyed by
    /// the canonical KiCad field name (`Tolerance`, `Voltage`,
    /// `Power`, `Dielectric`). Exported as hidden symbol properties so
    /// KiCad BOM tooling and the derating checks can read them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_hint: Option<PlacementHintAst>,
    pub span: Span,
}

/// A named design variant: `variant "lite" { dnp U3 }`.
///
/// Variants share one schematic and layout but differ in what is
/// populated. Exported to KiCad's native design variants: the project
/// file lists the name/description, and each affected symbol carries a
/// `(variant …)` block inside its `(instances …)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantDeclStmt {
    pub name: String,
    /// Optional human description (`variant "lite" description "…"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Refdes left unpopulated in this variant, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dnp: Vec<String>,
    pub span: Span,
}

/// A free-text design note: `notes "Title" { "line one" "line two" }`.
///
/// Rendered on the schematic as a titled text block (§21.1): the
/// title at caption size, one run per line below it. A `notes` block
/// inside a `group` carries that group's name and renders beneath
/// the group; top-level notes stack at the sheet's bottom-left.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotesDeclAst {
    pub title: String,
    #[serde(default)]
    pub lines: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementHintAst {
    /// Optional component reference designator for a board-level hint.
    /// Component-local hints leave this unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
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
    /// Extra `->`-targets for one-to-many fanout
    /// (`connect U1.vout -> C3.p1, U2.vdd`), empty for a plain pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional: Vec<EndpointAst>,
    /// Explicit net name from `as "NAME"`. All endpoints of this
    /// statement (and every other statement naming the same string)
    /// merge into one named net.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_name: Option<String>,
    /// Netclass join from `class "NAME"` on the connect line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netclass: Option<String>,
    pub span: Span,
}

/// What an [`EndpointAst`] names.
///
/// `Pin` is the classic `U1.vout`. `Port` is a bare identifier naming a
/// module port (`-> vdd`); `Net` is a quoted net name (`-> "+3V3"`,
/// `-> "I2C0.sda"`), which is how a bus member is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointRefKind {
    #[default]
    Pin,
    Port,
    Net,
}

impl EndpointRefKind {
    /// Serde helper: keep the field out of existing JSON when it is the
    /// classic `Pin` form, so pre-module snapshots are byte-identical.
    #[must_use]
    pub fn is_pin(&self) -> bool {
        matches!(self, Self::Pin)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointAst {
    /// Component refdes (`Pin`), port name (`Port`), or net name
    /// (`Net`), depending on `ref_kind`.
    pub component: String,
    /// Pin name (`Pin`), the port name again (`Port`), or empty (`Net`).
    pub pin: String,
    #[serde(default, skip_serializing_if = "EndpointRefKind::is_pin")]
    pub ref_kind: EndpointRefKind,
    pub span: Span,
}

impl EndpointAst {
    /// A classic `component.pin` endpoint.
    #[must_use]
    pub fn pin(component: impl Into<String>, pin: impl Into<String>, span: Span) -> Self {
        Self {
            component: component.into(),
            pin: pin.into(),
            ref_kind: EndpointRefKind::Pin,
            span,
        }
    }

    /// A bare module-port reference (`-> vdd`).
    #[must_use]
    pub fn port(name: impl Into<String>, span: Span) -> Self {
        let name = name.into();
        Self {
            pin: name.clone(),
            component: name,
            ref_kind: EndpointRefKind::Port,
            span,
        }
    }

    /// A quoted net reference (`-> "I2C0.sda"`).
    #[must_use]
    pub fn net(name: impl Into<String>, span: Span) -> Self {
        Self {
            component: name.into(),
            pin: String::new(),
            ref_kind: EndpointRefKind::Net,
            span,
        }
    }
}

/// A reusable, parameterised block: `module "N" (port: type, …) { … }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDeclStmt {
    pub name: String,
    pub ports: Vec<PortDeclAst>,
    pub params: Vec<ParamDeclAst>,
    pub statements: Vec<StatementAst>,
    pub span: Span,
}

/// One module/interface port: `vdd: power`, `i2c: I2C`, `alert: output`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortDeclAst {
    pub name: String,
    /// Declared type: a direction keyword (`input`, `output`,
    /// `power`, `ground`, `bidirectional`) or an interface name.
    pub ty: String,
    pub span: Span,
}

/// One module parameter: `param r_pull: resistance = 4.7kohm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamDeclAst {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<ValueWithUnit>,
    pub span: Span,
}

/// An interface bundle type: `interface "I2C" (sda: i2c_sda, scl: i2c_scl)`.
///
/// Declaring a bus (`bus "I2C0" (sda, scl)`) gives the bundle a concrete
/// set of nets; binding an interface to that bus in one clause connects
/// every member by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceDeclStmt {
    pub name: String,
    pub members: Vec<PortDeclAst>,
    pub span: Span,
}

/// A named group of nets: `bus "I2C0" (sda, scl)`.
///
/// Members are addressed as `<bus>.<member>` (for example `I2C0.sda`) and
/// exported to KiCad as a bus with a matching `bus_alias`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusDeclStmt {
    pub name: String,
    pub members: Vec<String>,
    pub span: Span,
}

/// One port binding inside a `use` block (`vdd -> "3V3"`) or one
/// member binding inside a `bind` block (`sda -> U9.gpio0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortBindingAst {
    /// Port/member name, optionally qualified for an interface member
    /// (`i2c` or `i2c.sda`).
    pub port: String,
    /// Where the port goes. A `Net` target names a net (a bus name means
    /// "bind each member of the bundle to `<bus>.<member>`"); a `Pin`
    /// target wires the port straight to a component pin.
    pub target: EndpointAst,
    pub span: Span,
}

/// Instantiate a module: `use "Sensor" as CH1 (prefix "CH1_") { … }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseStmt {
    pub module: String,
    /// Instance label; also the default refdes prefix and the emitted
    /// sheet name.
    pub label: String,
    /// Explicit refdes prefix; defaults to `<label>_`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Parameter overrides (`name = value`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(String, ValueWithUnit)>,
    pub bindings: Vec<PortBindingAst>,
    pub span: Span,
}

/// Connect a bus's members to endpoints in one clause:
/// `bind "I2C0" : I2C { sda -> U9.gpio0 }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindStmt {
    /// Bus being bound (must be declared).
    pub bus: String,
    /// Optional interface type name to check membership against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    pub connections: Vec<PortBindingAst>,
    pub span: Span,
}

/// A named net: `net "+3V3" { U1.vout, C3.p1, U2.vdd }`.
///
/// All listed endpoints merge into one net carrying `name`. The same
/// name may appear on several `net` blocks and `connect … as "NAME"`
/// lines — they all merge. An optional `class "PWR"` (either after
/// the name or as a line inside the body) joins the net to a
/// declared netclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetDeclAst {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netclass: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<EndpointAst>,
    pub span: Span,
}

/// A power rail with a declared voltage: `power "+3V3" 3.3v`.
///
/// A power declaration is a named net with a nominal voltage. It may
/// carry an optional endpoint body (`power "+3V3" 3.3v { U1.vout }`)
/// and an optional `class "PWR"` join, exactly like [`NetDeclAst`].
/// A bare declaration (no endpoints) still materializes the named
/// net so later `connect … as "+3V3"` lines join it and power-domain
/// inference sees the declared voltage instead of guessing from pin
/// names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerDeclAst {
    pub name: String,
    pub voltage: ValueWithUnit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netclass: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<EndpointAst>,
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

/// A named routing-constraint class: `netclass "PWR" { trace_width 0.5mm clearance 0.2mm }`.
///
/// Nets join the class with `class "PWR"` on a `net`, `power`, or
/// `connect` statement, and the PCB exporter emits one KiCad
/// `net_class` per declared class carrying its member nets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetclassStmt {
    pub name: String,
    pub attrs: Vec<NetclassAttr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum NetclassAttr {
    TraceWidth(ValueWithUnit),
    Clearance(ValueWithUnit),
    /// Explicit hue for the class (`color "#c2410c"`), applied to the
    /// schematic `net_settings` colour. Six-digit `#rrggbb` (the
    /// leading `#` optional).
    Color(String),
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
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            span: Span::new(0, 0),
        });
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["stmt"], "component");
        assert_eq!(v["kind"], "mcu");
        assert_eq!(v["refdes"], "U1");
    }
}
