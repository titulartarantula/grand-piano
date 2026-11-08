# Grand Piano

A simple, low-latency sampled grand piano for Windows and a USB MIDI keyboard.
Plug in the keyboard, start the app, and play. It finds an M-Audio Oxygen
automatically and maps its wheel, faders and knobs out of the box. Any control
can be re-mapped by clicking a knob's mapping chip and moving the control.

## Sound

- **Samples:** Salamander Grand Piano V3, a Yamaha C5 by Alexander Holm, in the
  FreePats SFZ+FLAC edition `SalamanderGrandPiano-SFZ+FLAC-V3+20200602`.
  It is recorded at 48 kHz/24-bit with **16 velocity layers**, sampled in minor
  thirds. It also has **release samples**: string resonance (3 layers) when the
  dampers fall, mechanical key-release noise on every key-up, and pedal up/down
  noises.
- **License:** Creative Commons Attribution 3.0 (CC BY 3.0). Checked on the
  source page <https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html>
  and in the archive's `readme.txt` on 2026-11-08. CC BY allows redistribution
  and commercial use with attribution. The attribution is in the app's About
  window and in `packaging/LICENSE-SAMPLES.txt`.
- **Download:** the samples aren't bundled (740 MB). On first launch the app
  downloads the archive from FreePats and checks its SHA-256
  (`b7760e168494cf095344e217b0af013fc449ad033abbbdf1c65211cf11dc038b`). It
  unpacks the archive only if the hash matches, into
  `%LOCALAPPDATA%\GrandPiano\samples`. A failed check deletes the download and
  shows an error with a retry button.
- **In memory:** the 641 FLAC files decode to about 1.3 GB of RAM. Each sample is
  normalised and stored as 16-bit. Loading takes a few seconds on a multi-core
  PC.
- Piano in 162 (Ivy Audio) was considered and rejected. It has no release
  samples, and it's only offered by BitTorrent.

### Engine

- Velocity picks one of the 16 layers. Amplitude follows the SFZ's
  `amp_veltrack`. Notes are pitch-shifted at most ±1 semitone, with 4-point
  Hermite interpolation.
- **Sustain (CC64) with half-pedal.** A key that's up decays at a rate set by
  the pedal position: pedal down means natural decay, pedal up means dampers on
  the strings, and in between gives partial damping. The Oxygen's footswitch
  input is on/off, so half-pedal only matters with a continuous pedal or a
  fader mapped to Sustain. Damper-resonance release samples play when the
  dampers land (key up with the pedal up, or the pedal lifted later), attenuated
  by `rt_decay` for how long the note was held. Notes above F#6 have no dampers,
  as on a real piano.
- **Polyphony:** up to 192 voices. Above 160, new notes fade out the least
  important voices over 5 ms, in this order: release noises, then released
  notes, then held notes, quietest first. Re-striking a key lets the previous
  strike ring out over about 120 ms.
- **Output chain:** tone filter → shimmer → room reverb → master volume →
  1 ms look-ahead limiter (ceiling −2 dBFS) → soft clipper. The output never
  clips.

### Controls (all mappable with MIDI learn)

| Control | What it does |
|---|---|
| Volume | Master gain from −48 dB to **+12 dB**. 0 dB is at 80 % of the knob. |
| Tone | Below centre: darker (low-pass from 20 kHz down to 1.2 kHz). Above centre: brighter (+8 dB high shelf at 2.5 kHz). |
| Room | Amount of a warm room reverb (Freeverb-style, 12 ms pre-delay). |
| Shimmer | **The mod wheel.** A stereo chorus whose depth (up to 3.5 ms) and mix (up to 50 %) follow the wheel. Above half-wheel it adds a gentle stereo tremolo (4.2 Hz, ±15 %). At zero it's bypassed. |
| Touch | Fine-tunes the velocity mode (see below). In Fixed mode the knob becomes **Level**. |
| Velocity mode | Normal / **Light** / Fixed switch on the main screen. A mapped button cycles through the modes. |
| Sustain | CC64 by default; a continuous value is supported. |

