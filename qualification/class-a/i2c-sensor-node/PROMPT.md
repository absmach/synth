# Prompt

This prompt is written from the finished board so that someone can rerun it. It is not the text the agent was given. The prompts the agent ran are in the history at `dcf1e328`.

Using synth mcp server design a two-layer I2C sensor node for JLCPCB, about 84 x 50 mm. Power it from a 5 V JST XH input through a 0.5 A resettable fuse, a reverse-polarity Schottky diode and a TVS, then regulate 3.3 V with an AMS1117-3.3 for a 100 mA load. Use an STM32G030F6P6, a BME680 temperature/humidity/pressure/gas sensor and a 24LC256 EEPROM on one I2C bus. Add an SWD programming header, a 4-pin UART breakout, and a 12-pin GPIO header with a 330 ohm series resistor and a 3.3 V zener clamp on every line. Include a power LED and two M2.5 mounting holes. Group the schematic by function and keep the design notes explicit about protection and load assumptions.
