# protected_power_entry rev A — design notes

Source of truth: `protected_power_entry.synth` (native Synth project) with the PCB placement sidecar
`protected_power_entry.placement.layout.toml`. KiCad files in `export/` are build artifacts.

**Read this first.** No datasheet could be opened in this session (the agent had only Synth MCP and
Read/Write/Edit). Every device limit below is marked **[recalled]**: it is from my memory of the
datasheet, not read and checked. A reviewer must confirm each [recalled] figure before fabrication.
The calculations are arithmetic on those figures and are only as good as they are.

Stated exception: this board has no MCU, no USB-C programming path and no SWD header. It is a power
stage; nothing on it is programmable. There is no firmware (see `BRINGUP.md`).

## 1. Architecture

```
J1 (XH 2-pin) -> VIN_12V_RAW -> F1 (1.5 A PPTC) -> VIN_12V_FUSED -> Q1 (P-FET, reverse block)
  -> +12V_PROT --+-- D1 SMAJ15A TVS to GND (after Q1)
                 +-- C1 100 uF, C2/C3 10 uF, C4 100 nF
                 +-- U1 MP2307 buck -> BUCK_SW -> L1 10 uH -> +5V
+5V --+-- F2 -> OUT_5V -> J3
      +-- U3 TPS22918 (ON = LOAD_EN, default OFF) -> +5V_SW -- F3 -> OUT_5V_SW -> J4
      +-- U2 AMS1117-3.3 -> +3V3 -> J5, and via R9 (100 R) to J2 pin 1
J2: control header (LOAD_EN in, SENSE_12V/5V/3V3 out, 3V3_REF, GND)
```

## 2. Load assumptions

| Item | Value |
|---|---|
| Input | nominal 12 V; **designed range 9 to 15 V** (see 4.3: SMAJ15A standoff is 15 V, so the provisional 16 V top is not used) |
| Hot-plug | assumed: J1 is hot-plugged from a bench supply or wall adapter |
| Transient environment | generic DC adapter; one 400 W 10/1000 us pulse (SMAJ15A rating). Not automotive load dump (ISO 7637 / ISO 16750) |
| Provisional targets | 5 V (incl. switched) 1.5 A and 3.3 V 0.5 A, both from the one buck: 2.0 A |
| **Supportable (defended below)** | **total buck load 1.5 A** (5V + 5V_SW + 3V3 LDO input current), of which **3V3 <= 0.35 A** |

The 2.0 A target is **not** supported: F1 limits it (3.1) and the AMS1117 copper limits 3V3 to 0.35 A (3.4).
Unresolved product specification: whether the end user needs 2.0 A. If so, F1 must be a 2.5 A+ hold part
and the AMS1117 needs a copper pour or a different regulator.

## 3. Calculations

### 3.1 Input current and F1 (1812L150/16DR, 1.5 A hold, 3.0 A trip, 16 V [recalled])
Pout = 5.01 V x 1.5 A = 7.52 W. Efficiency assumed 85 % (MP2307 typical is higher; 85 % is conservative) -> Pin = 8.85 W.

| Vin | Iin at 1.5 A total | Iin at 2.0 A total (Pout 10.0 W, Pin 11.8 W) |
|---|---|---|
| 9 V | 0.98 A | 1.31 A |
| 12 V | 0.74 A | 0.98 A |
| 15 V | 0.59 A | 0.78 A |

PPTC hold current derates with temperature; about 80 % at 40 C [recalled], i.e. 1.2 A.
At 9 V: 1.5 A total is 0.98 A = 82 % of the derated hold current (acceptable); 2.0 A total is 1.31 A = 109 % (nuisance trips). Hence the 1.5 A total.
F1 voltage drop at 1 A: about 0.12-0.2 V [recalled resistance]. F1 is resettable and sits where it can be reached (left, beside J1).

### 3.2 Reverse-polarity PFET Q1 (Si2301CDS: Vds -20 V, Vgs +/-8 V abs max, Rds(on) <= 0.112 ohm at Vgs -4.5 V [recalled])
Orientation: drain on the input side (VIN_12V_FUSED), source on the load side (+12V_PROT), so the body diode conducts in the forward direction. Pins checked against the footprint: SOT-23 pad 1 = G, 2 = S, 3 = D.
Gate network: R1 47 k from gate to GND; D9 BZT52C5V1 (5.1 V zener, 4.8-5.4 V [recalled]) from gate (anode) to source (cathode).

