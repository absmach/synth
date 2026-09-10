#!/usr/bin/env python3
import os

out_dir = "fixtures/place-errors"
os.makedirs(out_dir, exist_ok=True)

# 1. Area Overflow (01..05) — Exceeds 230x230mm max board area (needs >23,805mm² courtyard, i.e. >400 MCUs)
for i in range(1, 6):
    path = os.path.join(out_dir, f"area_overflow_{i:02d}.synth")
    with open(path, "w") as f:
        f.write(f'board "area_overflow_{i:02d}" {{\n  layers 2\n')
        for c in range(1, 450 + i * 20):
            f.write(f'  component MCU{c}: mcu "rp2350"\n')
        f.write('  connect MCU1.run -> MCU2.run\n}\n')

# 2. No Position Available (01..05) — Keepout covers almost entire board, remaining area too small
for i in range(1, 6):
    path = os.path.join(out_dir, f"no_position_{i:02d}.synth")
    radius = 110 + i * 5
    with open(path, "w") as f:
        f.write(f'board "no_position_{i:02d}" {{\n  layers 2\n')
        f.write('  component U1: mcu "rp2350"\n')
        f.write('  component ANT1: antenna "ant_chip_2g4"\n')
        for c in range(1, 15 + i * 2):
            f.write(f'  component C{c}: capacitor "c_generic_0603"\n')
            f.write(f'  connect U1.vdd_io -> C{c}.p1\n')
            f.write(f'  connect U1.gnd -> C{c}.p2\n')
        f.write(f'  keepout ant {{\n    radius {radius}mm\n  }}\n')
        f.write('}\n')

# 3. Keepout Blocked (01..05) — Radius > 300mm covers entire board
for i in range(1, 6):
    path = os.path.join(out_dir, f"keepout_blocked_{i:02d}.synth")
    radius = 300 + i * 50
    with open(path, "w") as f:
        f.write(f'board "keepout_blocked_{i:02d}" {{\n  layers 2\n')
        f.write('  component U1: mcu "rp2350"\n')
        f.write('  component U2: regulator "ams1117_3v3"\n')
        f.write('  component J1: connector "usb_c_receptacle"\n')
        f.write('  component ANT1: antenna "ant_chip_2g4"\n')
        f.write(f'  keepout board_blocker {{\n    radius {radius}mm\n  }}\n')
        f.write('  connect J1.vbus -> U2.vin\n')
        f.write('  connect U2.vout -> U1.vdd_io\n')
        f.write('}\n')

# 4. Oversized / Extreme constraint (01..05) — Radius 500mm+
for i in range(1, 6):
    path = os.path.join(out_dir, f"oversized_{i:02d}.synth")
    radius = 500 + i * 100
    with open(path, "w") as f:
        f.write(f'board "oversized_{i:02d}" {{\n  layers 2\n')
        f.write('  component U1: mcu "rp2350"\n')
        f.write('  component ANT1: antenna "ant_chip_2g4"\n')
        for c in range(1, 15):
            f.write(f'  component MCU_BLOCK{c}: mcu "rp2350"\n')
        f.write(f'  keepout ant_huge {{\n    radius {radius}mm\n  }}\n')
        f.write('}\n')

print(f"Generated 20 placement error fixtures in {out_dir}")
