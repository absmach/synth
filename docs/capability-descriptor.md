# Capability descriptor

Before you ask Synth to do something, you can ask it what it is able to do. That is what the capability descriptor is for: a single read-only report of the commands, compiler stages, language constructs, and fabrication limits this particular binary supports.

It never opens your design. You can run it in an empty directory.

## Step 1 — ask, in a form you can read

```bash
synth capability list
```

You get a plain table, starting with the commands this build supports:

```
COMMAND              AVAILABLE
-------------------- ---------
validate             yes
export-kicad         yes
route                yes
check                yes
...
```

This is the quickest way to answer "is this feature in the build I actually have installed?" without reading release notes.

## Step 2 — ask, in a form a program can read

```bash
synth capability list --json
```

Same facts, emitted as a stable descriptor. This is the form agents and UI integrations should consume, because the keys and the versioning rules below are a contract.

## Step 3 — find what you need in the output

The descriptor has ten top-level keys:

| Key                        | What it tells you                                                                           |
| -------------------------- | ------------------------------------------------------------------------------------------- |
| `schema_version`           | Which version of this contract you are reading.                                             |
| `compiler`                 | The `name` and `version` of the binary that answered.                                       |
| `commands`                 | Each CLI subcommand, and whether it is available.                                           |
| `physical_stages`          | The layout, placement, routing, DRC and export stages, each with its evidence.              |
| `manufacturer_profiles`    | The built-in fabrication limits, per profile.                                               |
| `language`                 | The statements, attributes and endpoint forms the parser will accept.                       |
| `geometry_and_constraints` | Board size, layer count, schematic page, `routing_constraints` and `fabrication_processes`. |
| `board_family_profiles`    | The shipped board-family mechanical outlines.                                               |
| `unsupported`              | Capabilities with no code path in Synth at all.                                             |
| `unverified`               | Declarations Synth accepts but never checks or uses.                                        |

## Step 4 — read the limits before you trust a silence

Those last two keys are the important ones, and the reason to prefer this descriptor over guesswork.

> [!WARNING]
> Silence in the rest of the descriptor is not a claim of support. If a capability is simply absent from `language` or `physical_stages`, that tells you nothing on its own. The `unsupported` and `unverified` arrays are where the limits are stated out loud.

The distinction between them matters when you are debugging why something you wrote had no effect:

- **`unsupported`** — no code path exists for it. Synth cannot do this.
- **`unverified`** — the language accepts the declaration, but no stage verifies or consumes it. Your design will compile and the line will be silently inert.

Each entry in either array has the same shape:

| Field        | Type             | Meaning                                                                                                        |
| ------------ | ---------------- | -------------------------------------------------------------------------------------------------------------- |
| `id`         | string           | A stable identifier, unique across both arrays.                                                                |
| `summary`    | string           | What is missing or unchecked, worded no more strongly than the code supports.                                  |
| `constraint` | string, optional | The `statement.attribute` pair from `routing_constraints` this entry qualifies, such as `diff_pair.impedance`. |

An entry moves out of its array when a stage starts honouring it. If you want to see the source of truth, they are the `UNSUPPORTED` and `UNVERIFIED` tables in `crates/synth-cli/src/main.rs`.

## Understanding `fabrication_processes`

A fabrication process is something a board cannot be built without. A design declares one with `requires <process>`.

The descriptor deliberately keeps two separate facts apart, because conflating them is how boards get sent to a fab that cannot build them:

| The question                                    | Where the answer lives         |
| ----------------------------------------------- | ------------------------------ |
| What does this board's geometry need?           | `processes`                    |
| What does this manufacturer tier actually sell? | `profiles_offering_via_in_pad` |
| What fires when the two disagree?               | `gate`                         |

> [!IMPORTANT]
> A requirement is never a permission on its own. `requires via_in_pad` states that your geometry depends on the process; it does not assert that your chosen tier offers it. The `gate` diagnostic is what fires when a board requires something its declared tier does not offer.

`override` is the escape hatch, and the descriptor names it for what it is: an operator taking responsibility for one panel, not an answer about whether the process is available.

## Versioning

`schema_version` is `MAJOR.MINOR`:

- Adding a key bumps the **minor** version.
- Renaming or removing a key bumps the **major** version.

So you can add keys and existing consumers keep working; a major bump is your signal to go and look at what moved.

| Version | What it added                                         |
| ------- | ----------------------------------------------------- |
| `1.0`   | The initial descriptor.                               |
| `1.1`   | `unsupported` and `unverified`.                       |
| `1.2`   | The `requires` statement and the tier that offers it. |

## If you are adding to the language

This list is kept honest by a test rather than by discipline. The unit test `every_language_constraint_is_supported_unsupported_or_unverified` reads the `netclass`, `diff_pair` and `keepout` attributes straight out of the language's AST enums, and fails when it finds one that is neither in `routing_constraints` nor named by a `constraint` field.

In practice this means: **if you add an attribute to one of those statements, you have to classify it here.** The test will stop you otherwise.

> [!NOTE]
> The component `placement_hint` constraint is not covered by that test, so adding to it will not be caught automatically.