| Case | Vs | Vg | Vgs | Result |
|---|---|---|---|---|
| Normal 9 V | 8.9 V | Vs - Vz = 3.8 V | about -4.9 to -5.1 V | on, Rds about 0.11 ohm, 1 A -> 0.11 W, 0.11 V drop |
| Normal 15 V | 14.8 V | 9.7 V | -5.1 V (max -5.4 V) | inside +/-8 V |
| Transient to 24.4 V (TVS clamp) | 24.4 V | 19.3 V | -5.2 V | inside +/-8 V; zener current (24.4 - 5.1)/47k = 0.41 mA |
| Reversed supply -15 V | about 0 V (rail caps discharged) | 0 V | 0 V | off; body diode blocked; Vds = -15 V, inside -20 V (5 V margin) |

Gate-source stays at the zener voltage over the whole range, so the gate is never exposed to the +/-8 V limit. A resistor alone to ground would give Vgs = -15 V, which is why the zener is there.
Zener bias current is 81 uA at 9 V (knee region, Vz may be about 4.9 V) and 0.21 mA at 15 V; zener dissipation is about 1 mW.

Residual risks (not removed):
- If the supply is swapped while +12V_PROT is still charged (about 1 s time constant through the dividers and LEDs), Vs = +12 V and the gate zener turns Q1 on in reverse, dumping the rail capacitors into the source through F1 (peak limited only by Rds and wiring; 7 mJ stored in C1). Vds momentarily approaches -24 V, beyond the -20 V rating. Do not hot-swap-reverse. The Si2301 is the only registry P-FET; a -30 V part would remove this.
- Q1 conducts both ways when on. With J1 unplugged the rail capacitors can discharge back toward J1 through Q1; no second source exists on the board, so only stored energy flows.

### 3.3 Protection chain and position of each element

| Order | Element | Fault it answers | Why this position |
|---|---|---|---|
| 1 | J1 XH 2-pin, rated 3 A/contact [recalled] | mechanical entry, polarity keying | keyed housing; pin 1 = +12 V, pin 2 = GND |
| 2 | F1 PPTC 1.5 A hold | over-current, shorted rail | first element: limits fault current for everything behind it and gives the TVS a source impedance |
| 3 | Q1 PFET + D9 + R1 | reversed supply | after F1 so a reversed supply is blocked before it reaches any load; before the TVS so the TVS never sees a reversed rail |
| 4 | D1 SMAJ15A to GND | transient | **after F1 and after Q1.** A unidirectional TVS ahead of Q1 would forward-conduct on a reversed supply; here it sits on +12V_PROT, which Q1 holds near 0 V in reverse |
| 5 | C1 100 uF, C2/C3 10 uF, C4 100 nF | ripple, hot-plug ringing | on +12V_PROT at the buck. The electrolytic damps the L-C ring of a hot-plugged cable (ceramics alone can ring to about 2 x Vin) |
| 6 | F2, F3 (1.5 A PPTC) | output shorts | one per 5 V output connector |
| 7 | AMS1117 current limit and thermal shutdown | 3V3 short | no fuse on J5 (a PPTC would drop 0.1 V or more of 3.3 V); see 6 |

TVS standoff/clamp [recalled]: VRWM 15 V, VBR 16.7-18.5 V, VC 24.4 V at 16.4 A. Input range is therefore limited to 15 V.
Clamp against downstream: MP2307 IN absolute maximum is **not verified** (I recall 26 V; I used 24 V to be conservative). 24.4 V at the rated 16.4 A **exceeds 24 V**. The clamp is below 24 V for surge currents under about 15 A (linear interpolation of VC between VBR and 24.4 V; an estimate). The 25 V ceramics C2/C3 and the 25 V electrolytics are inside the clamp. The chain is **not claimed safe** against a full-rated 400 W surge until the MP2307 limit is read from its datasheet.

