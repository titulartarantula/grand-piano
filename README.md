# Grand Piano

A simple, low-latency sampled grand piano for Windows and a USB MIDI keyboard,
with two real-time synths and a metronome. Plug in the keyboard, start the app,
and play. It finds an M-Audio Oxygen automatically and maps its wheel, faders
and knobs out of the box. Any control can be re-mapped by clicking a knob's
mapping chip and moving the control.

## Instruments

The selector at the top of the main screen switches instruments; a mapped
button steps through them (MIDI learn: the chip under the selector). The
choice is saved. Notes that are sounding when you switch ring out naturally.

| Instrument | What it is | Presets |
|---|---|---|
| **Grand Piano** (default) | The sampled Salamander grand, unchanged. | — |
| **Mono Synth (Moog-style)** | A real-time subtractive lead/bass synth in the style of the classic 1970s monosynths: 3 oscillators, 24 dB/oct ladder filter, glide. | Bass, Lead, Poly |
| **String Machine** | A real-time, fully polyphonic divide-down string ensemble in the style of late-1970s string synthesizers, with ensemble chorus and (on one preset) a phaser. | Strings, Phased, Dark |

The synths are generated in real time: no samples are downloaded for them, and
everything about them is covered by this repository's MIT license. "Moog-style"
and similar names describe a kind of instrument; this project has no connection
with Moog Music, ARP or any other manufacturer, and their names are trademarks
of their owners.

### Mono Synth (Moog-style)

- **Oscillators:** 3 per voice, band-limited (PolyBLEP saw and square, PolyBLAMP
  triangle), slightly detuned against each other, one an octave down (Bass) or up
  (Lead).
- **Filter:** a 4-pole, 24 dB/oct resonant ladder low-pass in zero-delay-feedback
  form with tanh saturation at its input. It stays stable at maximum resonance,
  where it self-oscillates. It has key tracking and partial bass-loss compensation.
- **Envelopes:** an ADSR for the filter and one for the amp, analogue-style (RC
  attack, exponential decay and release, retrigger from the current level).
- **Playing:** Bass and Lead are mono, last-note priority, legato (holding a key
  while you press the next doesn't retrigger the envelopes; releasing the top key
  falls back to the one still held) with glide. Poly is 8 voices; glide is off there.
- **Presets:** *Bass* (punchy, saw+saw+square sub, short filter decay), *Lead*
  (saw+square+saw octave, glide on), *Poly* (two detuned saws + triangle sub,
  8 voices).

### String Machine

- **Divide-down oscillators:** twelve top-octave clocks run all the time, and
  every key reads its phase from its clock divided by 2, 4, 8 ..., so octaves are
  phase-locked and a key press opens a gate on an already-running oscillator, as
  on the originals. Each key mixes two footages (16'/8'/4') of band-limited saw.
- **Per-key attack/release**, a shared low-pass (Tone) and a high-pass that thins
  the low end.
- **Ensemble:** three modulated delay lines, slow (0.6 Hz) plus fast (6 Hz) LFOs
  at 120° offsets, spread to stereo. **Phaser** on the *Phased* preset: six
  all-pass stages with feedback, swept at 0.3 Hz.
- **Presets:** *Strings* (8'+4', bright ensemble), *Phased* (8'+4' through the
  phaser, the classic post-punk string-synth lead sound), *Dark* (16'+8', slower
  attack, lower cutoff).
- Velocity changes the level gently (about 8 dB soft to hard); the original
  machines weren't touch-sensitive.

### Controls per instrument

| Control (default CC) | Grand Piano | Mono Synth | String Machine |
|---|---|---|---|
| Volume (CC7, CC11) | master volume | same | same |
| Tone (CC74, CC71) | tone (darker / brighter) | **filter cutoff** (40 Hz–20 kHz) | **filter cutoff** |
| Mod wheel (CC1, CC93) | Shimmer (chorus + tremolo) | **vibrato** (5.6 Hz, up to ±0.6 semitone) | **vibrato** (bends the whole machine, like the originals) |
| Room (CC91) | room reverb | same | same |
| Touch / Level | velocity curve | same | same |
| Velocity mode | Normal / Light / Fixed | same: velocity sets level and filter-envelope depth | same: velocity sets level |
| Octave −/+, Transpose | shift incoming notes | same | same |
| Sustain (CC64) | pedal with half-pedal | holds notes | holds notes |
| Pitch wheel | ignored (can be allowed in Settings) | ±2 semitones | ±2 semitones |
| Resonance | — | ladder resonance (top = self-oscillation) | — |
| Glide | — | portamento, 0–1.2 s (mono presets) | — |

Each synth keeps its own cutoff, and the piano's Tone and Shimmer settings are
untouched while a synth is selected (on the synths the piano's tone filter and
shimmer are bypassed). Choosing a preset loads its cutoff, resonance and glide,
like recalling a patch. Resonance, Glide, Instrument and Preset have MIDI-learn
chips too.

### Pitch bend

The Oxygen's spring-loaded pitch wheel sends MIDI pitch bend (14-bit; centre
8192 is no bend). The range is set per instrument in **Settings → Pitch bend**,
0–12 semitones up and down separately, ±2 by default, with a one-click "up 2 /
down 12" option. The synths smooth the wheel per sample (about 3 ms), so there's
no zipper noise; on the mono synth the bend rides on top of glide. The Grand
Piano ignores the wheel unless "Allow" is ticked there (it then bends the playback
rate, for fun). A small BEND meter under the sustain bar shows the wheel.

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
| Volume | Master gain with a smooth taper: 25 % ≈ −25 dB, 50 % = −12 dB, **80 % = 0 dB** (default), 100 % = **+6 dB**. It gets gentler towards the top and fades smoothly to silence at the bottom; neighbouring MIDI values are at most ~1.4 dB apart above CC 10. |
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

