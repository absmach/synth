// SPDX-License-Identifier: Apache-2.0

//! Recursive-descent parser for SynthSpec.
//!
//! Consumes a stream of [`Token`]s from the lexer and produces a
//! [`ProgramAst`] plus zero or more [`Diagnostic`]s.
//!
//! Error recovery: on a syntax error inside the board body, the parser
//! emits a diagnostic, then skips tokens until it finds either `}` or
//! the start of a known statement (`layers`, `component`, `connect`,
//! `diff_pair`, `keepout`, `manufacturer`), at which point it resumes.
//! This keeps cascading errors bounded: one underlying mistake produces
//! at most one diagnostic per statement.

use synth_ast::{
    BoardAst, ComponentDeclAst, ConnectionAst, DiffPairAttr, DiffPairStmt, EndpointAst, GroupStmt,
    ImportAst, KeepoutAttr, KeepoutStmt, LayersStmt, ManufacturerStmt, PlacementHintAst,
    PlacementHintAttr, ProgramAst, RevisionStmt, StatementAst, ValueWithUnit,
};
use synth_diagnostics::{
    Diagnostic, DiagnosticBuilder, Location, Patch, PatchKind, Severity, Span,
};

use crate::token::{LexError, Token, TokenKind};

pub struct ParseResult {
    pub ast: Option<ProgramAst>,
    pub diagnostics: Vec<Diagnostic>,
}

impl std::fmt::Debug for ParseResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParseResult")
            .field("ast", &self.ast.is_some())
            .field(
                "diagnostic_codes",
                &self.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ParseResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity.is_blocking())
    }
}