### 3.4 5 V buck (MP2307DN-LF-Z, SOIC-8E; Vref 0.925 V, fsw 340 kHz, 3 A [recalled]; synchronous, so no catch diode)
- Feedback: R3 = 44.2 k (top), R4 = 10 k (bottom): Vout = 0.925 x (1 + 4.42) = **5.014 V**. With 1 % resistors and +/-1.5 % Vref, 4.85-5.18 V.
- Inductor L1 = 10 uH, Bourns SRN6045TA-100M, Isat about 3.5 A and Irms about 3 A [recalled]. Ripple dI = Vout(1 - D)/(L x f): at 15 V in, D = 0.333, dI = 5 x 0.667/(10 uH x 340 kHz) = 0.98 A p-p; peak at 1.5 A load = 1.99 A (2.49 A at 2.0 A). At 9 V in: 0.65 A p-p. Ripple is 65 % of 1.5 A (high, but inside Isat). Isat is below the MP2307 current limit (about 4-4.5 A [recalled]), so a hard output short can saturate L1 until the regulator hiccups.
- Output capacitance: C8/C9 2 x 22 uF 10 V (about 50 % effective at 5 V bias -> 22 uF) plus C10 100 uF 25 V electrolytic. Ripple dV = dI/(8 f C) = 0.98/(8 x 340 kHz x 22 uF) = 16 mV + ESR term about 5 mV. Load step 1 A, loop response about 8 us: dV = 1 A x 8 us/(22 + 100 uF) = 0.07 V.
- Input: C2/C3 2 x 10 uF 25 V; Irms = Iout x sqrt(D(1-D)) = 0.75 A at D = 0.5; ripple Iout x D(1-D)/(f C) = 1.5 x 0.25/(340 kHz x 10 uF) = 0.11 V.
- BS: C5 10 nF to SW. SS: C6 100 nF. COMP: R2 10 k + C7 2.2 nF in series to GND. These three are typical datasheet-application values from memory; **loop stability is not verified** and needs a bench load-step test.
- EN: R5 100 k / R6 33 k from +12V_PROT: EN = 2.2 V at 9 V and 3.7 V at 15 V (EN threshold about 1.2 V [recalled]); the buck starts about 4.8 V in. EN absolute maximum is not verified.
- Exposed pad: pad 9 is connected to GND (the shipped registry part `mp2307_buck` had no pad-9 pin, so the pad would have floated; replaced by the authored `mp2307_buck_ep`). Pad is 2.41 x 3.3 mm = 8 mm^2, joined to the GND zone on F.Cu and B.Cu. **Thermal copper area was not measured** and no thermal via array was specified.
- Dissipation: about 1.33 W total loss at 1.5 A (Pout x (1/0.85 - 1)); IC share roughly 0.45-0.5 W (conduction 2 x 0.13 ohm x Irms^2 plus switching, estimated). At about 60 C/W [assumed for SOIC-8E on small copper] that is about +30 C, Tj about 55 C at 25 C ambient.
- Start-up: EN at about 4.8 V, soft-start through C6 ramps +5V over milliseconds (time not computed: SS current/threshold not verified). U3 is off (LOAD_EN low) during the ramp, so the switched rail does not load the buck while it starts.
- BUCK_SW is a routed net (1.0 mm class PWR5; router narrowed it to 0.85-0.98 mm at pad necks). TP7 is on it.

### 3.5 Load switch (TPS22918DBVR, 5.5 V max, 2 A, Ron about 60 mohm at 5 V [recalled])
- **Pinout correction.** The shipped registry part `tps22918_loadswitch` lists 5 pins on a 6-pad SOT-23-6 and puts VOUT on pad 5. My recollection of the datasheet is 1 VIN, 2 GND, 3 ON, 4 CT, 5 QOD, 6 VOUT. I authored `tps22918_sot23_6` with that map. **This pin map is [recalled], not verified; confirm before ordering.** If the registry were right instead, VOUT and QOD are swapped and the board is wrong.
- ON: threshold VIH 1.0 V / VIL 0.6 V [recalled], pin abs max 6.5 V [recalled]. R7 100 k pulls to GND: **default OFF**. J2 pin 3 drives it through R8 1 k; D10 (5.1 V zener) clamps the pin, so a 12 V mistake on the header is 6.9 mA through R8 and the ON pin stays under 5.4 V. Host high of 3.3 V reads 3.27 V at ON. Test point TP8 on LOAD_EN. With nothing connected the output stays off; turn it on by wiring J2 pin 3 to a 3.3 V source (for example J2 pin 1).
- QOD (pad 5) is tied to VOUT so the output discharges when off. Discharge resistance not verified.
- Soft start: C13 2.2 nF on CT. Rise time grows with CT; the datasheet table value is not verified, so no turn-on time is claimed. Inrush formula I = C x 5 V / tR. For C = C14 22 uF + 100 uF assumed external load = 122 uF: tR = 1 ms gives 0.61 A, tR = 3 ms gives 0.2 A; both under 2 A. If tR is shorter than 0.3 ms the inrush would exceed 2 A.
- TPS22918 has no current limit [recalled]; F3 (1.5 A hold) is the limit on J4.
- Input capacitor C12 10 uF; output C14 22 uF.

