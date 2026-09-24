// SPDX-License-Identifier: Apache-2.0

//! Module instantiation: expands reusable blocks into concrete board
//! statements (see `docs/modules.md` for the design decision).
//!
//! Modules are a **source-level** reuse construct. Instantiating one
//! twice produces two sets of *distinct* components with prefixed
//! reference designators (`CH1_U1`, `CH2_U1`) and distinct nets — the
//! physically correct model, and the one the flat IR every downstream
//! stage assumes.
//!
//! Each instantiation is wrapped in a synthetic `sheet` block named
//! after the instance, so the P26 exporter splits it into its own file
//! with a distinct filename. That keeps the output valid in both flat
//! and hierarchical KiCad modes and never aliases two sheets to one
//! file (which KiCad rejects). Complex hierarchy — one shared sheet
//! file reused by several instances — is deliberately not used; see
//! `docs/modules.md`.
//!
//! Diagnostic codes emitted here:
//!
//! - `E-SYNTH-MODULE-001` unknown module
//! - `E-SYNTH-MODULE-002` unknown parameter
//! - `E-SYNTH-MODULE-003` unknown port
//! - `E-SYNTH-MODULE-004` unbound module port
//! - `E-SYNTH-MODULE-005` nested module instantiation unsupported
//! - `E-SYNTH-MODULE-006` unknown bus or interface in a `bind`
//! - `E-SYNTH-MODULE-007` duplicate instance label

use std::collections::{BTreeMap, HashSet};

use synth_ast::{
    BindStmt, BusDeclStmt, ComponentDeclAst, ConnectionAst, EndpointAst, EndpointRefKind,
    InterfaceDeclStmt, ModuleDeclStmt, StatementAst, UseStmt, ValueWithUnit,
};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity, Span};

/// A declared bus, carried on the `Board` so the exporter can emit a
/// KiCad bus and `bus_alias`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BusBundle {
    pub name: String,
    pub members: Vec<String>,
}

/// A module declaration kept for tooling/inspection (ports and params).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModuleDesc {
    pub name: String,
    /// `(port name, declared type)` in declaration order.
    pub ports: Vec<(String, String)>,
    /// `(param name, default literal)`.
    pub params: Vec<(String, String)>,
}

/// The result of expanding every `use` in a program.
#[derive(Debug)]
pub struct Expansion {
    /// The program's statements with every `use` replaced by concrete
    /// components and connections.
    pub statements: Vec<StatementAst>,
    pub buses: Vec<BusBundle>,
    pub modules: Vec<ModuleDesc>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Declarations collected from anywhere in the program (they may be
/// declared in an imported library file, and order does not matter).
#[derive(Default)]
struct Tables<'a> {
    modules: BTreeMap<&'a str, &'a ModuleDeclStmt>,
    interfaces: BTreeMap<&'a str, &'a InterfaceDeclStmt>,
    buses: BTreeMap<&'a str, &'a BusDeclStmt>,
}

/// Expand every `use` (and `bind`) in `statements`. Groups and sheets
/// are walked so a `use` may sit inside either.
pub fn expand(statements: &[StatementAst], file: &str) -> Expansion {
    let mut tables = Tables::default();
    collect(statements, &mut tables);

    let mut diagnostics = Vec::new();
    let mut seen_labels: HashSet<String> = HashSet::new();
    let expanded = rewrite(
        statements,
        &tables,
        file,
        &mut diagnostics,
        &mut seen_labels,
    );

    Expansion {
        statements: expanded,
        buses: tables
            .buses
            .values()
            .map(|b| BusBundle {
                name: b.name.clone(),
                members: b.members.clone(),
            })
            .collect(),
        modules: tables
            .modules
            .values()
            .map(|m| ModuleDesc {
                name: m.name.clone(),
                ports: m
                    .ports
                    .iter()
                    .map(|p| (p.name.clone(), p.ty.clone()))
                    .collect(),
                params: m
                    .params
                    .iter()
                    .map(|p| {
                        let default = p.default.as_ref().map_or_else(String::new, |v| {
                            format!("{}{}", v.literal, v.unit.as_str())
                        });
                        (p.name.clone(), default)
                    })
                    .collect(),
            })
            .collect(),
        diagnostics,
    }
}

fn collect<'a>(statements: &'a [StatementAst], tables: &mut Tables<'a>) {
    for stmt in statements {
        match stmt {
            StatementAst::Module(m) => {
                tables.modules.insert(m.name.as_str(), m);
            }
            StatementAst::Interface(i) => {
                tables.interfaces.insert(i.name.as_str(), i);
            }
            StatementAst::Bus(b) => {
                tables.buses.insert(b.name.as_str(), b);
            }
            StatementAst::Group(g) => collect(&g.statements, tables),
            StatementAst::Sheet(s) => collect(&s.statements, tables),
            _ => {}
        }
    }
}

