#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
Generates valid clean .synth design variations into fixtures/designs-generated/
by sampling and parametrizing clean design templates (sensor_logger, secure_tracker, etc.).
Validates each design via `synth validate`.
"""

import json
import os
import random
import subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SYNTH_BIN = REPO_ROOT / "target" / "debug" / "synth"

def ensure_synth_bin():
    if not SYNTH_BIN.exists():
        print("Building synth CLI binary...")
        subprocess.run(["cargo", "build", "-p", "synth-cli", "--quiet"], cwd=REPO_ROOT, check=True)

def generate_sensor_logger_variant(index: int) -> str:
    sensor_types = ["bme680_env", "bmp280_pressure", "hdc1080_humidity", "apds9960_color"]
    reg_types = ["ams1117_3v3", "ams1117_1v8", "ams1117_5v"]
    layers = [2, 4, 6]
    mfrs = ["jlcpcb", "pcbway", "oshpark"]

    sensor = sensor_types[index % len(sensor_types)]
    reg = reg_types[index % len(reg_types)]
    layer = layers[index % len(layers)]
    mfr = mfrs[index % len(mfrs)]

    return f"""// Clean sensor logger variant #{index}
board "sensor_logger_gen_{index}" {{
  layers {layer}
  manufacturer "{mfr}"

  component U1: mcu "stm32f103c8"
  component U2: sensor "{sensor}"
  component U3: regulator "{reg}"

  component J1: connector "micro_usb_b"

  component R1: resistor "r_generic_0603" value "4.7k"
  component R2: resistor "r_generic_0603" value "4.7k"
  component R3: resistor "r_generic_0603" value "10k"

  component C1: capacitor "c_generic_0603" value "10uF"
  component C2: capacitor "c_generic_0603" value "10uF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component C4: capacitor "c_generic_0603" value "100nF"

  connect J1.vbus -> U3.vin
  connect J1.gnd -> U3.gnd

  connect U3.vout -> U1.vdd
  connect U3.vout -> U1.vbat
  connect U3.vout -> U2.vdd

  connect U3.gnd -> U1.vss
  connect U3.gnd -> U2.gnd

  connect U1.pb7 -> U2.sda
  connect U1.pb6 -> U2.scl

  connect U1.pb7 -> R1.p1
  connect U3.vout -> R1.p2

  connect U1.pb6 -> R2.p1
  connect U3.vout -> R2.p2

  connect U1.nrst -> R3.p1
  connect U3.vout -> R3.p2

  connect U3.vin -> C1.p1
  connect U3.gnd -> C1.p2

  connect U3.vout -> C2.p1
  connect U3.gnd -> C2.p2

  connect U1.vdd -> C3.p1
  connect U3.gnd -> C3.p2

  connect U2.vdd -> C4.p1
  connect U3.gnd -> C4.p2
}}
"""

def generate_secure_tracker_variant(index: int) -> str:
    layers = [4, 6, 8]
    mfrs = ["jlcpcb", "pcbway"]
    layer = layers[index % len(layers)]
    mfr = mfrs[index % len(mfrs)]

    return f"""// Clean secure tracker variant #{index}
