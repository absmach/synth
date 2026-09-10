// SPDX-License-Identifier: Apache-2.0

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Byte offset into a source file. Authoritative; line/column are derived.
pub type ByteOffset = u32;

/// Half-open byte range `[start, end)` within a single source file.
///
/// Always carry the file id alongside (see [`Location`]); a `Span` alone is
/// only meaningful inside the parser's per-file context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub byte_start: ByteOffset,
    pub byte_end: ByteOffset,
}

impl Span {
    pub fn new(byte_start: ByteOffset, byte_end: ByteOffset) -> Self {
        debug_assert!(byte_start <= byte_end, "Span start must precede end");
        Self {
            byte_start,
            byte_end,
        }
    }

    pub fn len(self) -> u32 {
        self.byte_end.saturating_sub(self.byte_start)
    }

    pub fn is_empty(self) -> bool {
        self.byte_start == self.byte_end
    }

    /// Smallest span enclosing both `self` and `other`. Used by parsers
    /// when combining child spans into a parent node's span.
    pub fn join(self, other: Span) -> Span {
        Span {
            byte_start: self.byte_start.min(other.byte_start),
            byte_end: self.byte_end.max(other.byte_end),
        }
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(range: std::ops::Range<usize>) -> Self {
        Span::new(range.start as u32, range.end as u32)
    }
}

/// Interned file identifier. Allocated by the host (usually the CLI or
/// the parser entry point) when a source file is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct FileId(pub u32);

/// 1-indexed line and column within a file. Always derived from a
/// `(FileId, ByteOffset)` pair by the host — never authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// A fully-qualified location: file path + byte span + derived line/cols.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Location {
    pub file: String,
    #[serde(flatten)]
    pub span: Span,
    pub line_start: u32,
    pub col_start: u32,
    pub line_end: u32,
    pub col_end: u32,
}

impl Location {
    /// Construct a [`Location`] without computing line/column. Use when
    /// line/column are not yet known; they default to 0.
    pub fn from_span(file: impl Into<String>, span: Span) -> Self {
        Self {
            file: file.into(),
            span,
            line_start: 0,
            col_start: 0,
            line_end: 0,
            col_end: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_join() {
        let a = Span::new(10, 20);
        let b = Span::new(15, 30);
        assert_eq!(a.join(b), Span::new(10, 30));
        assert_eq!(b.join(a), Span::new(10, 30));
    }

    #[test]
    fn span_from_range() {
        let s: Span = (4..9_usize).into();
        assert_eq!(s, Span::new(4, 9));
    }

    #[test]
    fn location_serializes_with_flat_span() {
        let loc = Location {
            file: "board.synth".into(),
            span: Span::new(10, 20),
            line_start: 1,
            col_start: 11,
            line_end: 1,
            col_end: 21,
        };
        let v = serde_json::to_value(&loc).unwrap();
        assert_eq!(v["file"], "board.synth");
        assert_eq!(v["byte_start"], 10);
        assert_eq!(v["byte_end"], 20);
        assert_eq!(v["line_start"], 1);
    }
}
