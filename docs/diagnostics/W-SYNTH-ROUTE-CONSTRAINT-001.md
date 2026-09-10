# W-SYNTH-ROUTE-CONSTRAINT-001: Unknown Net Name in Route Constraints

## Diagnostic Summary

| Attribute | Value |
|---|---|
| **Code** | `W-SYNTH-ROUTE-CONSTRAINT-001` |
| **Severity** | Warning |
| **Category** | Routing / Constraints |
| **Phase** | MCP Tool Execution (`synth_route_with_constraints`) |

## Explanation

The `synth_route_with_constraints` MCP tool received a routing constraint specification for a net name that does not exist in the compiled board design. The unrecognized net constraint is safely ignored, and routing proceeds for all valid nets.

## Example

```json
{
  "net": "VCC_3V3_NONEXISTENT",
  "width_mm": 0.5,
  "clearance_mm": 0.25
}
```

## Remediation

1. Verify the exact net name spelling against the `.synth` source code or ERC report.
2. Use `synth_validate` or `synth_erc_report` to list all valid net names in the board.
3. Update the `net` property in `net_constraints` to match a valid net identifier.