**Velocity modes:**

- **Normal:** the standard response. Touch at centre is linear; turning it
  right makes soft playing louder (concave curve), turning it left asks for a
  heavier touch.
- **Light (default):** soft playing still gives medium-loud notes. It uses a
  concave curve (γ 0.75 to 0.25 along the Touch knob, 0.5 at default) and a
  raised floor (velocity 32 to 68, 50 at default). For example, a gentle
  velocity of 30 plays as 88.
- **Fixed:** every note plays at one velocity, set by the Level knob
  (default 100).

### Loudness

The output is calibrated so that a medium-velocity chord (C-major triad at
velocity 64) in the default Light mode, with default volume and room, peaks
around −6 dBFS. Measured by the offline render test:

| Mode | Played velocity | → engine | Peak dBFS | RMS dBFS |
|---|---|---|---|---|
| Light | 40 | 93 | −9.8 | −26.5 |
| Light | 64 | 105 | −6.1 | −22.6 |
| Light | 100 | 118 | −2.0 (limiter) | −20.8 |
| Normal | 40 | 40 | −23.7 | −39.7 |
| Normal | 64 | 64 | −17.9 | −33.4 |
| Normal | 100 | 100 | −8.8 | −25.0 |

For more volume, turn Volume up (up to +12 dB). The limiter keeps the output
clean.

## MIDI

- **Ports:** the app auto-connects to any input port whose name contains
  "Oxygen". If there's none, it uses the saved choice, then the first port.
  Ports are rescanned every 1.5 s, so unplugging and replugging works. Settings
  also has a picker and a Rescan button.
- **MIDI learn:** click the small chip under a knob (it reads `CC 7`, or
  `learn`). It turns coral and shows "move a control…". Move a knob, fader,
  wheel or button on the keyboard, and that control now drives the target. The
  new mapping replaces the target's previous one. Right-click a chip to clear
  it. Settings → Controller mapping lists everything and has "Reset to Oxygen 49
  defaults".
- **CC123/CC120** (all notes off) are honoured.

### Oxygen 49 defaults

The Oxygen 49 tested here reports USB ID `0763:3102`, "Oxygen 49". That is the
**4th-generation Oxygen 49 (MKIV)**, whose manual is
`Oxygen49-UserGuide-v1.3.pdf` (factory presets: VIP, Xpand!2, Velvet, Loom,
Vacuum, Transfuser, and General MIDI on presets 7–10). These defaults assume a
General MIDI preset such as preset 10:

| Control on the keyboard | CC | Mapped to | Source |
|---|---|---|---|
| Mod wheel | 1 | Shimmer | user guide (wheel sends CC1 by default) |
| Right-most fader (C9) | 7 | Volume | M-Audio's MKIV programming walkthrough, preset 10 |
| Left-most fader (C1) | 74 | Tone | same walkthrough (left slider sends CC74 on preset 10) |
| Sustain pedal | 64 | Sustain | standard |
| A knob sending CC91 (GM reverb depth) | 91 | Room | GM convention |
| CC71 (resonance) | 71 | Tone | GM convention |
| CC93 (GM chorus depth) | 93 | Shimmer | GM convention |
| CC11 (expression) | 11 | Volume | GM convention |

The user guide doesn't list each knob's CC per preset, so the knob defaults are
GM conventions, not verified against the hardware. If a knob does nothing, use
MIDI learn: it takes two seconds. Settings shows the last CC received.

## Audio and latency

