---
description: Enforces mandatory user review and approval of KiCad schematic before PCB routing
trigger: always_on
---

# KiCad Schematic Review Gate Rule

Whenever a workflow generates or exports a KiCad schematic:
1. Stop before PCB component placement refinement or PCB routing.
2. Present the schematic files, visual renders, and key net/part facts to the user.
3. Explicitly ask for user approval before invoking `/plan-pcb-routing`, `/plan-pcb-placement full`, `route.py`, or any external autorouter.
4. Only initiate PCB routing once explicit confirmation is received from the user.
