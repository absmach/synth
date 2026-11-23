# Workspace Agent Rules & Workflow Guidelines

## Mandatory KiCad Schematic Review Gate

When generating or modifying circuits and PCB designs in this workspace, the agent must enforce a strict **Human-in-the-Loop Review Gate** between schematic generation and PCB routing:

### 1. Schematic Generation Stage
* When a KiCad schematic (`.kicad_sch`), multi-sheet hierarchy, or schematic render (SVG/PNG) is generated (e.g., via `synth export-kicad --allow-incomplete`, `synth render`, `synth_render_schematic`, `synth_preview_schematic`, or `synth_export`):
* The agent must compile and export the schematic files, symbol table, and bill of materials (BOM).

### 2. Mandatory User Review & Pause (NO Automatic Routing)
* **DO NOT** proceed immediately to PCB component placement or PCB routing (`synth_route`, `plan-pcb-routing`, `route.py`, or `synth export-kicad` with routing).
* The agent **MUST pause** and present the schematic review packet to the user:
  1. Provide clickable file links using `file://` scheme to:
     - The root schematic file (`<project>.kicad_sch`)
     - Any sub-sheet schematic files
     - Rendered schematic images (SVG/PNG)
     - The generated BOM (`bom.csv`)
  2. Summarize key design facts:
     - Power rails, voltage domains, and ground ties
     - Major ICs and connector pinouts
     - High-speed or differential pair nets
     - Any schematic ERC warnings or aesthetic findings
  3. Explicitly ask the user to inspect and approve the schematic.

### 3. Pre-Placement Symbol & Footprint Verification Gate
* **Full Symbol & Footprint Requirement**: Every part in the schematic must have a complete, verified KiCad schematic symbol (in `.kicad_sym` or symbol library) and physical footprint (`.kicad_mod` in `<project>.pretty/` or KiCad footprint library) before component placement starts.
* **Prohibit Direct Unverified Synthetic PCB Footprints**: Never synthesize fallback footprints directly onto `.kicad_pcb` without generating full `.kicad_mod` and `.kicad_sym` definitions.
* **Multi-Pad & Package Topology Verification**:
  - Parts with multi-pad pins (e.g. 4-pin tactile switches with paired terminals, power MOSFETs with shared drain pads) must explicitly declare `physical_pads` mapping all physical footprint pads so every pad is netted and routed.
  - Parts with quad-sided packages (e.g., castellated MCU modules like ESP32-P4, QFN, QFP) must declare or infer `package_type = "quad"` with pads distributed across all 4 perimeter edges (West, South, East, North in counter-clockwise order), never truncated to two sides.
* **Symbol-to-Footprint Pin Match**: Verify that 100% of symbol pins map to valid footprint pads prior to running placement.

### 4. PCB Routing Stage (Requires User Approval)
* **Proceed to PCB routing ONLY after the user has reviewed and explicitly approved the schematic.**
* If the user requests changes, apply the changes to the source/schematic, re-render, and present the updated schematic for review.
* Once approved by the user, proceed to PCB component placement (`/plan-pcb-placement`) and PCB routing (`/plan-pcb-routing`).