The app uses its own WASAPI backend, written against the Windows API.
(cpal's WASAPI backend only does shared mode at the 10 ms engine period.)

- **Auto (default):** uses low-latency shared mode if the driver really offers
  periods of about 5 ms or less; otherwise exclusive mode; otherwise standard
  shared mode.
- **Shared, low latency:** IAudioClient3 with the smallest engine
  period the driver allows, rounded up to the buffer setting. Other apps keep
  sound. If the driver doesn't offer small periods, it falls back to standard
  shared mode and says so in Settings.
- **Exclusive:** event-driven exclusive mode at the requested buffer size. It
  tries float32, then 24-in-32, 24-bit and 16-bit, at the device rate, 48 kHz
  or 44.1 kHz.
- **Shared, standard:** the most compatible mode, with a 10 ms engine period.
- The header shows the **estimated output latency**: 2 × period + the
  driver-reported stream latency + 1 ms limiter look-ahead. The audio thread
  runs under MMCSS "Pro Audio".
- If the device disappears, the app retries every 2 s and falls back to the
  Windows default output.
- **Example (HDMI TV output):** a typical NVIDIA HDMI driver offers only a
  480-frame (10 ms) shared period, giving about 21 ms in shared mode. **Auto
  therefore picks exclusive mode: 144 frames (3 ms) at 48 kHz, 24-bit, about
  7 ms estimated.** While the app is open, other apps can't play through that
  device; switch to "Shared, standard" in Settings if that matters. A TV's own
  audio processing adds latency the app can't see.
- **ASIO:** not included. It would need Steinberg's SDK at build time.

## Files

- Settings and mappings: `%APPDATA%\GrandPiano\settings.json` (saved within
  2 s of a change and on exit).
- Window position and size: `%APPDATA%\GrandPiano\window.ron`.
- Samples: `%LOCALAPPDATA%\GrandPiano\samples\…`.
- Log: `%LOCALAPPDATA%\GrandPiano\grandpiano.log`. A crash writes `crash.txt`
  next to it.

## Command line

```
GrandPiano.exe --selftest                 list MIDI inputs and audio outputs, open the output in each mode
GrandPiano.exe --download-samples [--archive FILE]   fetch (or use a local copy), verify, unpack
GrandPiano.exe --render-test [OUT.wav]    load the real samples, run the level/clip/click/silence checks, write a WAV
```

Results are printed and also written to `%LOCALAPPDATA%\GrandPiano\selftest.txt`.

## Building

On Linux, cross-compiling to Windows:

```
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin            # needs clang, lld, llvm
cargo xwin build --release --target x86_64-pc-windows-msvc
```

On a small VM, use `-j 1` and `CARGO_PROFILE_RELEASE_LTO=false` to keep memory
down.

Tests (any OS; the engine has no OS dependencies):

```
cargo test --no-default-features                      # unit tests: synthetic instrument
GP_SFZ=/path/to/SalamanderGrandPiano-V3+20200602.sfz \
  cargo test --release --no-default-features --test render -- --nocapture
```

The render test plays a scripted performance: scales, pedalled chords, half
pedal, fast repeats, a pedalled glissando over all 88 keys (voice stealing),
fortissimo clusters and pianissimo notes. It checks for clipping, clicks
(second-difference detector), silence while keys are held, and that the tail
decays. It writes `target/render-test.wav`.

## Install (per-user, no admin)

Unzip the release folder, then run:

```
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

This installs to `%LOCALAPPDATA%\Programs\GrandPiano` and adds a Start menu
shortcut called "Grand Piano". `uninstall.ps1` removes it; `-All` also removes
the settings and samples.

## Code layout

| File | Contents |
|---|---|
| `src/engine.rs` | voices, damper and pedal model, release triggers, stealing, output chain |
| `src/library.rs`, `src/sfz.rs` | SFZ parsing, FLAC/WAV decoding, lookup tables |
| `src/dsp.rs` | tone filter, shimmer, reverb, limiter |
| `src/params.rs`, `src/mapping.rs` | lock-free shared parameters, velocity curves, MIDI learn |
| `src/settings.rs`, `src/download.rs` | JSON settings, checksum-verified sample download |
| `src/analysis.rs` | offline render and audio checks |
| `src/audio_win.rs`, `src/midi.rs`, `src/ui.rs`, `src/main.rs` | Windows app: WASAPI, midir, egui |

## License

The code is released under the [MIT License](LICENSE).

The piano samples are **not** part of this repository and are not covered by the
MIT License. The app downloads the Salamander Grand Piano V3 by Alexander Holm,
licensed under Creative Commons Attribution 3.0 (see
`packaging/LICENSE-SAMPLES.txt`).
