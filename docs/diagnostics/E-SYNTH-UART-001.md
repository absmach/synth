# E-SYNTH-UART-001 — UART endpoint connected to non-UART pin

**Severity:** error
**Stage:** erc — protocol

## What this means

A dedicated UART pin (`uart_tx`/`uart_rx` capability with no muxable alternative) was wired to a pin that carries no UART capability.

## Minimal reproduction

(intentionally omitted — most UART-capable pins are muxable GPIOs which the rule correctly ignores)

## Suggested fix

Connect UART TX to the peer's RX (and vice versa). Cross-couple RX↔TX, never RX↔RX.
