# 🔊 BassBridge

**BassBridge** is a lightweight, low-latency native desktop application for Windows (and Linux) that captures system-wide audio in real-time, extracts sub-bass frequencies using a Linkwitz-Riley 4th-order (LR4) low-pass filter, and routes them to a dedicated secondary audio output (e.g., secondary DAC, soundcard, or USB amplifier) driving subwoofers or chair tactile transducers (Bass Shakers / ButtKickers).

---

## ✨ Features

- **System-Wide Loopback Capture:**
  Captures all PC audio (games, Spotify, YouTube, movies, DAW output) via Windows WASAPI Loopback or default system audio.
- **Linkwitz-Riley 4th-Order (LR4) Low-Pass Filter:**
  Steep -24 dB/octave acoustic slope with exactly -6.02 dB at the crossover frequency, ensuring clean sub-bass separation without high-frequency bleed or muddy resonance.
- **Stereo vs. Dual-Mono Routing:**
  - **Dual-Mono Mode:** Sums `(L + R) * 0.5` to eliminate phase cancellation and symmetrically drive traditional subwoofer cabinets.
  - **Stereo Mode:** Preserves discrete Left & Right channels for dual tactile transducers installed on racing simulator rigs or gaming chairs, delivering spatial haptic rumble (e.g., curb rumbles on the left vs right).
- **Delay Compensation (0 to 500 ms):**
  Sample-accurate ring-buffer delay line for time-aligning subwoofer acoustic arrival with main room speakers, or compensating for physical body conduction latency on tactile transducers.
- **Gain Trim (-30 dB to +30 dB):**
  Fine-grained decibel trimming to balance subwoofer output levels against main speakers without touching Windows master volume.
- **Phase Inversion (0° / 180°):**
  Instantly inverts audio polarity to prevent destructive acoustic phase cancellation between main speakers and subwoofers at the crossover boundary.
- **DSP Bypass Toggle:**
  Instantly audition full-range vs. low-pass filtered audio for real-time calibration.
- **Live Oscilloscope Monitor:**
  Dual-waveform real-time display comparing the captured full-range input signal (Cyan) against the processed sub-bass output (Neon Orange) at 60 FPS.
- **Linear Resampler:**
  Ultra-low latency sample rate conversion handling mismatches between system render rate (e.g., 44.1 kHz / 48 kHz / 96 kHz) and secondary DAC hardware sample rate.
- **Automatic Configuration Persistence:**
  Saves all hardware selections and DSP parameters to `%APPDATA%/BassBridge/config.json` (or `~/.config/BassBridge/config.json` on Linux) on any parameter adjustment.

---

## 🔬 DSP Mechanics & Architecture ("The HOW")

### 1. WASAPI Loopback Capture
In Windows audio architecture, the Windows Audio Session API (WASAPI) allows an application to open a client in `AUDCLNT_STREAMFLAGS_LOOPBACK` mode on the default render endpoint. Instead of capturing physical microphone input, WASAPI captures the digital audio stream rendered by all system applications *before* it reaches the digital-to-analog converter (DAC).

### 2. Linkwitz-Riley 4th-Order (LR4) Low-Pass Filter
A 4th-order Linkwitz-Riley filter is implemented by cascading two identical 2nd-order Butterworth low-pass filters in Direct Form I:

$$H_{\text{LR4}}(s) = \left( H_{\text{Butterworth 2nd}}(s) \right)^2$$

Where each stage has a quality factor $Q = \frac{1}{\sqrt{2}} \approx 0.7071$:
- **Passband:** Fully flat (maximally flat amplitude response in the Butterworth stages).
- **At Cutoff ($f_c$):** Each stage provides -3.01 dB attenuation, yielding an exact $(-3.01) \times 2 = -6.02 \text{ dB}$ attenuation at crossover.
- **Stopband:** Rolloff slope of **-24 dB/octave** (-80 dB/decade), aggressively rejecting vocal and midrange frequencies from leaking into tactile transducers or subwoofers.
- **Phase Alignment:** The phase lag at $f_c$ is $90^\circ \times 2 = 180^\circ$. When acoustically combined with high-pass filtered mains or matching natural acoustic room rolloff, it produces a flat frequency response without crossover peaking.

### 3. Direct Form I Biquad Transfer Function
Each biquad filter evaluates the standard discrete-time difference equation:

$$y[n] = b_0 x[n] + b_1 x[n-1] + b_2 x[n-2] - a_1 y[n-1] - a_2 y[n-2]$$

Normalized such that $a_0 = 1$. The frequency response $H(e^{j\omega})$ is evaluated analytically:

$$|H(e^{j\omega})|^2 = \frac{(b_0 + b_1 \cos\omega + b_2 \cos 2\omega)^2 + (b_1 \sin\omega + b_2 \sin 2\omega)^2}{(1 + a_1 \cos\omega + a_2 \cos 2\omega)^2 + (a_1 \sin\omega + a_2 \sin 2\omega)^2}$$

### 4. Circular Delay Line Buffer
A circular ring buffer stores audio history:

$$\text{delay\_samples} = \left\lfloor \frac{\text{delay\_ms} \times \text{sample\_rate}}{1000} \right\rceil$$

Delaying the subwoofer signal allows matching the slower sound wave propagation from a distant subwoofer to the listener's ears, achieving sub-millisecond phase coherence with nearfield desktop monitors.

---

## 🛠️ Building & Running

### Requirements
- [Rust toolchain](https://rustup.rs/) (1.75+ recommended)
- Windows (Windows 10/11 with WASAPI audio) or Linux (ALSA/PulseAudio/PipeWire)

### Build in Release Mode
```bash
cargo build --release
```

### Run
```bash
cargo run --release
```

The resulting binary will be located at `target/release/bass-bridge.exe` (Windows) or `target/release/bass-bridge` (Linux).

---

## 📄 License
Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