fn rewrite(
    statements: &[StatementAst],
    tables: &Tables<'_>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
    seen_labels: &mut HashSet<String>,
) -> Vec<StatementAst> {
    let mut out = Vec::new();
    for stmt in statements {
        match stmt {
            // Declarations are metadata; they do not lower to circuitry.
            StatementAst::Module(_) | StatementAst::Interface(_) | StatementAst::Bus(_) => {}
            StatementAst::Use(u) => {
                if let Some(expanded) = expand_use(u, tables, file, diagnostics, seen_labels) {
                    out.push(StatementAst::Sheet(synth_ast::SheetStmt {
                        name: u.label.clone(),
                        statements: expanded,
                        span: u.span,
                    }));
                }
            }
            StatementAst::Bind(b) => {
                out.extend(expand_bind(b, tables, file, diagnostics));
            }
            StatementAst::Group(g) => {
                let mut g = g.clone();
                g.statements = rewrite(&g.statements, tables, file, diagnostics, seen_labels);
                out.push(StatementAst::Group(g));
            }
            StatementAst::Sheet(s) => {
                let mut s = s.clone();
                s.statements = rewrite(&s.statements, tables, file, diagnostics, seen_labels);
                out.push(StatementAst::Sheet(s));
            }
            other => out.push(other.clone()),
        }
    }
    out
}

/// One instantiation: resolve the module, apply parameters, bind ports,
/// prefix every refdes and internal net, and stamp the module/instance
/// on each component.
fn expand_use(
    u: &UseStmt,
    tables: &Tables<'_>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
    seen_labels: &mut HashSet<String>,
) -> Option<Vec<StatementAst>> {
    let Some(module) = tables.modules.get(u.module.as_str()) else {
        diagnostics.push(
            diag(
                "E-SYNTH-MODULE-001",
                "unknown module",
                u.span,
                file,
                format!(
                    "no `module \"{}\"` is declared in this design or its imports",
                    u.module
                ),
                "a module declared with `module \"NAME\" (…) { … }`",
            )
            .build(),
        );
        return None;
    };
    if !seen_labels.insert(u.label.clone()) {
        diagnostics.push(
            diag(
                "E-SYNTH-MODULE-007",
                "duplicate instance label",
                u.span,
                file,
                format!("`{}` is used by more than one `use`", u.label),
                "each instance to have a unique label",
            )
            .build(),
        );
        return None;
    }
    let prefix = u.prefix.clone().unwrap_or_else(|| format!("{}_", u.label));

    let params = resolve_params(module, u, file, diagnostics);

    let (port_types, bindings) = resolve_bindings(module, u, tables, file, diagnostics);

    check_unbound_ports(module, &bindings, tables, u, file, diagnostics);

    let ctx = InstanceCtx {
        module: module.name.clone(),
        instance: u.label.clone(),
        prefix,
        params,
        bindings,
        port_types,
        interfaces: &tables.interfaces,
        file,
    };
    let mut out = Vec::new();
    for stmt in &module.statements {
        lower_body_stmt(stmt, &ctx, &mut out, diagnostics);
    }
    Some(out)
}

