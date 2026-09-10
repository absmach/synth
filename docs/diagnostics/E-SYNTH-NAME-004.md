# E-SYNTH-NAME-004 — refdes letter does not match the component kind

**Severity:** warning
**Stage:** erc — naming

## What this means

The component's reference designator prefix is unconventional for its
declared kind. Reference-designator letters follow an IEEE-derived
industry convention (Sierra Circuits, "How to Draw and Design a PCB
Schematic", guideline 10): reviewers, datasheets, and fab/assembly
teams expect them, so a nonstandard letter slows every downstream
reader down.

| Kind                                           | Conventional prefix |
| ---------------------------------------------- | ------------------- |
| Resistor                                       | `R`                 |
| Capacitor                                      | `C`                 |
| Inductor / filter                              | `L` (`FL`)          |
| Diode / LED                                    | `D`                 |
| Transistor                                     | `Q`                 |
| Crystal                                        | `Y` (or `X`)        |
| Switch                                         | `SW`                |
| Relay                                          | `K`                 |
| Fuse                                           | `F`                 |
| Battery                                        | `BT`                |
| Antenna                                        | `E` (`AN`)          |
| Buzzer / speaker                               | `LS` (`BZ`)         |
| Connector                                      | `J` (`P`, `CON`)    |
| ICs (mcu, sensor, regulator, opamp, memory, …) | `U`                 |

Kinds without a well-known letter are never flagged.

## Minimal reproduction

```synth
board "x" {
  component X1: resistor "r_generic_0603"
  //            ^^^ resistors conventionally use R — NAME-004 fires.
}
```

## Suggested fix

Rename the refdes in the `.synth` source (`R1`, not `X1`). This is a
warning, not an error: the design is electrically identical, but
convention-following designators keep reviews and assembly docs
recognizable.

## See also

- `E-SYNTH-NAME-001` duplicate refdes · `E-SYNTH-NAME-002` empty ·
  `E-SYNTH-NAME-003` must start with a letter
- `docs/kicad-workflows.md` §2 — sourcing/BOM discipline
