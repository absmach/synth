---
description: Enforces mandatory user review and approval of KiCad schematic before PCB routing
trigger: always_on
---

# KiCad Schematic Review Gate Rule

Whenever a workflow generates or exports a KiCad schematic:
1. Stop before PCB component placement refinement or PCB routing.
2. Ensure every component has a complete, verified KiCad schematic symbol and physical footprint (.kicad_mod) generated in the library before placement.
   - For multi-pad components (e.g. 4-pin switches), map all physical pads via `physical_pads`.
   - For quad-sided packages (e.g. ESP32-P4 module, QFN), ensure pads are distributed around all 4 perimeter edges (West, South, East, North) rather than dual columns.
3. Present the schematic files, visual renders, and key net/part facts to the user.
4. Explicitly ask for user approval before invoking `/plan-pcb-routing`, `/plan-pcb-placement full`, `route.py`, or any external autorouter.
5. Only initiate PCB routing once explicit confirmation is received from the user.
