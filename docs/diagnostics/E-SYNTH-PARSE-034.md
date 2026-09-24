# E-SYNTH-PARSE-034 — invalid group attribute

**Severity:** error
**Stage:** parse

## What this means

A `group` header attribute is malformed — `color` or `title` without a quoted string, or `region` without an identifier.

## Minimal reproduction

```synth
board "x" {
  group "Power" color red {
    component U1: regulator "ams1117_3v3"
  }
}
```

## Suggested fix

`color` and `title` take a quoted string, `region` takes a quadrant identifier:

```synth
group "Power" color "#c2410c" region top_left title "3.3 V regulator" {
  ...
}
```
