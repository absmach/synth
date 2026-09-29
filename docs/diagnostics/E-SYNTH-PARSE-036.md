# E-SYNTH-PARSE-036 — unknown schematic overflow policy

**Severity:** error
**Stage:** parse

## What this means

A `schematic { overflow = … }` block named a policy that is not one of the two supported values. The value is matched case-insensitively and surrounding whitespace is ignored; `hierarchy`, `hierarchical` and `sheets` are all accepted spellings of the same policy.

## Minimal reproduction

```synth
board "x" {
  schematic { paper = "A4" overflow = "shrink" }
  component U1: regulator "ams1117_3v3"
}
```

## Suggested fix

```synth
schematic { paper = "A4" overflow = "grow" }
```

- `grow` (the default) walks the standard ladder — A4, A3, A2, A1, A0 — keeping the design on a single sheet for as long as a standard page can hold it, and only splits into a sheet-per-group hierarchy once the content exceeds A0.
- `hierarchy` treats the requested page as the page: the moment the content no longer fits, the design is split into a hierarchy instead of being drawn on a larger sheet. It is the right choice for a review sheet that must stay one standard size, and the wrong one when a reviewer would rather scroll one large drawing than click between sheets.

Omitting the setting is the same as `overflow = "grow"`.