### 3.6 3.3 V regulator (AMS1117-3.3, SOT-223, tab = VOUT [recalled])
- Pinout checked against the footprint `SOT-223-3_TabPin2`: pad 1 GND, 2 VOUT (tab), 3 VIN. The tab pad is tied to +3V3.
- Dropout 1.1-1.3 V [recalled]; 5.01 - 3.3 = 1.7 V, margin about 0.4-0.6 V.
- Dissipation at worst: Vin 5.18 V, 0.5 A -> (5.18 - 3.3) x 0.5 = **0.94 W**; at 5.01 V and 0.5 A, 0.86 W. Synth's own check estimates 1.36 W (it assumes 0.8 A) and warns W-SYNTH-THERMAL-001.
- SOT-223 on minimal copper is about 90 C/W [recalled]. 0.94 W -> +85 C -> Tj 110 C at 25 C ambient and 135 C at 50 C: **exceeds the 125 C limit**. The only copper given is the tab pad and a trace; no pour was specified.
- **Defended limit: 0.35 A** (0.66 W, +59 C, Tj about 99 C at 40 C ambient, 90 C/W). The 0.5 A target needs a copper pour of about 5 cm^2 or a switching 3.3 V stage. Reported as an open issue.
- Stability: C16 10 uF ceramic + C17 100 uF electrolytic (ESR of a general electrolytic about 0.3-1 ohm gives the zero the AMS1117 wants; ceramic alone is marginal). Not bench-verified. No in-to-out protection diode is fitted (output capacitance 110 uF; add a Schottky if the 5 V rail can collapse faster than 3V3).
- Power-up order: +12V_PROT -> buck EN (about 4.8 V) -> +5V ramps -> 3V3 follows once +5V exceeds about 4.6 V (3.3 V + dropout), so 3V3 cannot be present before +5V and stays below it. +5V_SW stays off until the host asserts LOAD_EN. Power-down: capacitors bleed through the dividers, LEDs and loads.

### 3.7 Control header (J2, 1x8, 2.54 mm)
See `PIN_MAPPING.md`. Dividers and clamps:
- SENSE_12V: R10 10 k / R11 2 k -> k = 0.1667; 15 V -> 2.50 V; 16 V -> 2.67 V; then R12 1 k and C19 100 nF to the pin. D11 BZT52C3V3 at the divider node clamps a 24.4 V transient to 3.3-3.5 V (2.1 mA through R10).
- SENSE_5V: R13/R14 10 k/10 k -> k = 0.5; 5.01 V -> 2.51 V; 5.5 V -> 2.75 V. R15 1 k, C20, D12.
- SENSE_3V3: R16/R17 10 k/10 k -> 1.65 V. R18 1 k, C21, D13.
- Output impedance at the pin about 2.7 k (12 V) and 6 k (5 V, 3V3); a SAR ADC with a small sampling capacitor needs the 100 nF reservoir already fitted.
- Zener leakage error at 2.5 V is a few uA x 1.67 k, about 8 mV (0.3 %).
- 3V3_REF: +3V3 through R9 100 ohm: shorted, 33 mA. Reference use only, 5 mA (0.5 V drop).
- Every pin is 3.3 V compatible: highest voltage any signal pin can present is the zener clamp (about 3.5 V; 5.4 V on LOAD_EN, which is an input to the board, not a host input).

**Fault status: no pin is provided.** No part on the board produces a fault or power-good output (MP2307 has no PG pin; TPS22918 has no fault pin [recalled]). I did not add a comparator. A local indicator exists instead: D7 (red) across F1 lights when F1 drops more than about 2 V, i.e. when it has tripped (3.0 mA at 12 V across 3.3 k). F1 normal drop is under 0.3 V, so the LED is off. When tripped, D7 and R24 also pass about 3 mA into the dead rail, which keeps the buck hovering; this is cosmetic.

**Input current sensing: not added.** A shunt, amplifier and header range were out of scope for the parts in the registry.

