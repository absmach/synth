#!/usr/bin/env python3
"""
Generates parametric .synth test fixtures for the AgentHarness corpus under fixtures/agent/generated/.
Fixtures without machine-applicable textual fixes are prefixed with noconverge__ per fixtures/agent/README.md.
"""

from pathlib import Path

OUTPUT_DIR = Path(__file__).parent.parent / "fixtures" / "agent" / "generated"

def ensure_dir():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

def generate_diff_pair_variants():
    for i in range(1, 8):
        content = f"""board "gen_diff_pair_{i}" {{
  layers 2
  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  component R3: resistor "r_generic_0603"
  component R4: resistor "r_generic_0603"

  connect R1.p1 -> R2.p1
  connect R3.p1 -> R4.p1

  diff_pair r1_p1 r3_p1 {{
  }}
}}
"""
        (OUTPUT_DIR / f"gen_diff_pair_{i}.synth").write_text(content)

def generate_power_decoupling_variants():
    for pins in range(2, 9):
        endpoints = "\n".join([f'  connect U1.vcc -> J{j}.p1' for j in range(1, pins + 1)])
        connectors = "\n".join([f'  component J{j}: connector "header_1x4"' for j in range(1, pins + 1)])
        content = f"""board "gen_power_dec_{pins}" {{
  layers 2
  component U1: ic "ch340g"
{connectors}

{endpoints}
  connect U1.gnd -> J1.p2
}}
"""
        (OUTPUT_DIR / f"gen_power_dec_{pins}.synth").write_text(content)

def generate_single_endpoint_variants():
    for i in range(1, 8):
        content = f"""board "gen_single_end_{i}" {{
  layers 2
  component U{i}: ic "ch340g"
  component J1: connector "header_1x4"

  connect U{i}.tx -> J1.p1
  connect U{i}.rx -> J1.p2
  connect U{i}.cts -> J1.p3
}}
"""
        (OUTPUT_DIR / f"noconverge__gen_single_end_{i}.synth").write_text(content)

def generate_keepout_variants():
    for i in range(1, 8):
        content = f"""board "gen_keepout_{i}" {{
  layers 2
  component U{i}: ic "esp32_wroom_32"
  component J1: connector "header_1x4"

  connect U{i}.tx -> J1.p1
  connect U{i}.rx -> J1.p2

  keepout antenna {{
  }}
}}
"""
        (OUTPUT_DIR / f"gen_keepout_{i}.synth").write_text(content)

def generate_boot_floating_variants():
    for i in range(1, 8):
        content = f"""board "gen_boot_floating_{i}" {{
  layers 2
  component U{i}: ic "esp32_wroom_32"
  component J1: connector "header_1x4"

  connect U{i}.tx -> J1.p1
  connect U{i}.gnd -> J1.p2
}}
"""
        (OUTPUT_DIR / f"gen_boot_floating_{i}.synth").write_text(content)

def generate_rf_keepout_variants():
    for i in range(1, 8):
        content = f"""board "gen_rf_keepout_{i}" {{
  layers 2
  component U{i}: ic "esp32_wroom_32"
  component ANT{i}: antenna "antenna_2_4ghz"

  connect U{i}.rf_feed -> ANT{i}.rf_in
}}
"""
        (OUTPUT_DIR / f"noconverge__gen_rf_keepout_{i}.synth").write_text(content)

def generate_i2c_pullup_variants():
    for i in range(1, 8):
        content = f"""board "gen_i2c_pullup_{i}" {{
  layers 2
  component MCU{i}: ic "stm32f401"
  component SENSOR{i}: sensor "mpu6050"

  connect MCU{i}.scl -> SENSOR{i}.scl
  connect MCU{i}.sda -> SENSOR{i}.sda
}}
"""
        (OUTPUT_DIR / f"noconverge__gen_i2c_pullup_{i}.synth").write_text(content)

def generate_reset_floating_variants():
    for i in range(1, 8):
        content = f"""board "gen_reset_floating_{i}" {{
  layers 2
  component MCU{i}: ic "stm32f401"
  component J{i}: connector "header_1x4"

  connect MCU{i}.pa0 -> J{i}.p1
}}
"""
        (OUTPUT_DIR / f"noconverge__gen_reset_floating_{i}.synth").write_text(content)

def main():
    ensure_dir()
    # Clean old generated files first
    for p in OUTPUT_DIR.glob("*.synth"):
        p.unlink()
    generate_diff_pair_variants()
    generate_power_decoupling_variants()
    generate_single_endpoint_variants()
    generate_keepout_variants()
    generate_boot_floating_variants()
    generate_rf_keepout_variants()
    generate_i2c_pullup_variants()
    generate_reset_floating_variants()
    print(f"Generated parametric fixtures in {OUTPUT_DIR}")

if __name__ == "__main__":
    main()
