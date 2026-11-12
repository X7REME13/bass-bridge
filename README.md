# 🔊 BassBridge Pro

**BassBridge Pro** is a high-performance, low-latency native desktop application for Windows (and Linux) that captures system-wide audio in real-time, extracts sub-bass frequencies using an acoustic-grade Linkwitz-Riley 4th-order (LR4) low-pass filter, and routes them to a dedicated secondary audio output (e.g., secondary DAC, soundcard, or USB amplifier) driving subwoofers or chair tactile transducers (Bass Shakers / ButtKickers).

---

## ✨ Features & What's New in v0.2.0

- **System-Wide Loopback Capture:**
  Captures all PC audio (games, Spotify, YouTube, movies, DAW output) via Windows WASAPI Loopback before digital-to-analog conversion (pre-DAC).
- **Linkwitz-Riley 4th-Order (LR4) Low-Pass Filter:**
  Steep -24 dB/octave acoustic slope with exactly -6.02 dB at the crossover frequency, ensuring clean sub-bass separation without vocal or guitar bleed.
- **Massive Volume & Tactile Punch:**
  - **Direct Summing Switch (+6 dB Voltage Boost):** Drives mono subwoofers with full $1.0 \times (L+R)$ power instead of standard $0.5 \times$ halving, doubling signal voltage to chair transducers.
  - **Tape Soft-Saturator / Limiter:** Analog-modeled hyperbolic tangent $\tanh(x)$ soft saturation prevents harsh DAC digital clipping when heavily boosting levels.
  - **Expanded Gain Range:** From -12 dB up to +36 dB with an optimized default gain of +6 dB.
- **Cyber-Studio Dark UI Redesign:**
  - Modern metallic obsidian studio aesthetic (`#0B0E12`) with vibrant Cyan (`#00E5FF`), Amber (`#FF7A00`), and Mint (`#00E676`) accents.
  - **Interactive Frequency Response Bode Plot:** Visualizes the Linkwitz-Riley curve ($20\text{ Hz} - 1000\text{ Hz}$) on a logarithmic frequency axis with real-time crossover cutoff tracking and shaded power area.
  - **Dual Real-time Oscilloscope:** Dual-trace waveform monitor with CRT dark grid (Cyan full-range vs Neon Amber sub-bass).
  - **Hardware-Style LED Peak Meters:** Live dBFS level monitoring with clip warning.
- **Stereo vs. Dual-Mono Routing:**
  - **Dual-Mono Mode:** Symmetrically drives traditional subwoofer cabinets.
  - **Stereo Mode:** Preserves discrete Left & Right channels for dual tactile transducers installed on racing simulator rigs or gaming chairs, delivering spatial haptic rumble (e.g., curb rumbles on the left vs right).
- **Acoustic Delay Compensation (0 to 500 ms):**
  Sample-accurate ring-buffer delay line with dynamic distance readout (e.g. `12.0 ms (~4.1 m / 13.5 ft)`), allowing exact time-alignment with main room speakers.
- **Phase Inversion (0° / 180°):**
  Instantly inverts audio polarity to prevent destructive acoustic phase cancellation between main speakers and subwoofers at the crossover boundary.
- **Automatic Configuration Persistence:**
  Saves all hardware selections and DSP parameters to `%APPDATA%/BassBridge/config.json`.

---

## 🔬 DSP Mechanics & Architecture ("The HOW")

### 1. Linkwitz-Riley 4th-Order (LR4) Low-Pass Filter
A 4th-order Linkwitz-Riley filter is constructed by cascading two identical 2nd-order Butterworth low-pass filters in Direct Form I:

$$H_{\text{LR4}}(s) = \left( H_{\text{Butterworth 2nd}}(s) \right)^2 \quad \text{where } Q = \frac{1}{\sqrt{2}} \approx 0.7071$$

- **Passband:** Maximally flat amplitude response with zero ripple.
- **Cutoff ($f_c$):** Exactly $(-3.01 \times 2) = -6.02\text{ dB}$ attenuation at crossover.
- **Stopband:** Aggressive **-24 dB/octave** slope rejecting speech and mid-range clutter.

### 2. Analog-Modeled Soft Saturation (Limiter)
To allow heavy volume boosting without square-wave DAC clipping, audio samples pass through a continuous non-linear transfer function:

$$y = \begin{cases} x & |x| < 0.65 \\ \tanh(x) & |x| \ge 0.65 \end{cases}$$

This adds subtle, musical even/odd harmonics, saturating peaks smoothly without digital clicks.

---

## 🛠️ Building & Running

### Requirements
- [Rust toolchain](https://rustup.rs/) (1.75+ recommended)
- Windows 10/11 or Linux (ALSA/PulseAudio/PipeWire)

### Build Release
```bash
cargo build --release
```

Binary output: `target/release/bass-bridge.exe` (Windows) or `target/release/bass-bridge` (Linux).

---

## 📄 License
Dual-licensed under MIT OR Apache-2.0.
