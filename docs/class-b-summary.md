# Class-B public summary: proved versus expert

This is the short public-facing summary for the Class-B boards that exist in
the tree. It is not a qualification record.

## Buck (24 V to 3.3 V, LMR38010)

**Proved by Synth:** source/lowering, ERC, placement, routing completeness,
independent DRC, fabrication outputs, registry provenance.
**Still needs an expert:** thermal-via pattern, surge/transient protection,
converter stability and capacitor derating, switch-node/EMI, grounding and
power-good behaviour.

## USB-UART (CH342F + THVD1450)

**Proved by Synth:** source/lowering, ERC, routing, independent DRC, BOM and
position output on the recorded revision.
**Still needs an expert:** connector ESD/return path, RS-485 bus protection and
the LP5907 VIO load envelope (`E-SYNTH-POWER-010` currently blocks a clean
source gate).

## RTD analog front end

**Proved by Synth:** source/lowering, placement, routing on the recorded
revision.
**Still needs an expert:** ADC input settling/noise, reference drift, analog
ground join and the open FreeRouting route findings.

## BLE environment node (#366)

The BLE board is not in the tree yet, so there is no golden entry or summary
for it. #366 remains blocked on #365/#364 until that board imports and exports.