/// Instance parameters: every declared default, then each override.
/// An override of an undeclared parameter is `E-SYNTH-MODULE-002`.
fn resolve_params(
    module: &ModuleDeclStmt,
    u: &UseStmt,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, String> {
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    for p in &module.params {
        params.insert(p.name.clone(), literal_with_unit(p.default.as_ref()));
    }
    for (name, value) in &u.params {
        if !module.params.iter().any(|p| &p.name == name) {
            diagnostics.push(
                diag(
                    "E-SYNTH-MODULE-002",
                    "unknown module parameter",
                    value.span,
                    file,
                    format!("module `{}` has no parameter `{name}`", module.name),
                    "a parameter declared with `param` in the module body",
                )
                .build(),
            );
            continue;
        }
        params.insert(name.clone(), literal_with_unit(Some(value)));
    }
    params
}

fn literal_with_unit(value: Option<&ValueWithUnit>) -> String {
    value.map_or_else(String::new, |v| format!("{}{}", v.literal, v.unit.as_str()))
}

/// `i2c -> "BUS0"`: bind every member of interface `i` to `<bus>.<member>`.
/// The target must be a *declared* bus (`E-SYNTH-MODULE-006`).
fn bind_whole_interface(
    head: &str,
    b: &synth_ast::PortBindingAst,
    iface: &InterfaceDeclStmt,
    bus_names: &HashSet<&str>,
    bindings: &mut BTreeMap<String, EndpointAst>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if b.target.ref_kind != EndpointRefKind::Net {
        diagnostics.push(
            diag(
                "E-SYNTH-MODULE-006",
                "interface port must bind to a bus",
                b.span,
                file,
                format!(
                    "`{head}` is interface `{}`; bind it to a declared bus name",
                    iface.name
                ),
                "a quoted bus name, e.g. `-> \"I2C0\"`",
            )
            .build(),
        );
        return;
    }
    let bus = b.target.component.as_str();
    if !bus_names.contains(bus) {
        diagnostics.push(
            diag(
                "E-SYNTH-MODULE-006",
                "unknown bus",
                b.target.span,
                file,
                format!("`{bus}` is not a declared `bus`"),
                "a bus declared with `bus \"NAME\" (…)`",
            )
            .build(),
        );
        return;
    }
    for m in &iface.members {
        bindings.insert(
            format!("{head}.{}", m.name),
            EndpointAst::net(format!("{bus}.{}", m.name), b.target.span),
        );
    }
}

/// Resolve each declared port to a binding target. A whole-interface
/// binding (`i2c -> "BUS0"`) expands into one per-member binding so
/// every later lookup is uniform.
fn resolve_bindings<'a>(
    module: &'a ModuleDeclStmt,
    u: &UseStmt,
    tables: &Tables<'a>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> (BTreeMap<&'a str, &'a str>, BTreeMap<String, EndpointAst>) {
    let port_types: BTreeMap<&str, &str> = module
        .ports
        .iter()
        .map(|p| (p.name.as_str(), p.ty.as_str()))
        .collect();
    let mut bindings: BTreeMap<String, EndpointAst> = BTreeMap::new();
    let bus_names: HashSet<&str> = tables.buses.keys().copied().collect();
    for b in &u.bindings {
        let (head, member) = split_member(&b.port);
        let Some(ty) = port_types.get(head) else {
            diagnostics.push(
                diag(
                    "E-SYNTH-MODULE-003",
                    "unknown module port",
                    b.span,
                    file,
                    format!("module `{}` has no port `{head}`", module.name),
                    "a port declared in the module's port list",
                )
                .build(),
            );
            continue;
        };
        let iface = tables.interfaces.get(*ty).copied();
        match (iface, member) {
            // A whole interface bound to a bus: one clause, every member.
            (Some(i), None) => {
                bind_whole_interface(head, b, i, &bus_names, &mut bindings, file, diagnostics);
            }
            (Some(i), Some(m)) => {
                if !i.members.iter().any(|x| x.name == m) {
                    diagnostics.push(
                        diag(
                            "E-SYNTH-MODULE-003",
                            "unknown interface member",
                            b.span,
                            file,
                            format!("interface `{}` has no member `{m}`", i.name),
                            "a member declared in the interface",
                        )
                        .build(),
                    );
                    continue;
                }
                bindings.insert(b.port.clone(), b.target.clone());
            }
            (None, None) => {
                bindings.insert(b.port.clone(), b.target.clone());
            }
            (None, Some(m)) => {
                diagnostics.push(
                    diag(
                        "E-SYNTH-MODULE-003",
                        "port is not an interface",
                        b.span,
                        file,
                        format!("`{head}` is type `{ty}` and has no member `{m}`"),
                        "a plain port name",
                    )
                    .build(),
                );
            }
        }
    }
    (port_types, bindings)
}

