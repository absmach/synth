# E-SYNTH-MODULE-009 — recorded deviations from reference modules

**Severity:** info
**Stage:** erc — board

## What this means

The design departs from one or more reference modules, and says why. This
diagnostic lists every recorded deviation in one place, so a reviewer reads
the whole list of exceptions at once. Each line names the instance, the
module, the overridden parameter and its value (or "the instance" for an
instance-level deviation), and the reason.

```text
LDO1 (LdoAms1117): `c_out` = 47uf: polymer part with the ESR the data sheet asks for
USB (UsbCSinkEsd): the instance: CC clamps removed, the enclosure has no exposed port
```

`synth_review_schematic` also returns the list as `module_deviations`.

## Where deviations come from

- `param = value deviation "reason"` on a `use`, for any parameter. A
  `fixed` parameter must have one ([`E-SYNTH-MODULE-008`](E-SYNTH-MODULE-008.md)).
- A bare `deviation "reason"` in the `use` parameter list, for a change that
  is not one parameter, such as an edited topology.

## Suggested fix

Nothing to fix. Review each line. A design with no deviations gets no
diagnostic.