### 3.8 Indicators (resistors calculated from rail and Vf)

| LED | Colour | Rail | Vf (est.) | R | I at nominal | Meaning |
|---|---|---|---|---|---|---|
| D2 | white | VIN_12V_RAW via D8 (reverse block, 0.3 V) | 3.1 V | R19 3.0 k | (12 - 3.1 - 0.3)/3.0 k = 2.9 mA (1.9 mA at 9 V, 3.9 mA at 15 V) | input present |
| D3 | blue | +12V_PROT | 3.1 V | R20 3.3 k | 2.7 mA | protected rail up |
| D4 | green (LTST-C190KGKT) | +5V | 2.1 V | R21 1 k | 2.9 mA | 5 V up |
| D5 | green (0603-GREEN) | +5V_SW | 2.2 V | R22 1 k | 2.8 mA | switched output on |
| D6 | amber | +3V3 | 2.0 V | R23 470 | 2.8 mA | 3.3 V up |
| D7 | red (LTST-C190KRKT) | across F1 | 2.0 V | R24 3.3 k | 3.0 mA at 12 V | F1 tripped |

D4 and D5 are the same colour (the registry has five distinct colours for six LEDs); they are separate positions on separate rails. No silkscreen legends exist (section 7), so the LED names are only the reference designators.
A blue/white LED on 3V3 would not light (Vf about 3.1 V), which is why D6 is amber.

## 4. Design deviations from the brief
1. Input range reduced from 9-16 V to 9-15 V (TVS standoff).
2. Supportable load 1.5 A total (3.1), 3V3 <= 0.35 A (3.6), not the 2.0 A / 0.5 A targets.
3. FAULT line left off the header (no real source).
4. 3V3 output has no fuse (AMS1117 current limit and thermal shutdown stand in; see 3.3).
5. Three authored parts and two corrected registry parts (section 5).

## 5. Parts imported or authored (Tier-2, `user-registry/`), and why
| Part | Reason |
|---|---|
| `mp2307_buck_ep` | shipped `mp2307_buck` has no pad-9 pin (the exposed pad would float) and names the non-existent footprint `SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.4x3.4mm`. Footprint changed to `..._EP2.41x3.3mm` (exists in KiCad). EP size not verified against the MPS land pattern. |
| `tps22918_sot23_6` | shipped part has 5 pins for a 6-pad footprint (3.5). |
| `l_power_10uh_6045` | shipped inductors (CBC3225, 1210) are about 0.5-1 A parts, far below the 2.5 A peak. |
| `header_1x8` | registry has only 1x3, 1x4, 1x20. |
| `bzt52c3v3` | registry 3.3 V zener has no MPN. |
All five are marked unverified (`W-SYNTH-PART-UNVERIFIED`) until a reviewer signs them.

## 6. Output connectors and short circuits
| Connector | Pin 1 / pin 2 | Limit and short-circuit behaviour |
|---|---|---|
| J3 | +5V / GND | F2 1.5 A hold, 3.0 A trip [recalled]; behind it the buck current limit. Total with J4 <= 1.5 A |
| J4 | +5V_SW / GND | F3 1.5 A hold; TPS22918 has no current limit [recalled]; load switch off by default |
| J5 | +3V3 / GND | AMS1117 limit (about 1 A [recalled]) and thermal shutdown; a hard short dissipates about 5 W and relies on thermal shutdown. Rated use 0.35 A. No fuse |
XH housing and contact rating 3 A [recalled]. Mating: JST XHP-2 housing and SXH-001T-P0.6 contacts. A short on J3 or J4 can pull more than 3 A for the seconds F2/F3 take to trip, beyond the connector rating (accepted, reported).
Back-driven voltage: none of the output connectors is protected against an external supply driven into it. The MP2307 can sink into a driven rail; the AMS1117 can be damaged by Vout > Vin. Accepted as a documented risk.

