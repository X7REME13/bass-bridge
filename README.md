# BassBridge

Low-latency native audio router for Windows and Linux that captures system audio via loopback, applies a 4th-order Linkwitz-Riley (LR4) low-pass crossover filter, and feeds a secondary audio output driving subwoofers or tactile transducers.

## Features

- **System Loopback Routing:** Captures playback audio via WASAPI loopback (Windows) or monitor/input devices (Linux) and routes filtered bass to a dedicated secondary DAC or soundcard, with built-in same-device feedback prevention.
- **Linkwitz-Riley 4th-Order (LR4) Filter:** Two cascaded 2nd-order Butterworth low-pass stages ($Q = 1/\sqrt{2}$) yielding a $-24\text{ dB/octave}$ rolloff and $-6.02\text{ dB}$ at cutoff.
- **Adaptive Cubic Resampler & Continuous Stream Sync:** Catmull-Rom cubic interpolation with drift-controlled rate adaptation and anti-aliasing low-pass pre-filtering keeps asynchronous loopback and output endpoints locked without underrun stutters or high-frequency step clicks.
- **Dynamic Peak Limiter:** Fast-attack, smooth-release stereo-linked peak limiter paired with a $C^1$-continuous soft ceiling at $-0.7\text{ dBFS}$ (`0.92`) avoids harsh clipping without introducing distortion harmonics at normal operating levels.
- **Mono Direct Sum & Stereo Modes:** Mono mode sums $L + R$ cleanly with an optional $+6\text{ dB}$ Direct Sum boost; Stereo mode preserves discrete L/R channels for dual tactile transducers.
- **Time & Phase Alignment:** $0\text{--}500\text{ ms}$ pre-allocated circular delay line with distance readout in meters and $180^\circ$ polarity inversion.
- **Real-Time FFT Spectrum & Triggered Oscilloscope:** 4096-point Hann-windowed live spectrum ($20\text{--}1000\text{ Hz}$) overlaid with the analytical LR4 curve (draggable cutoff handle), plus a rising-edge zero-crossing triggered dual oscilloscope and dBFS peak meters.
- **Persistent Settings:** Automatically saves configuration to `%APPDATA%\BassBridge\config.json` (Windows) or `$XDG_CONFIG_HOME/BassBridge/config.json` (Linux).

## Building

Requirements: Rust toolchain (`1.75+`)

```bash
cargo build --release
```

Output binary: `target/release/bass-bridge.exe` on Windows or `target/release/bass-bridge` on Linux.

## License

Dual-licensed under MIT OR Apache-2.0.
