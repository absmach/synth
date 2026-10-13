# Capability descriptor

`synth capability list` prints what the compiler offers; `synth capability list --json`
prints the same facts as a stable descriptor for agents and UI integrations. The command is
read-only and never inspects a design.

## Versioning

`schema_version` is `MAJOR.MINOR`. Adding a key bumps the minor version; renaming or removing
a key bumps the major version. Version `1.1` added `unsupported` and `unverified`.

## Top-level keys

| Key | Meaning |
| --- | --- |
| `schema_version` | Descriptor version. |
| `compiler` | `name` and `version` of the binary. |
| `commands` | CLI subcommands and whether each is available. |
| `physical_stages` | Layout, placement, routing, DRC and export stages with their evidence. |
| `manufacturer_profiles` | Built-in fabrication limits. |
| `language` | Statements, attributes and endpoint forms the parser accepts. |
| `geometry_and_constraints` | Board size, layer count, schematic page, `routing_constraints` and `fabrication_processes`. |
| `board_family_profiles` | Shipped board-family mechanical outlines. |
| `unsupported` | Capabilities with no code path in Synth. |
| `unverified` | Declarations Synth accepts but does not check or use. |

## `unsupported` and `unverified`

Silence in the rest of the descriptor is not a claim of support. These two sections name the
limits explicitly. Each is an array of entries:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Stable identifier, unique across both sections. |
| `summary` | string | What is missing or unchecked, worded no more strongly than the code supports. |
| `constraint` | string, optional | The `statement.attribute` pair from `routing_constraints` the entry qualifies, such as `diff_pair.impedance`. |

- `unsupported` means no code path exists for the capability.
- `unverified` means the language accepts the declaration but no stage verifies or consumes it.

## `fabrication_processes`

A process a board cannot be built without, declared with `requires <process>`.

Two facts, kept apart. The board declares what its geometry needs; the manufacturer profile
declares what the tier sells. `processes` names the first, `profiles_offering_via_in_pad` the
second, and `gate` the diagnostic that fires when a board requires what its declared tier does
not offer — so a requirement is never a permission on its own.

`override` names the escape hatch and says what it is: an operator taking responsibility for one
panel, not an answer about the process.

An entry moves out of its section when a stage starts honouring it. The entries are the
`UNSUPPORTED` and `UNVERIFIED` tables in `crates/synth-cli/src/main.rs`.

## Keeping the list in step with the language

The unit test `every_language_constraint_is_supported_unsupported_or_unverified` reads the
`netclass`, `diff_pair` and `keepout` attributes from the language's AST enums and fails when
one appears that is not in `routing_constraints` or named by a `constraint` field. Adding an
attribute to one of those statements therefore requires classifying it here. The component
`placement_hint` constraint is not covered by the test.