## 7. PCB
- 2 layers; **110.5 x 72.5 mm** (outline x 1.25-111.75, y 1.25-73.75, auto-sized by Synth from the component extents); board thickness 1.6 mm (Synth default; no `stackup` block declared). Four M2.5 NPTH holes at (4,4), (109,4), (4,71), (109,71).
- Top layer: components and routing. Bottom layer: GND pour (plus F.Cu GND fill). Signal routing on B.Cu only where the router hopped.
- Placement (sidecar, hard overrides): J1, F1, Q1, D9/R1, D1, C1 on the left in chain order; U1, L1 and its caps in the middle; U3 and U2 on the right and bottom; J3-J5 on the right edge; J2 and the sense network on the bottom; each LED sits near its own rail (D2, D7, D8 at the top left beside F1; D3 at the top beside D1; D4 beside the buck output; D5 beside U3; D6 beside U2). Placement review score 100, no findings. Test points sit beside the net they probe; TP1, TP2, TP3 are on the left, TP9 (GND) beside J1 and TP10 (GND) beside J2, with 1.5 mm pads clear of neighbours (not verified with a probe).
- Connector orientation: J1 rotated 270 and J3-5 rotated 90, so the XH opening faces the board edge (the body extends from the pad row toward local +y, which these rotations send to -x and +x respectively; derived from the footprint geometry and the KiCad rotation convention, not seen in a render). J1 origin x = 9.7, J3-5 origin x = 100.5; the board outline grew to the connector courtyards, so they are flush with the 1.25 mm margin. No rendered PCB image was available to confirm this.
- **Silkscreen: Synth gives no way to place text or legends.** Only the footprint outlines, pin-1 marks and reference designators exist. There are **no** "+12V", "GND", "J1 POLARITY" or net labels on the exported board. Polarity marking is only the pin-1 arrow in the XH footprint.
- Mounting holes: 4 x M2.5 NPTH at the corners; each has a keepout zone for the screw head.
- Net classes (declared in the source and assigned by name; confirmed present in the exported `.kicad_pcb` header):

| Class | Width / clearance / via | Nets |
|---|---|---|
| PWR12 | 0.8 / 0.25 / 0.8-0.4 mm | VIN_12V_RAW, VIN_12V_FUSED, +12V_PROT |
| PWR5 | 1.0 / 0.25 / 0.8-0.4 mm | +5V, +5V_SW, BUCK_SW, OUT_5V, OUT_5V_SW |
| PWR3 | 0.5 / 0.25 / 0.8-0.4 mm | +3V3 |
| PGND | 0.8 / 0.25 / 0.8-0.4 mm | GND |
| SIG | 0.25 / 0.2 / 0.6-0.3 mm | all the other 23 nets |

Trace widths from IPC-2221 (external, 1 oz, 10 C rise): 1.3 A needs about 0.42 mm; 2 A needs about 0.78 mm; so 0.8 mm on the 12 V side and 1.0 mm on the 5 V side leave margin.

- **Applied widths: the classes were declared and assigned to every net (confirmed in the `.kicad_pcb` header), but the router did not honour them everywhere** (see `VERIFICATION_REPORT.md` section 4). In the final export the router's own report lists 22 narrowed sites, all at pad "terminal necks": +5V 0.70-0.90 mm, +12V_PROT 0.664 mm, VIN_12V_RAW 0.726 mm, BUCK_SW 0.85-0.98 mm, +5V_SW 0.80 mm (declared 1.0 / 0.8 / 0.8 / 1.0 / 1.0), plus SENSE_12V and SENSE_5V at 0.2 mm (declared 0.25). VIN_12V_FUSED, +3V3, OUT_5V and OUT_5V_SW are not listed, so they were routed at their declared width. GND copper stubs were 0.2 mm in an earlier export of this session (class PGND says 0.8 mm) and the report never lists GND; I did not re-read GND widths in the final file. Vias are 0.6/0.3 mm everywhere I looked, not the 0.8/0.4 mm the power classes declare. I read the segment list of an earlier export, not of the final one, so the per-segment widths are not re-confirmed for the final file.
- Switch-node copper is short (BUCK_SW joins U1.3, L1.1, C5.2 and TP7 on F.Cu at 0.85-1.0 mm), and the FB pin (U1 pin 5) is on the opposite side of the package.
- Signal-integrity findings returned by the final export (8, 0 blocking; the engine counts nets from 0, so engine net N is file net N+1): one 18.2 mm parallel run of EN_HDR and SENSE_3V3 at 0.35 mm (static signals); and layer changes with no ground via within 1.53 mm on LOAD_EN, **+12V_PROT at (60.1, 33.9) next to U1's input pin**, +3V3 at (60.7, 66.6), S12_DIV, SENSE_12V, S5_DIV, SENSE_5V. The +12V_PROT hop through a 0.6 mm via beside the buck input is the one power-path finding; it was not fixed.