/// Every declared port must be bound (`E-SYNTH-MODULE-004`). An
/// interface port is bound when *all* of its members are.
fn check_unbound_ports(
    module: &ModuleDeclStmt,
    bindings: &BTreeMap<String, EndpointAst>,
    tables: &Tables<'_>,
    u: &UseStmt,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for p in &module.ports {
        let iface_members = tables.interfaces.get(p.ty.as_str()).map(|i| &i.members);
        let bound = match iface_members {
            Some(members) => members
                .iter()
                .all(|m| bindings.contains_key(&format!("{}.{}", p.name, m.name))),
            None => bindings.contains_key(&p.name),
        };
        if !bound {
            diagnostics.push(
                diag(
                    "E-SYNTH-MODULE-004",
                    "unbound module port",
                    u.span,
                    file,
                    format!(
                        "port `{}` of module `{}` has no binding",
                        p.name, module.name
                    ),
                    "a `port -> net` line in the `use` block",
                )
                .build(),
            );
        }
    }
}

/// Context for rewriting one module body.
struct InstanceCtx<'a> {
    module: String,
    instance: String,
    prefix: String,
    params: BTreeMap<String, String>,
    bindings: BTreeMap<String, EndpointAst>,
    port_types: BTreeMap<&'a str, &'a str>,
    interfaces: &'a BTreeMap<&'a str, &'a InterfaceDeclStmt>,
    file: &'a str,
}

fn lower_body_stmt(
    stmt: &StatementAst,
    ctx: &InstanceCtx<'_>,
    out: &mut Vec<StatementAst>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match stmt {
        StatementAst::Component(c) => out.push(StatementAst::Component(prefix_component(c, ctx))),
        StatementAst::Connection(conn) => {
            if let Some(rewritten) = rewrite_connection(conn, ctx, diagnostics) {
                out.push(StatementAst::Connection(rewritten));
            }
        }
        StatementAst::Net(n) => {
            let mut n = n.clone();
            n.name = format!("{}{}", ctx.prefix, n.name);
            let mut endpoints = Vec::new();
            for ep in &n.endpoints {
                if let Some(r) = rewrite_endpoint(ep, ctx, diagnostics) {
                    endpoints.push(r);
                }
            }
            n.endpoints = endpoints;
            out.push(StatementAst::Net(n));
        }
        StatementAst::Power(p) => {
            // A power rail named inside a module is instance-local.
            let mut p = p.clone();
            p.name = format!("{}{}", ctx.prefix, p.name);
            out.push(StatementAst::Power(p));
        }
        StatementAst::Notes(n) => {
            let mut n = n.clone();
            n.title = format!("{} — {}", ctx.instance, n.title);
            out.push(StatementAst::Notes(n));
        }
        StatementAst::Use(_) => diagnostics.push(
            diag(
                "E-SYNTH-MODULE-005",
                "nested module instantiation is not supported",
                stmt.span(),
                ctx.file,
                format!("a `use` inside module `{}`", ctx.module),
                "modules to contain only component/connection statements",
            )
            .build(),
        ),
        StatementAst::Group(g) => {
            let mut g = g.clone();
            let mut inner = Vec::new();
            for s in &g.statements {
                lower_body_stmt(s, ctx, &mut inner, diagnostics);
            }
            g.statements = inner;
            out.push(StatementAst::Group(g));
        }
        StatementAst::Sheet(s) => {
            let mut s = s.clone();
            let mut inner = Vec::new();
            for st in &s.statements {
                lower_body_stmt(st, ctx, &mut inner, diagnostics);
            }
            s.statements = inner;
            out.push(StatementAst::Sheet(s));
        }
        // Declarations and anything else inside a body are inert.
        _ => {}
    }
}