board "secure_tracker_gen_{index}" {{
  layers {layer}
  manufacturer "{mfr}"

  component U1: mcu "rp2350"
  component U2: secure_element "atecc608"
  component U3: modem "bg95"
  component U4: charger "bq24074"

  component J1: connector "usb_c_receptacle"
  component J2: connector "jst_ph_2pin"
  component ANT1: antenna "ant_chip_2g4"
  component Y1: crystal "osc_smd_25mhz"

  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  component R3: resistor "r_generic_0603"
  component R4: resistor "r_generic_0603"
  component R5: resistor "r_generic_0603"
  component R6: resistor "r_generic_0603"

  component C1: capacitor "c_generic_0603"
  component C2: capacitor "c_generic_0603"
  component C3: capacitor "c_generic_0603"
  component C4: capacitor "c_generic_0603"
  component C5: capacitor "c_generic_0603"
  component C6: capacitor "c_generic_0603"
  component C7: capacitor "c_generic_0603"
  component C8: capacitor "c_generic_0805"
  component C9: capacitor "c_generic_0603"
  component C10: capacitor "c_generic_0603"
  component C11: capacitor "c_generic_0603"

  connect J1.vbus -> U4.vin
  connect J1.gnd -> U4.gnd
  connect J2.p1 -> U4.vbat
  connect J2.p2 -> U4.gnd

  connect U4.vout -> U1.vdd_io
  connect U4.vout -> U1.vdd_core
  connect U4.vout -> U2.vcc
  connect U4.vout -> U3.vbatt
  connect U4.vout -> Y1.vcc

  connect U4.gnd -> U1.gnd
  connect U4.gnd -> U2.gnd
  connect U4.gnd -> U3.gnd
  connect U4.gnd -> Y1.gnd
  connect U4.gnd -> ANT1.gnd

  connect U1.usb_dp -> J1.dp
  connect U1.usb_dn -> J1.dn
  connect J1.cc1 -> R1.p1
  connect R1.p2 -> U4.gnd
  connect J1.cc2 -> R2.p1
  connect R2.p2 -> U4.gnd

  connect U1.gp0 -> U2.sda
  connect U1.gp1 -> U2.scl
  connect U1.gp0 -> R3.p1
  connect U4.vout -> R3.p2
  connect U1.gp1 -> R4.p1
  connect U4.vout -> R4.p2

  connect U1.gp2 -> U3.rx
  connect U1.gp3 -> U3.tx

  connect U1.run -> R5.p1
  connect U4.vout -> R5.p2
  connect U3.reset_n -> R6.p1
  connect U4.vout -> R6.p2

  connect Y1.out -> U1.xin
  connect U3.main_ant -> ANT1.feed

  connect U1.vdd_io -> C1.p1
  connect U4.gnd -> C1.p2
  connect U1.vdd_io -> C2.p1
  connect U4.gnd -> C2.p2
  connect U1.vdd_io -> C3.p1
  connect U4.gnd -> C3.p2
  connect U1.vdd_io -> C4.p1
  connect U4.gnd -> C4.p2

  connect U1.vdd_core -> C5.p1
  connect U4.gnd -> C5.p2
  connect U1.vdd_core -> C6.p1
  connect U4.gnd -> C6.p2

  connect U2.vcc -> C7.p1
  connect U4.gnd -> C7.p2

  connect U3.vbatt -> C8.p1
  connect U4.gnd -> C8.p2

  connect U4.vin -> C9.p1
  connect U4.gnd -> C9.p2

  connect U4.vout -> C10.p1
  connect U4.gnd -> C10.p2

  connect Y1.vcc -> C11.p1
  connect U4.gnd -> C11.p2

  diff_pair u1_usb_dp u1_usb_dn {{
    impedance 90ohm
  }}

  diff_pair u3_main_ant ant1_feed {{
    impedance 50ohm
  }}

  keepout antenna {{
    radius 20mm
  }}
}}
"""

def main():
    ensure_synth_bin()
    out_dir = REPO_ROOT / "fixtures" / "designs-generated"
    out_dir.mkdir(parents=True, exist_ok=True)

    target_count = 150
    generated_count = 0

    print(f"Generating target {target_count} clean synthetic designs into {out_dir}...")

    generators = [generate_sensor_logger_variant, generate_secure_tracker_variant]

    for i in range(1, target_count + 1):
        gen_fn = generators[i % len(generators)]
        src = gen_fn(i)
        file_path = out_dir / f"clean_gen_{i:03d}.synth"
        file_path.write_text(src, encoding="utf-8")

        # Validate with synth CLI
        res = subprocess.run(
            [str(SYNTH_BIN), "validate", str(file_path), "--format", "json"],
            capture_output=True,
            text=True
        )

        valid = False
        if res.returncode == 0 or res.stdout:
            try:
                data = json.loads(res.stdout)
                diags = data.get("diagnostics", [])
                blocking = [d for d in diags if d.get("severity") == "error"]
                if len(blocking) == 0:
                    valid = True
            except Exception:
                pass

        if valid:
            generated_count += 1
        else:
            print(f"Design #{i} had validation errors; unlinking.")
            file_path.unlink(missing_ok=True)

    print(f"Successfully generated and validated {generated_count} clean .synth design files.")

if __name__ == "__main__":
    main()