For more volume, turn Volume up (up to +6 dB). The limiter keeps the output
clean. Fader positions, same triad (Light, velocity 64):

| Fader | Gain | Piano peak / RMS | Mono Bass peak / RMS | Strings peak / RMS |
|---|---|---|---|---|
| 25 % | −24.8 dB | −30.9 / −47.3 | −35.3 / −42.8 | −30.7 / −44.9 |
| 50 % | −12.0 dB | −18.1 / −34.6 | −22.5 / −30.1 | −17.9 / −32.1 |
| 80 % | 0 dB | −6.1 / −22.6 | −10.5 / −18.1 | −5.9 / −20.1 |
| 100 % | +6.0 dB | −2.0 (limiter) / −17.4 | −4.5 / −12.1 | −2.0 (limiter) / −14.5 |

Synth levels (first second, default volume and room; Bass plays C3, Lead C4, the
others the C4-E4-G4 triad), peak / RMS dBFS:

| Preset | Light 40 | Light 64 | Light 100 | Normal 40 | Normal 64 | Normal 100 |
|---|---|---|---|---|---|---|
| Mono Synth / Bass | −11.6 / −19.2 | −10.5 / −18.1 | −9.3 / −16.9 | −16.2 / −23.7 | −14.3 / −21.8 | −11.0 / −18.5 |
| Mono Synth / Lead | −11.7 / −21.0 | −10.6 / −19.9 | −9.3 / −18.7 | −16.2 / −25.5 | −14.3 / −23.7 | −11.0 / −20.4 |
| Mono Synth / Poly | −12.4 / −22.5 | −11.2 / −21.4 | −10.0 / −20.2 | −17.0 / −27.2 | −15.0 / −25.2 | −11.7 / −21.9 |
| String Machine / Strings | −6.5 / −20.7 | −5.9 / −20.1 | −5.4 / −19.6 | −9.6 / −23.8 | −8.1 / −22.3 | −6.2 / −20.4 |
| String Machine / Phased | −8.1 / −20.6 | −7.5 / −20.0 | −6.9 / −19.5 | −11.2 / −23.7 | −9.6 / −22.1 | −7.7 / −20.3 |
| String Machine / Dark | −5.6 / −20.0 | −5.0 / −19.5 | −4.4 / −18.9 | −8.6 / −23.1 | −7.1 / −21.6 | −5.2 / −19.7 |

The synth render test also plays a 25-second workout through every preset (bass
line, legato with glide, fast repeats, a cutoff sweep with resonance at maximum
and full vibrato, the lowest and highest keys, pedalled chords, a ten-note
fortissimo chord, quiet notes) and checks for clipping, clicks, NaN/denormal
samples and that everything stops. Each preset renders in about 1 % of real time
on one core.

## Octave, transpose and the keyboard's range

- **Octave −/+** (main screen, above the on-screen keyboard; −3 to +3) and
  **Transpose** (Settings, ±12 semitones) shift every note that comes from the
  MIDI keyboard. Both are exact and saved. Octave − and + can each be mapped to a
  button with MIDI learn. The app remembers which note each held key started, so
  changing the octave while holding a key still releases the right note.