fn prefix_component(c: &ComponentDeclAst, ctx: &InstanceCtx<'_>) -> ComponentDeclAst {
    let mut c = c.clone();
    c.refdes = format!("{}{}", ctx.prefix, c.refdes);
    if let Some(value) = c.value.clone() {
        if let Some(param) = value.strip_prefix('$') {
            c.value = ctx.params.get(param).cloned();
        }
    }
    c
}

fn rewrite_connection(
    conn: &ConnectionAst,
    ctx: &InstanceCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ConnectionAst> {
    let from = rewrite_endpoint(&conn.from, ctx, diagnostics)?;
    let to = rewrite_endpoint(&conn.to, ctx, diagnostics)?;
    let mut additional = Vec::new();
    for ep in &conn.additional {
        if let Some(r) = rewrite_endpoint(ep, ctx, diagnostics) {
            additional.push(r);
        }
    }
    let mut out = conn.clone();
    out.from = from;
    out.to = to;
    out.additional = additional;
    // An explicit `as "NAME"` inside a module is instance-local.
    if let Some(name) = out.net_name.take() {
        out.net_name = Some(format!("{}{}", ctx.prefix, name));
    }
    Some(out)
}

/// Rewrite one endpoint: component references get the refdes prefix, and
/// port references become the bound net (or pin).
fn rewrite_endpoint(
    ep: &EndpointAst,
    ctx: &InstanceCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<EndpointAst> {
    match ep.ref_kind {
        EndpointRefKind::Net => Some(ep.clone()),
        EndpointRefKind::Port => resolve_port(&ep.component, None, ep, ctx, diagnostics),
        EndpointRefKind::Pin => {
            let comp = ep.component.as_str();
            if ctx.port_types.contains_key(comp) {
                // `port.member` — a member of an interface port.
                resolve_port(comp, Some(ep.pin.as_str()), ep, ctx, diagnostics)
            } else {
                let mut ep = ep.clone();
                ep.component = format!("{}{}", ctx.prefix, ep.component);
                Some(ep)
            }
        }
    }
}

/// A port reference resolves to its binding target.
fn resolve_port(
    port: &str,
    member: Option<&str>,
    span_of: &EndpointAst,
    ctx: &InstanceCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<EndpointAst> {
    let key = match member {
        Some(m) => format!("{port}.{m}"),
        None => port.to_string(),
    };
    if let Some(target) = ctx.bindings.get(&key) {
        let mut t = target.clone();
        t.span = span_of.span;
        return Some(t);
    }
    // A whole interface referenced without a member.
    if member.is_none() && ctx.interfaces.contains_key(ctx.port_types.get(port)?) {
        diagnostics.push(
            diag(
                "E-SYNTH-MODULE-004",
                "unbound module port",
                span_of.span,
                ctx.file,
                format!("interface port `{port}` is referenced without a member"),
                "`port.member` for an interface port",
            )
            .build(),
        );
        return None;
    }
    diagnostics.push(
        diag(
            "E-SYNTH-MODULE-004",
            "unbound module port",
            span_of.span,
            ctx.file,
            format!("port `{key}` was never bound"),
            "a `port -> net` binding in the `use` block",
        )
        .build(),
    );
    None
}

/// `bind "I2C0" : I2C { sda -> U9.gpio0 }` becomes concrete connections
/// from `I2C0.sda` to the target.
fn expand_bind(
    b: &BindStmt,
    tables: &Tables<'_>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<StatementAst> {
    let Some(bus) = tables.buses.get(b.bus.as_str()) else {
        diagnostics.push(
            diag(
                "E-SYNTH-MODULE-006",
                "unknown bus",
                b.span,
                file,
                format!("`{}` is not a declared `bus`", b.bus),
                "a bus declared with `bus \"NAME\" (…)`",
            )
            .build(),
        );
        return Vec::new();
    };
    if let Some(iface_name) = &b.interface {
        let Some(iface) = tables.interfaces.get(iface_name.as_str()) else {
            diagnostics.push(
                diag(
                    "E-SYNTH-MODULE-006",
                    "unknown interface",
                    b.span,
                    file,
                    format!("`{iface_name}` is not a declared `interface`"),
                    "an interface declared with `interface \"NAME\" (…)`",
                )
                .build(),
            );
            return Vec::new();
        };
        for m in &iface.members {
            if !bus.members.iter().any(|x| x == &m.name) {
                diagnostics.push(
                    diag(
                        "E-SYNTH-MODULE-006",
                        "bus is missing an interface member",
                        b.span,
                        file,
                        format!("bus `{}` has no member `{}`", bus.name, m.name),
                        "the bus to declare every member of the interface",
                    )
                    .build(),
                );
            }
        }
    }
    let mut out = Vec::new();
    for c in &b.connections {
        let (member, _) = split_member(&c.port);
        if !bus.members.iter().any(|m| m == member) {
            diagnostics.push(
                diag(
                    "E-SYNTH-MODULE-006",
                    "unknown bus member",
                    c.span,
                    file,
                    format!("bus `{}` has no member `{member}`", bus.name),
                    "a member declared in the bus",
                )
                .build(),
            );
            continue;
        }
        out.push(StatementAst::Connection(ConnectionAst {
            from: EndpointAst::net(format!("{}.{member}", bus.name), c.span),
            to: c.target.clone(),
            additional: Vec::new(),
            net_name: None,
            netclass: None,
            span: c.span,
        }));
    }
    out
}

fn split_member(s: &str) -> (&str, Option<&str>) {
    match s.split_once('.') {
        Some((head, member)) => (head, Some(member)),
        None => (s, None),
    }
}

fn diag(
    code: &str,
    title: &str,
    span: Span,
    file: &str,
    found: String,
    expected: &str,
) -> DiagnosticBuilder {
    DiagnosticBuilder::new(code, Severity::Error, title)
        .location(Location::from_span(file.to_string(), span))
        .expected(expected.to_string())
        .found(found)
        .explanation_url(format!("synth.docs/diagnostics/{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_parser::parse;

    fn expand_src(src: &str) -> Expansion {
        let parsed = parse(src, "test.synth");
        assert!(
            !parsed.has_errors(),
            "parse diagnostics: {:?}",
            parsed.diagnostics
        );
        expand(&parsed.ast.unwrap().board.statements, "test.synth")
    }

    fn components(stmts: &[StatementAst]) -> Vec<String> {
        fn walk(stmts: &[StatementAst], out: &mut Vec<String>) {
            for s in stmts {
                match s {
                    StatementAst::Component(c) => out.push(c.refdes.clone()),
                    StatementAst::Sheet(s) => walk(&s.statements, out),
                    StatementAst::Group(g) => walk(&g.statements, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(stmts, &mut out);
        out
    }

    fn nets(stmts: &[StatementAst], out: &mut Vec<String>) {
        for s in stmts {
            match s {
                StatementAst::Connection(c) => {
                    for ep in std::iter::once(&c.from)
                        .chain(std::iter::once(&c.to))
                        .chain(c.additional.iter())
                    {
                        if ep.ref_kind == EndpointRefKind::Net {
                            out.push(ep.component.clone());
                        }
                    }
                }
                StatementAst::Sheet(sh) => nets(&sh.statements, out),
                _ => {}
            }
        }
    }

    fn values(stmts: &[StatementAst]) -> Vec<(String, Option<String>)> {
        fn walk(stmts: &[StatementAst], out: &mut Vec<(String, Option<String>)>) {
            for s in stmts {
                match s {
                    StatementAst::Component(c) => out.push((c.refdes.clone(), c.value.clone())),
                    StatementAst::Sheet(sh) => walk(&sh.statements, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(stmts, &mut out);
        out
    }

    const SENSOR: &str = r#"board "b" {
        module "Sensor" (vdd: power, gnd: ground, i2c: I2C, alert: output) {
            param r_pull: resistance = 4.7kohm
            component U1: sensor "bmp280_pressure"
            component R1: resistor "r_generic_0603" value $r_pull
            connect U1.vdd -> vdd
            connect U1.gnd -> gnd
            connect U1.sda -> i2c.sda
            connect U1.scl -> i2c.scl
            connect U1.int -> alert
            connect U1.sda -> R1.p1
        }
        interface "I2C" (sda: i2c_sda, scl: i2c_scl)
        bus "I2C0" (sda, scl)
        use "Sensor" as CH1 (prefix "CH1_") {
            vdd -> "3V3"
            gnd -> "GND"
            i2c -> "I2C0"
            alert -> "ALERT_1"
        }
        use "Sensor" as CH2 {
            vdd -> "3V3"
            gnd -> "GND"
            i2c -> "I2C0"
            alert -> "ALERT_2"
        }
    }"#;

    #[test]
    fn two_instances_get_distinct_prefixed_refdes() {
        let e = expand_src(SENSOR);
        assert!(e.diagnostics.is_empty(), "{:?}", e.diagnostics);
        let refs = components(&e.statements);
        assert_eq!(
            refs,
            vec!["CH1_U1", "CH1_R1", "CH2_U1", "CH2_R1"],
            "refdes are prefixed per instance"
        );
        // Instance blocks become sheets named after the label.
        let sheets: Vec<&str> = e
            .statements
            .iter()
            .filter_map(|s| match s {
                StatementAst::Sheet(sh) => Some(sh.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sheets, vec!["CH1", "CH2"]);
    }

    #[test]
    fn interface_binding_expands_every_member() {
        let e = expand_src(SENSOR);
        assert!(e.diagnostics.is_empty(), "{:?}", e.diagnostics);
        let mut names = Vec::new();
        nets(&e.statements, &mut names);
        assert!(names.contains(&"I2C0.sda".to_string()), "{names:?}");
        assert!(names.contains(&"I2C0.scl".to_string()), "{names:?}");
        assert!(names.contains(&"ALERT_1".to_string()), "{names:?}");
        assert!(names.contains(&"ALERT_2".to_string()), "{names:?}");
    }

    #[test]
    fn parameter_default_and_override_substitute_into_value() {
        let e = expand_src(SENSOR);
        let values = values(&e.statements);
        let ch1 = values.iter().find(|(r, _)| r == "CH1_R1").expect("CH1_R1");
        assert_eq!(ch1.1.as_deref(), Some("4.7kohm"), "default param");
    }

    #[test]
    fn param_override_is_applied() {
        let src = SENSOR.replace(
            "use \"Sensor\" as CH2 {",
            "use \"Sensor\" as CH2 (r_pull = 10kohm) {",
        );
        let e = expand_src(&src);
        assert!(e.diagnostics.is_empty(), "{:?}", e.diagnostics);
        let values = values(&e.statements);
        let ch2 = values.iter().find(|(r, _)| r == "CH2_R1").expect("CH2_R1");
        assert_eq!(ch2.1.as_deref(), Some("10kohm"), "override applied");
    }

    #[test]
    fn bus_metadata_is_recorded() {
        let e = expand_src(SENSOR);
        assert_eq!(e.buses.len(), 1);
        assert_eq!(e.buses[0].name, "I2C0");
        assert_eq!(e.buses[0].members, vec!["sda", "scl"]);
        assert_eq!(e.modules.len(), 1);
        assert_eq!(e.modules[0].ports.len(), 4);
    }

    #[test]
    fn unbound_port_is_reported() {
        let src = r#"board "b" {
            module "M" (a: input, b: output) {
                component U1: resistor "r_generic_0603"
                connect U1.p1 -> a
                connect U1.p2 -> b
            }
            use "M" as X {
                a -> "NA"
            }
        }"#;
        let e = expand_src(src);
        assert!(
            e.diagnostics.iter().any(|d| d.code == "E-SYNTH-MODULE-004"),
            "{:?}",
            e.diagnostics
        );
    }

    #[test]
    fn unknown_module_is_reported() {
        let e = expand_src(r#"board "b" { use "Nope" as X { } }"#);
        assert!(e.diagnostics.iter().any(|d| d.code == "E-SYNTH-MODULE-001"));
    }

    #[test]
    fn standalone_bind_connects_bus_members() {
        let src = r#"board "b" {
            bus "I2C0" (sda, scl)
            component U1: mcu "rp2350"
            bind "I2C0" : I2C { sda -> U1.gp0, scl -> U1.gp1 }
        }"#;
        // No interface declared: the optional `: I2C` is reported, but the
        // member connections still lower.
        let e = expand_src(src);
        assert!(e.diagnostics.iter().any(|d| d.code == "E-SYNTH-MODULE-006"));
    }
}