pub fn parse(tokens: Vec<Token>, file: String) -> ParseResult {
    let mut p = Parser {
        tokens,
        pos: 0,
        file,
        diagnostics: Vec::new(),
    };
    p.surface_lexer_errors();
    let ast = p.parse_program();
    ParseResult {
        ast,
        diagnostics: p.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    file: String,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn surface_lexer_errors(&mut self) {
        // Lexer errors travel as Error tokens; convert them to diagnostics
        // up front. The parser will subsequently see them as Eof for the
        // purpose of synchronizing (handled by skip_error_tokens during the
        // main parse loop).
        let mut errs = Vec::new();
        for (i, t) in self.tokens.iter().enumerate() {
            if let TokenKind::Error(e) = &t.kind {
                errs.push((i, t.span, e.clone()));
            }
        }
        for (_, span, e) in errs {
            self.emit_lex_diagnostic(span, &e);
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn bump(&mut self) -> &Token {
        let i = self.pos;
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        &self.tokens[i]
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn skip_error_tokens(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Error(_)) {
            self.pos += 1;
        }
    }

    fn parse_program(&mut self) -> Option<ProgramAst> {
        self.skip_error_tokens();
        let prog_start = self.peek().span.byte_start;

        let mut imports = Vec::new();
        while matches!(self.peek_kind(), TokenKind::KwImport) {
            if let Some(imp) = self.parse_import() {
                imports.push(imp);
            }
            self.skip_error_tokens();
        }

        let board = self.parse_board()?;

        self.skip_error_tokens();
        if !self.at_eof() {
            let span = self.peek().span;
            // Find the end of the trailing content: byte_end of the
            // last non-Eof token in the stream. `last_offset()`
            // points at the end of the LAST CONSUMED token, which is
            // BEFORE `span.byte_start` here — using it would produce
            // a backwards Span and trip the debug assertion.
            let trailing_end = self
                .tokens
                .iter()
                .rev()
                .find(|t| !matches!(t.kind, TokenKind::Eof))
                .map_or(span.byte_end, |t| t.span.byte_end);
            self.emit(
                span,
                "E-SYNTH-PARSE-005",
                "unexpected trailing input after board",
                "end of file",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.7,
                    rationale: Some("remove trailing input".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::DeleteRange {
                        range: Span::new(span.byte_start, trailing_end),
                    },
                }),
            );
        }

        let prog_end = self.last_offset();
        Some(ProgramAst {
            imports,
            board,
            span: Span::new(prog_start, prog_end),
        })
    }

    fn parse_import(&mut self) -> Option<ImportAst> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `import`
        let path =
            self.expect_string("E-SYNTH-PARSE-014", "expected import path (quoted string)")?;
        let end = self.last_offset();
        Some(ImportAst {
            path,
            span: Span::new(start, end),
        })
    }

    fn parse_board(&mut self) -> Option<BoardAst> {
        let start = self.peek().span.byte_start;

        // `board` keyword.
        if !matches!(self.peek_kind(), TokenKind::KwBoard) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-001",
                "expected `board` keyword",
                "`board` to begin program",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.5,
                    rationale: Some("insert a minimal board declaration".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: self.peek().span.byte_start,
                        text: "board \"unnamed\" {}\n".into(),
                    },
                }),
            );
            return None;
        }
        self.bump();

        // Name.
        let name =
            self.expect_string("E-SYNTH-PARSE-002", "expected board name (quoted string)")?;

        // `{`.
        if !matches!(self.peek_kind(), TokenKind::LBrace) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-003",
                "expected `{` to open board body",
                "`{`",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.6,
                    rationale: Some("open the board body".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: self.peek().span.byte_start,
                        text: "{".into(),
                    },
                }),
            );
            return None;
        }
        self.bump();

        // Statements.
        let mut statements = Vec::new();
        loop {
            self.skip_error_tokens();
            match self.peek_kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                _ => {
                    if let Some(stmt) = self.parse_statement() {
                        statements.push(stmt);
                    } else {
                        self.synchronize();
                    }
                }
            }
        }

        // `}`.
        let close_span = self.peek().span;
        if !matches!(self.peek_kind(), TokenKind::RBrace) {
            self.emit(
                close_span,
                "E-SYNTH-PARSE-004",
                "expected `}` to close board body",
                "`}`",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.4,
                    rationale: Some("close the board body".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: close_span.byte_start,
                        text: "}".into(),
                    },
                }),
            );
            let end = self.last_offset();
            return Some(BoardAst {
                name,
                statements,
                span: Span::new(start, end),
            });
        }
        self.bump();

        let end = self.last_offset();
        Some(BoardAst {
            name,
            statements,
            span: Span::new(start, end),
        })
    }

    fn parse_statement(&mut self) -> Option<StatementAst> {
        match self.peek_kind() {
            TokenKind::KwLayers => self.parse_layers().map(StatementAst::Layers),
            TokenKind::KwManufacturer => self.parse_manufacturer().map(StatementAst::Manufacturer),
            TokenKind::KwRevision => self.parse_revision().map(StatementAst::Revision),
            TokenKind::KwComponent => self.parse_component().map(StatementAst::Component),
            TokenKind::KwConnect => self.parse_connection().map(StatementAst::Connection),
            TokenKind::KwDiffPair => self.parse_diff_pair().map(StatementAst::DiffPair),
            TokenKind::KwKeepout => self.parse_keepout().map(StatementAst::Keepout),
            TokenKind::KwGroup => self.parse_group().map(StatementAst::Group),
            _ => {
                self.emit(
                    self.peek().span,
                    "E-SYNTH-PARSE-011",
                    "expected statement keyword",
                    "one of: layers, manufacturer, revision, component, connect, diff_pair, keepout, group",
                    self.describe_current(),
                    None,
                );
                None
            }
        }
    }

    fn parse_layers(&mut self) -> Option<LayersStmt> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `layers`
        let span = self.peek().span;
        let TokenKind::IntLit(n) = *self.peek_kind() else {
            self.emit(
                span,
                "E-SYNTH-PARSE-017",
                "expected layer count (positive integer)",
                "a positive integer",
                self.describe_current(),
                None,
            );
            return None;
        };
        self.bump();
        if !(1..=64).contains(&n) {
            self.emit(
                span,
                "E-SYNTH-PARSE-018",
                "layer count out of range",
                "an integer in [1, 64]",
                format!("{n}"),
                None,
            );
            return None;
        }
        let end = self.last_offset();
        Some(LayersStmt {
            count: u32::try_from(n).unwrap(),
            span: Span::new(start, end),
        })
    }

    fn parse_manufacturer(&mut self) -> Option<ManufacturerStmt> {
        let start = self.peek().span.byte_start;
        self.bump();
        let name = self.expect_string(
            "E-SYNTH-PARSE-015",
            "expected manufacturer name (quoted string)",
        )?;
        let end = self.last_offset();
        Some(ManufacturerStmt {
            name,
            span: Span::new(start, end),
        })
    }

    fn parse_revision(&mut self) -> Option<RevisionStmt> {
        let start = self.peek().span.byte_start;
        self.bump();
        let rev =
            self.expect_string("E-SYNTH-PARSE-019", "expected revision tag (quoted string)")?;
        let end = self.last_offset();
        Some(RevisionStmt {
            rev,
            span: Span::new(start, end),
        })
    }

    fn parse_component(&mut self) -> Option<ComponentDeclAst> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `component`
        let refdes =
            self.expect_ident("E-SYNTH-PARSE-010", "expected component refdes identifier")?;
        if !matches!(self.peek_kind(), TokenKind::Colon) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-012",
                "expected `:` after refdes",
                "`:`",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.7,
                    rationale: Some("insert `:` separator".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: self.peek().span.byte_start,
                        text: ":".into(),
                    },
                }),
            );
            return None;
        }
        self.bump();
        let kind = self.expect_ident("E-SYNTH-PARSE-013", "expected component kind identifier")?;
        let part = self.expect_string(
            "E-SYNTH-PARSE-016",
            "expected part identifier (quoted string)",
        )?;
        let value = if matches!(self.peek_kind(), TokenKind::KwValue) {
            self.bump(); // consume `value`
            Some(self.expect_string(
                "E-SYNTH-PARSE-027",
                "expected component value after `value`",
            )?)
        } else {
            None
        };
        let placement_hint = if matches!(self.peek_kind(), TokenKind::KwPlacementHint) {
            self.parse_placement_hint()
        } else if matches!(self.peek_kind(), TokenKind::LBrace) {
            self.bump(); // consume `{`
            let mut hint = None;
            loop {
                self.skip_error_tokens();
                match self.peek_kind() {
                    TokenKind::RBrace | TokenKind::Eof => break,
                    TokenKind::KwPlacementHint => {
                        hint = self.parse_placement_hint();
                    }
                    _ => {
                        self.emit(
                            self.peek().span,
                            "E-SYNTH-PARSE-029",
                            "unexpected keyword inside component body",
                            "`placement_hint`",
                            self.describe_current(),
                            None,
                        );
                        self.bump();
                    }
                }
            }
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                self.bump();
            }
            hint
        } else {
            None
        };
        let end = self.last_offset();
        Some(ComponentDeclAst {
            refdes,
            kind,
            part: Some(part),
            value,
            placement_hint,
            span: Span::new(start, end),
        })
    }

    #[allow(clippy::too_many_lines)]
    fn parse_placement_hint(&mut self) -> Option<PlacementHintAst> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `placement_hint`
        if !matches!(self.peek_kind(), TokenKind::LBrace) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-003",
                "expected `{` to open placement_hint body",
                "`{`",
                self.describe_current(),
                None,
            );
            return None;
        }
        self.bump(); // consume `{`
        let mut attrs = Vec::new();
        loop {
            self.skip_error_tokens();
            match self.peek_kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                TokenKind::KwRegion => {
                    if let Some((v, sp)) = self.parse_placement_ident_attr("region") {
                        attrs.push(PlacementHintAttr::Region(v, sp));
                    }
                }
                TokenKind::KwEdge => {
                    if let Some((v, sp)) = self.parse_placement_ident_attr("edge") {
                        attrs.push(PlacementHintAttr::Edge(v, sp));
                    }
                }
                TokenKind::KwNear => {
                    self.bump();
                    if matches!(self.peek_kind(), TokenKind::Colon) {
                        self.bump();
                    }
                    let val_span = self.peek().span;
                    let v = match self.peek_kind() {
                        TokenKind::StringLit(_) => self.expect_string(
                            "E-SYNTH-PARSE-028",
                            "expected near component identifier",
                        ),
                        _ => self.expect_ident(
                            "E-SYNTH-PARSE-028",
                            "expected near component identifier",
                        ),
                    };
                    if let Some(v) = v {
                        attrs.push(PlacementHintAttr::Near(v, val_span));
                    }
                }
                TokenKind::KwSide => {
                    if let Some((v, sp)) = self.parse_placement_ident_attr("side") {
                        attrs.push(PlacementHintAttr::Side(v, sp));
                    }
                }
                TokenKind::KwPriority => {
                    if let Some((v, sp)) = self.parse_placement_ident_attr("priority") {
                        attrs.push(PlacementHintAttr::Priority(v, sp));
                    }
                }
                _ => {
                    self.emit(
                        self.peek().span,
                        "E-SYNTH-PARSE-030",
                        "unexpected attribute inside placement_hint",
                        "one of: region, edge, near, side, priority",
                        self.describe_current(),
                        None,
                    );
                    self.bump();
                }
            }
        }
        if matches!(self.peek_kind(), TokenKind::RBrace) {
            self.bump();
        }
        let end = self.last_offset();
        Some(PlacementHintAst {
            attrs,
            span: Span::new(start, end),
        })
    }

    /// Shared body of the four ident-valued placement_hint attributes
    /// (`region`, `edge`, `side`, `priority`): consume the keyword,
    /// an optional `:`, then one identifier. `what` names the
    /// attribute for the diagnostic message.
    fn parse_placement_ident_attr(&mut self, what: &str) -> Option<(String, Span)> {
        self.bump(); // consume the attribute keyword
        if matches!(self.peek_kind(), TokenKind::Colon) {
            self.bump();
        }
        let val_span = self.peek().span;
        self.expect_ident("E-SYNTH-PARSE-028", &format!("expected {what} identifier"))
            .map(|v| (v, val_span))
    }

    fn parse_connection(&mut self) -> Option<ConnectionAst> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `connect`
        let from = self.parse_endpoint()?;
        if !matches!(self.peek_kind(), TokenKind::Arrow) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-008",
                "expected `->` between endpoints",
                "`->`",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.7,
                    rationale: Some("insert `->`".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: self.peek().span.byte_start,
                        text: " -> ".into(),
                    },
                }),
            );
            return None;
        }
        self.bump();
        let to = self.parse_endpoint()?;
        let end = self.last_offset();
        Some(ConnectionAst {
            from,
            to,
            span: Span::new(start, end),
        })
    }

    fn parse_endpoint(&mut self) -> Option<EndpointAst> {
        let start = self.peek().span.byte_start;
        let component = self.expect_ident("E-SYNTH-PARSE-010", "expected component identifier")?;
        if !matches!(self.peek_kind(), TokenKind::Dot) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-007",
                "expected `.` between component and pin",
                "`.`",
                self.describe_current(),
                None,
            );
            return None;
        }
        self.bump();
        let pin = self.expect_ident("E-SYNTH-PARSE-010", "expected pin identifier")?;
        let end = self.last_offset();
        Some(EndpointAst {
            component,
            pin,
            span: Span::new(start, end),
        })
    }

    fn parse_diff_pair(&mut self) -> Option<DiffPairStmt> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `diff_pair`
        let pos = self.expect_ident("E-SYNTH-PARSE-010", "expected positive net identifier")?;
        let neg = self.expect_ident("E-SYNTH-PARSE-010", "expected negative net identifier")?;
        if !matches!(self.peek_kind(), TokenKind::LBrace) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-003",
                "expected `{` to open diff_pair body",
                "`{`",
                self.describe_current(),
                None,
            );
            return None;
        }
        self.bump();
        let mut attrs = Vec::new();
        loop {
            self.skip_error_tokens();
            match self.peek_kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                TokenKind::KwImpedance => {
                    self.bump();
                    if let Some(v) = self.expect_value() {
                        attrs.push(DiffPairAttr::Impedance(v));
                    }
                }
                _ => {
                    self.emit(
                        self.peek().span,
                        "E-SYNTH-PARSE-019",
                        "unexpected attribute inside diff_pair",
                        "`impedance <value><unit>`",
                        self.describe_current(),
                        None,
                    );
                    self.bump();
                }
            }
        }
        if matches!(self.peek_kind(), TokenKind::RBrace) {
            self.bump();
        }
        let end = self.last_offset();
        Some(DiffPairStmt {
            pos,
            neg,
            attrs,
            span: Span::new(start, end),
        })
    }

    /// `group "<name>" { <statement>* }` — a named sub-circuit.
    ///
    /// The body accepts the same statements a board body does, parsed
    /// by the same `parse_statement`, so a group nests (and a stray
    /// `layers` inside one is a lowering concern, not a parse error).
    fn parse_group(&mut self) -> Option<GroupStmt> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `group`
        let name =
            self.expect_string("E-SYNTH-PARSE-002", "expected group name (quoted string)")?;
        if !matches!(self.peek_kind(), TokenKind::LBrace) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-003",
                "expected `{` to open group body",
                "`{`",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.6,
                    rationale: Some("open the group body".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: self.peek().span.byte_start,
                        text: "{".into(),
                    },
                }),
            );
            return None;
        }
        self.bump();

        let mut statements = Vec::new();
        loop {
            self.skip_error_tokens();
            match self.peek_kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                _ => {
                    if let Some(stmt) = self.parse_statement() {
                        statements.push(stmt);
                    } else {
                        self.synchronize();
                    }
                }
            }
        }

        if matches!(self.peek_kind(), TokenKind::RBrace) {
            self.bump();
        } else {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-004",
                "expected `}` to close group body",
                "`}`",
                self.describe_current(),
                Some(Patch {
                    confidence: 0.4,
                    rationale: Some("close the group body".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: self.peek().span.byte_start,
                        text: "}".into(),
                    },
                }),
            );
        }

        let end = self.last_offset();
        Some(GroupStmt {
            name,
            statements,
            span: Span::new(start, end),
        })
    }

    fn parse_keepout(&mut self) -> Option<KeepoutStmt> {
        let start = self.peek().span.byte_start;
        self.bump(); // consume `keepout`
        let name = self.expect_ident("E-SYNTH-PARSE-010", "expected keepout name identifier")?;
        if !matches!(self.peek_kind(), TokenKind::LBrace) {
            self.emit(
                self.peek().span,
                "E-SYNTH-PARSE-003",
                "expected `{` to open keepout body",
                "`{`",
                self.describe_current(),
                None,
            );
            return None;
        }
        self.bump();
        let mut attrs = Vec::new();
        loop {
            self.skip_error_tokens();
            match self.peek_kind() {
                TokenKind::RBrace | TokenKind::Eof => break,
                TokenKind::KwRadius => {
                    self.bump();
                    if let Some(v) = self.expect_value() {
                        attrs.push(KeepoutAttr::Radius(v));
                    }
                }
                _ => {
                    self.emit(
                        self.peek().span,
                        "E-SYNTH-PARSE-020",
                        "unexpected attribute inside keepout",
                        "`radius <value><unit>`",
                        self.describe_current(),
                        None,
                    );
                    self.bump();
                }
            }
        }
        if matches!(self.peek_kind(), TokenKind::RBrace) {
            self.bump();
        }
        let end = self.last_offset();
        Some(KeepoutStmt {
            name,
            attrs,
            span: Span::new(start, end),
        })
    }

    fn expect_ident(&mut self, code: &str, title: &str) -> Option<String> {
        let span = self.peek().span;
        if let TokenKind::Ident(_) = self.peek_kind() {
            let TokenKind::Ident(s) = self.bump().kind.clone() else {
                unreachable!()
            };
            Some(s)
        } else {
            self.emit(
                span,
                code,
                title,
                "an identifier",
                self.describe_current(),
                None,
            );
            None
        }
    }

    fn expect_string(&mut self, code: &str, title: &str) -> Option<String> {
        let span = self.peek().span;
        if let TokenKind::StringLit(_) = self.peek_kind() {
            let TokenKind::StringLit(s) = self.bump().kind.clone() else {
                unreachable!()
            };
            Some(s)
        } else {
            self.emit(
                span,
                code,
                title,
                "a quoted string",
                self.describe_current(),
                None,
            );
            None
        }
    }

    fn expect_value(&mut self) -> Option<ValueWithUnit> {
        let tok_span = self.peek().span;
        if let TokenKind::Value { literal, unit } = self.peek_kind() {
            let v = ValueWithUnit {
                literal: literal.clone(),
                unit: *unit,
                span: tok_span,
            };
            self.bump();
            Some(v)
        } else {
            self.emit(
                tok_span,
                "E-SYNTH-PARSE-026",
                "expected number with unit",
                "a number followed by a unit (e.g., `20mm`, `90ohm`)",
                self.describe_current(),
                None,
            );
            None
        }
    }

    /// On parse error, advance to the next safe recovery point so that
    /// one syntactic mistake doesn't poison the rest of the parse.
    fn synchronize(&mut self) {
        // First, always consume at least one token so we make progress.
        self.bump();
        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::RBrace
                | TokenKind::KwLayers
                | TokenKind::KwManufacturer
                | TokenKind::KwComponent
                | TokenKind::KwConnect
                | TokenKind::KwDiffPair
                | TokenKind::KwKeepout
                | TokenKind::KwGroup
                | TokenKind::KwPlacementHint => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn last_offset(&self) -> u32 {
        if self.pos == 0 {
            return 0;
        }
        self.tokens[self.pos - 1].span.byte_end
    }

    fn describe_current(&self) -> String {
        match self.peek_kind() {
            TokenKind::Eof => "end of file".to_string(),
            TokenKind::LBrace => "`{`".to_string(),
            TokenKind::RBrace => "`}`".to_string(),
            TokenKind::Colon => "`:`".to_string(),
            TokenKind::Dot => "`.`".to_string(),
            TokenKind::Arrow => "`->`".to_string(),
            TokenKind::KwBoard => "`board`".to_string(),
            TokenKind::KwImport => "`import`".to_string(),
            TokenKind::KwLayers => "`layers`".to_string(),
            TokenKind::KwManufacturer => "`manufacturer`".to_string(),
            TokenKind::KwRevision => "`revision`".to_string(),
            TokenKind::KwComponent => "`component`".to_string(),
            TokenKind::KwConnect => "`connect`".to_string(),
            TokenKind::KwDiffPair => "`diff_pair`".to_string(),
            TokenKind::KwKeepout => "`keepout`".to_string(),
            TokenKind::KwGroup => "`group`".to_string(),
            TokenKind::KwImpedance => "`impedance`".to_string(),
            TokenKind::KwRadius => "`radius`".to_string(),
            TokenKind::KwValue => "`value`".to_string(),
            TokenKind::KwPlacementHint => "`placement_hint`".to_string(),
            TokenKind::KwRegion => "`region`".to_string(),
            TokenKind::KwEdge => "`edge`".to_string(),
            TokenKind::KwNear => "`near`".to_string(),
            TokenKind::KwSide => "`side`".to_string(),
            TokenKind::KwPriority => "`priority`".to_string(),
            TokenKind::Ident(s) => format!("identifier `{s}`"),
            TokenKind::StringLit(_) => "a string literal".to_string(),
            TokenKind::IntLit(n) => format!("integer `{n}`"),
            TokenKind::Value { literal, unit } => format!("`{literal}{}`", unit.as_str()),
            TokenKind::Error(_) => "an unrecognized token".to_string(),
        }
    }

    fn emit(
        &mut self,
        span: Span,
        code: &str,
        title: &str,
        expected: &str,
        found: String,
        fix: Option<Patch>,
    ) {
        let mut b = DiagnosticBuilder::new(code, Severity::Error, title)
            .location(Location::from_span(self.file.clone(), span))
            .expected(expected)
            .found(found)
            .explanation_url(format!("synth.docs/diagnostics/{code}"));
        if let Some(p) = fix {
            b = b.suggested_fix(p);
        }
        self.diagnostics.push(b.build());
    }

    fn emit_lex_diagnostic(&mut self, span: Span, e: &LexError) {
        let (code, title, expected, found, fix) = match e {
            LexError::UnterminatedString => (
                "E-SYNTH-PARSE-021",
                "unterminated string literal",
                "a closing `\"`",
                "end of line or end of file".to_string(),
                Some(Patch {
                    confidence: 0.4,
                    rationale: Some("close the string".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: span.byte_end,
                        text: "\"".into(),
                    },
                }),
            ),
            LexError::InvalidEscape(c) => (
                "E-SYNTH-PARSE-022",
                "invalid escape sequence in string",
                "one of `\\\\`, `\\\"`, `\\n`, `\\t`, `\\r`",
                format!("`\\{c}`"),
                None,
            ),
            LexError::UnknownUnit(u) => (
                "E-SYNTH-PARSE-023",
                "unknown engineering unit",
                "one of: mm, mil, ohm, kohm, mohm, v, mv, a, ma, mhz, ghz, pf, nf, uf",
                format!("`{u}`"),
                None,
            ),
            LexError::InvalidNumber(s) => (
                "E-SYNTH-PARSE-024",
                "invalid number literal",
                "an integer or decimal",
                format!("`{s}`"),
                None,
            ),
            LexError::UnexpectedChar(c) => (
                "E-SYNTH-PARSE-006",
                "unexpected character",
                "a valid token",
                format!("`{c}`"),
                Some(Patch {
                    confidence: 0.5,
                    rationale: Some("remove the unexpected character".into()),
                    patch_consequence_preview: None,
                    kind: PatchKind::DeleteRange { range: span },
                }),
            ),
            LexError::UnterminatedBlockComment => (
                "E-SYNTH-PARSE-025",
                "unterminated block comment",
                "a closing `*/`",
                "end of file".to_string(),
                None,
            ),
        };
        let mut b = DiagnosticBuilder::new(code, Severity::Error, title)
            .location(Location::from_span(self.file.clone(), span))
            .expected(expected)
            .found(found)
            .explanation_url(format!("synth.docs/diagnostics/{code}"));
        if let Some(p) = fix {
            b = b.suggested_fix(p);
        }
        self.diagnostics.push(b.build());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    #[test]
    fn parse_component_with_placement_hint() {
        let src = r#"board "b" {
            component U1: mcu "stm32h743" {
                placement_hint { region: top_left  priority: hard }
            }
        }"#;
        let tokens = lex(src);
        let res = parse(tokens, "test.synth".into());
        assert!(
            res.ast.is_some(),
            "AST should be parsed: {:?}",
            res.diagnostics
        );
        assert!(
            res.diagnostics.is_empty(),
            "Diagnostics should be empty: {:?}",
            res.diagnostics
        );
        let ast = res.ast.unwrap();
        let stmt = &ast.board.statements[0];
        let StatementAst::Component(c) = stmt else {
            panic!("Expected component statement")
        };
        assert!(c.placement_hint.is_some());
        let hint = c.placement_hint.as_ref().unwrap();
        assert_eq!(hint.attrs.len(), 2);
    }

    #[test]
    fn parse_revision_statement() {
        let src = r#"board "b" {
            revision "A"
        }"#;
        let tokens = lex(src);
        let res = parse(tokens, "test.synth".into());
        assert!(
            res.diagnostics.is_empty(),
            "Diagnostics should be empty: {:?}",
            res.diagnostics
        );
        let ast = res.ast.unwrap();
        let stmt = &ast.board.statements[0];
        let StatementAst::Revision(r) = stmt else {
            panic!("Expected revision statement")
        };
        assert_eq!(r.rev, "A");
    }
}