- **The keyboard's own range (inferred).** The Oxygen 49's Octave buttons shift
  the note numbers inside the keyboard and send no MIDI message of their own (the
  user guide describes them only as shifting the keyboard's range), so the app
  can't be told about them. Instead it watches the notes that arrive: it starts
  from the default span C2–C6 (MIDI 36–84) and, when a note falls outside the
  current span, moves to the nearest octave that holds the notes of the last few
  seconds. The on-screen keyboard (all 88 keys) shows that span with an amber bar
  and dims the keys outside it; the label reads "Oct +1 (detected)", or "assumed"
  until you've played something.

## Metronome

The panel on the right of the main screen.

- **Start/Stop:** the big button (off at launch). Mappable: the chip next to it.
- **Tempo:** the knob, 30–240 BPM (default 100), shown large. Mappable. **TAP**
  sets the tempo from your taps (mappable too): tap two or more times; a pause
  of over 2 s starts over.
- **Click:** on/off. A short, clean sine "pip": 2 kHz on the downbeat, 1.3 kHz on
  the other beats. **Level** sets its volume. It's mixed after the instrument's
  tone, chorus, reverb and limiter, so they don't colour it, and the sum is
  soft-limited so it can't clip.
- **Blink:** on/off. An amber light pulses on every beat, brighter on the
  downbeat, and dots show the position in the bar.
- **Beats per bar:** 2–6 (default 4) with − and +.

Timing comes from the audio thread, not the screen: beats are scheduled
sample-accurately with integer arithmetic (beat *k* falls exactly on sample
⌈k · 60 · rate / BPM⌉, so there's no drift however long it runs). A tempo change
finishes the current beat at the new tempo, with no doubled or skipped clicks.
The light is driven by the same beats, timestamped for when they leave the
speakers (output latency included), so it stays in time with the click.
Everything except running/stopped is saved.


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
GrandPiano.exe --render-test [OUT.wav]    load the real samples, run the level/clip/click/silence checks, write a WAV;
                                          also renders and checks every synth preset
GrandPiano.exe --synth-test               synth checks only (no samples or audio device needed)
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
cargo test --no-default-features                      # unit tests + synth render test
GP_SFZ=/path/to/SalamanderGrandPiano-V3+20200602.sfz \
  cargo test --release --no-default-features --test render -- --nocapture
cargo test --release --no-default-features --test synth_render -- --nocapture
```

Unit tests cover, among others: glide and legato logic, last-note priority,
filter stability at maximum resonance, band-limited oscillators, no NaN or
denormal output, divide-down phase locking, pitch bend (centre and both
extremes), the volume taper, metronome timing over 10 minutes at several tempos
and sample rates (zero drift), tempo changes mid-beat, and the downbeat accent.

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
shortcut called "Grand Piano" with the app icon. If Grand Piano is running, the
installer stops and asks you to close it first (`-Force` closes it for you).
Settings and samples are kept. `uninstall.ps1` removes it; `-All` also removes
the settings and samples.

## Code layout

| File | Contents |
|---|---|
| `src/engine.rs` | voices, damper and pedal model, release triggers, stealing, output chain |
| `src/library.rs`, `src/sfz.rs` | SFZ parsing, FLAC/WAV decoding, lookup tables |
| `src/dsp.rs` | tone filter, shimmer, reverb, limiter |
| `src/synth.rs` | oscillators, ladder filter, envelopes, the mono synth and its presets |
| `src/strings.rs` | the string machine: divide-down oscillators, ensemble, phaser |
| `src/metronome.rs` | beat clock and click |
| `src/params.rs`, `src/mapping.rs` | lock-free shared parameters, velocity curves, MIDI learn |
| `src/settings.rs`, `src/download.rs` | JSON settings, checksum-verified sample download |
| `src/analysis.rs` | offline render and audio checks |
| `src/audio_win.rs`, `src/midi.rs`, `src/ui.rs`, `src/main.rs` | Windows app: WASAPI, midir, egui |

## Icon

The app icon (`assets/icon.png`, `assets/icon.ico`) was generated with OpenAI's
image model for this project, then cleaned up (edge fringing removed) and
converted into a multi-size .ico with `packaging/make-icon.py`. It is covered by
this repository's MIT license. The build embeds it in the exe (`build.rs`,
`assets/app.rc`).

## License

The code and the icon are released under the [MIT License](LICENSE).

The piano samples are **not** part of this repository and are not covered by the
MIT License. The app downloads the Salamander Grand Piano V3 by Alexander Holm,
licensed under Creative Commons Attribution 3.0 (see
`packaging/LICENSE-SAMPLES.txt`).
