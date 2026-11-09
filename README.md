# Grand Piano

A simple, low-latency keyboard instrument for Windows and a USB MIDI keyboard.
Plug in the keyboard, start the app, and play. It finds an M-Audio Oxygen
automatically and maps its wheel, faders and knobs out of the box.

**What's in it**

- **Six instruments:** a sampled **Grand Piano** (Salamander, 16 velocity
  layers) and five generated in real time with no samples: a **Moog-style
  Synth**, a **String Machine**, an **Electric Piano** (tine and reed), an
  **Organ** (tonewheels with a rotating speaker) and a **Polysynth**
  (Juno-, Prophet- and Oberheim-style voices). Each has its own presets
  and remembers its own settings.
- **Velocity modes:** Normal, Light (soft playing still sounds full) and
  Fixed.
- **Metronome** with a sample-accurate click, tap tempo and a beat light.
- **Pitch bend** with a range per instrument, and **octave shift** /
  transpose.
- **MIDI learn** for every knob and switch: click a control's chip and move
  a knob, fader, wheel or button on the keyboard.
- Native WASAPI audio (exclusive or low-latency shared mode), a look-ahead
  limiter, and settings saved automatically.

## Instruments

The selector at the top of the main screen switches instruments. A mapped
button steps through all six in order and wraps around (MIDI learn: the
chip next to the selector). The choice is saved. Notes that are sounding when
you switch ring out naturally. Instruments with presets show them in a second
row. A mapped button steps through the presets, and choosing a preset loads
its sound settings, like recalling a patch.

| Instrument | What it is | Presets |
|---|---|---|
| **Grand Piano** (default) | The sampled Salamander grand. | — |
| **Moog-style Synth** | Subtractive synth in the style of the classic 1970s Moog-type synths: 3 oscillators, 24 dB/oct ladder filter. Plays chords (Poly, the default) or one note with legato and glide (Mono). | Bass, Lead, Poly |
| **String Machine** | Fully polyphonic divide-down string ensemble in the style of late-1970s string synthesizers, with ensemble chorus and (on one preset) a phaser. | Strings, Phased, Dark |
| **Electric Piano** | Physically inspired tine (Rhodes-style) and reed (Wurlitzer-style) electric piano: pickup "bark" when played hard, hammer attack, damper thump, tremolo. | Suitcase, Bright Tines, Wurli |
| **Organ** | Tonewheel organ in the style of the B3: 91 tonewheels, 9 drawbars, key click, single-triggered percussion, overdrive, and a Leslie-style rotating speaker. | Jazz, Gospel, Rock, Ballad |
| **Polysynth** | 10-voice analogue-style polysynth with three voice types: Juno-style (PWM, sub, built-in chorus), Prophet-style (two oscillators, poly-mod) and Oberheim-style (fat detune, 12 dB filter). | 80s Pad, Brass Stab, Synthwave Lead, Juno Strings, OB Brass |

Everything except the piano is generated in real time: no samples are
downloaded for these instruments, and everything about them is covered by this
repository's MIT license. Names such as Moog, Rhodes, Wurlitzer, Hammond, B3,
Leslie, Juno, Prophet and Oberheim only describe the kind of instrument each
one imitates. This project has no connection with those companies, and the
names are trademarks of their owners.

### Moog-style Synth

(Called "Mono Synth" before 2026-11-08. Older settings files still load, with Voices set to Poly: Bass and Lead used to be mono-only.)

- **Oscillators:** 3 per voice, band-limited (PolyBLEP saw and square, PolyBLAMP
  triangle), slightly detuned against each other, one an octave down (Bass) or up
  (Lead).
- **Filter:** a 4-pole, 24 dB/oct resonant ladder low-pass in zero-delay-feedback
  form with tanh saturation at its input. It stays stable at maximum resonance,
  where it self-oscillates. It has partial bass-loss compensation and per-voice
  **keyboard tracking** (75 % on Bass and Poly, 80 % on Lead): each note's cutoff
  follows its key, so in a chord the upper notes stay bright instead of being
  buried under a cutoff set for the bass.
- **Envelopes:** an ADSR for the filter and one for the amp, analogue-style (RC
  attack, exponential decay and release, retrigger from the current level).
- **Voices: Poly / Mono.** A switch in the preset row (shown only for this
  synth; it has a MIDI-learn chip, so a button toggles it). It works with every
  preset and is saved.
  - **Poly** (default): up to 8 notes, so chords work on every preset. Each voice
    has a fixed gain that doesn't depend on how many notes are held, so adding a
    note never makes the others duck. Glide is off.
  - **Mono:** one voice, legato (holding a key while you press the next doesn't
    retrigger the envelopes; releasing the sounding key falls back to one still
    held) with glide. **Settings → Moog-style Synth → Note priority** picks which
    held key sounds: **Last** (default, the newest), **High** or **Low**.
- **Analogue drift:** each note is tuned up to ±1.5 cents off, at random. Without
  it, octave-related notes (C2 and C4) have harmonics at exactly the same
  frequency, locked at whatever phase they started with, and can partly cancel
  for as long as the chord is held; with it they beat slowly, as on real
  analogue synths.
- **Presets:** *Bass* (punchy, saw+saw+square sub, short filter decay), *Lead*
  (saw+square+saw octave, glide on), *Poly* (two detuned saws + triangle sub,
  slower envelopes). Choosing a preset doesn't change the Voices switch.

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

### Electric Piano

A model of how tine and reed electric pianos make their sound, not a sample
playback.

- **Tine / reed:** each note is a decaying sinusoid at the key's pitch with a
  two-stage, key-dependent decay. Low notes ring for many seconds and the top
  octave for about one to two.
- **Pickup:** the tine's swing goes through a fixed, asymmetric degree-5
  polynomial, as a magnetic pickup voiced slightly off-centre would. The
  harder you play, the further the tine swings and the more harmonics the
  pickup adds: soft notes are round and bell-like, and hard ones **bark**.
  Velocity changes the timbre much more than the level. A unit test checks
  that the 2nd to 5th harmonics rise steadily with velocity on every preset,
  by more than 10 dB from soft to hard. Because the polynomial is degree 5,
  nothing above the 5th harmonic is generated, and harmonics that would pass
  0.45 × the sample rate are dropped per note, so it doesn't alias.
- **Attack:** a hammer thud (a 12 ms burst of filtered noise, brighter when
  played harder) and the tine's or reed's inharmonic bending mode, which dies
  away in a fraction of a second. *Bright Tines* adds two quickly decaying
  high partials for a 1980s FM-piano sparkle.
- **Dampers:** releasing a key with the pedal up damps the note over
  60–250 ms (faster at the top) and adds a soft, low **damper thump**. The
  sustain pedal holds the dampers off. Up to 32 notes sound at once (a 33rd
  fades the least important one out over 5 ms; re-striking a key cross-fades
  the old strike over 30 ms).
- **Tremolo:** stereo autopan (the left and right sides swap level, like a
  suitcase amp) on *Suitcase* and *Bright Tines*, and mono tremolo on *Wurli*.
  **The mod wheel sets the depth.**
- **Tone** is brightness: it opens a low-pass (1.2 to 20 kHz) and pushes the
  pickup harder.
- **Presets:**
  - *Suitcase*: warm and round, barks when you dig in, with stereo autopan.
  - *Bright Tines*: bell-like attack and sparkle, for ballads and 80s pop.
  - *Wurli*: reedy and nasal, growls when played hard, with mono tremolo.

### Organ

- **Tonewheel generator:** 91 tonewheels that run all the time, tuned with the
  original generator's 12 gear ratios (wheel frequency = 20 rev/s × ratio ×
  teeth), so the organ is very slightly off equal temperament, as the real one
  is: every wheel is within 1.5 cents. Each key's **nine drawbars** (16', 5⅓',
  8', 4', 2⅔', 2', 1⅗', 1⅓', 1') tap wheels at fixed intervals and fold back at
  the top of the range. Drawbar steps are about 3 dB, and the more drawbars are
  out, the quieter each one gets ("drawbar loading").
- **Leakage:** every sounding wheel bleeds a little (about −40 dB) into its
  octave neighbours and the wheel sharing its generator compartment.
- **Key click:** a short band-passed noise burst when a key goes down (softer on
  release). The wheels' contacts close in about 2 ms.
- **Percussion:** 2nd or 3rd harmonic, soft or normal, fast (about 0.5 s) or slow
  (about 2 s). It's **single-triggered** as on the original: it fires only when
  a key is pressed with no other key held, so legato playing doesn't retrigger
  it. With percussion on, the 1' drawbar goes silent (the original borrows its
  contact), and at Normal volume the drawbars drop about 3 dB.
- **No velocity:** organ keys are on/off. The velocity modes don't change the
  organ, and the main screen says so while it's selected. A unit test checks
  that every mode and velocity gives bit-identical output.
- **Overdrive:** a cubic soft-clipping preamp. **Tone is drive and
  brightness.** One key at a low setting stays clean, while chords and higher
  settings break up. A cubic only adds up to the 3rd harmonic and the organ's
  own harmonics stop near 6 kHz, so it doesn't alias.
- **Rotating speaker (Leslie-style):** an 800 Hz split into a treble horn and a
  bass drum, each spinning. Each rotor's angle drives a modulated delay (Doppler
  pitch wobble) and its loudness towards two virtual microphones 90° apart, so
  the sound swirls in stereo. **Chorale** (slow: horn 0.83 Hz, drum 0.67 Hz) and
  **tremolo** (fast: horn 6.7 Hz, drum 5.7 Hz), with inertia: the light horn
  gets up to speed in about a second, while the heavy drum takes several
  seconds to speed up and slow down. The rotors keep turning when nothing
  plays.
- **Leslie control:** **the mod wheel switches it**: below half is slow, half and
  above is fast. The **Slow/Fast switch** in the preset row has a MIDI-learn
  chip, so a button toggles it too. The wheel only switches the speed when it
  crosses the middle, so a resting wheel doesn't fight the button. The speed is
  saved.
- **Drawbars and percussion** are editable in the panel on the main screen:
  drag or scroll a bar (pulled down = louder, coloured as on the original), or
  double-click it to return to the preset's value. The percussion switches (Perc,
  3rd, Soft, Fast) are under the bars. Choosing a preset loads its drawbars
  and percussion. Your changes are saved.
- **Pitch bend is off** by default (the original has none); set a range in
  Settings → Pitch bend if you want it. The sustain pedal holds notes, for
  convenience (the original console has no sustain pedal).
- **Presets:**
  - *Jazz*: `888000000`, the classic jazz-trio sound.
  - *Gospel*: `888800000` with fast, normal-volume 3rd-harmonic percussion.
  - *Rock*: `888888888`, everything out and driven hard.
  - *Ballad*: `008800000`, soft flutes, clean.

### Polysynth

Ten voices built from the Moog-style Synth's parts (PolyBLEP oscillators,
zero-delay-feedback filters, analogue-style envelopes), with three voice
architectures:

- **Juno-style:** one DCO with a band-limited saw and a pulse whose width an LFO
  sweeps (PWM), a square sub-oscillator an octave down, a high-pass filter, a
  24 dB/oct resonant low-pass, and the built-in **stereo bucket-brigade chorus**.
  The chorus has two modes: I (0.5 Hz, gentle) and II (0.83 Hz, deeper). It
  uses one delay line with two taps swept in opposite directions, one per side,
  and the BBD's high-frequency loss.
- **Prophet-style:** two oscillators with **poly-mod**: the filter envelope bends
  oscillator A's pitch, and oscillator B modulates the filter cutoff at audio
  rate, for a slightly gritty, vocal edge. 24 dB/oct low-pass.
- **Oberheim-style:** two saws detuned ±9 cents (fat) through a **12 dB/oct**
  state-variable filter, the SEM's softer, rounder slope.
- **Voice allocation:** a new note takes the same key's voice, then a free one,
  then the quietest released voice, then the oldest held one. Envelopes restart
  from their current level, so a steal doesn't click.
- **Filter keyboard tracking** of 60–80 % per preset, and ±1.5 cents of random
  drift per note (as on the Moog-style Synth).
- **Tone is cutoff**, the **Resonance** knob is shown (it's separate from the
  Moog-style Synth's), and **the mod wheel is vibrato on every preset** (5.6 Hz,
  up to ±0.6 semitone). The Juno presets' PWM runs on its own LFO.
- **Presets:**
  - *80s Pad* (Juno-style: saw + PWM pulse + sub, slow attack and release,
    chorus II): wide, warm pads.
  - *Brass Stab* (Prophet-style: two saws with a snappy filter envelope and a
    touch of poly-mod): punchy stabs.
  - *Synthwave Lead* (Prophet-style: saw + square an octave up, resonant, a
    little poly-mod pitch blip, chorus I): leads and arpeggios.
  - *Juno Strings* (Juno-style: PWM pulse ensemble through chorus I, thin
    high-pass): string-synth pads.
  - *OB Brass* (Oberheim-style: fat detuned saws, 12 dB filter, slower brass
    swell): big brass and pads.

### Controls per instrument

| Control (default CC) | Grand Piano | Moog-style Synth | String Machine | Electric Piano | Organ | Polysynth |
|---|---|---|---|---|---|---|
| Volume (CC7, CC11) | master volume | same | same | same | same | same |
| Tone (CC74, CC71) | tone (darker / brighter) | **filter cutoff** (40 Hz–20 kHz) | **filter cutoff** | **brightness** | **drive** (overdrive + brightness) | **filter cutoff** |
| Mod wheel (CC1, CC93) | Shimmer (chorus + tremolo) | **vibrato** (5.6 Hz, up to ±0.6 semitone) | **vibrato** (bends the whole machine, like the originals) | **tremolo depth** (stereo autopan on Suitcase / Bright Tines, mono on Wurli) | **Leslie slow / fast** (below / above half) | **vibrato** |
| Room (CC91) | room reverb | same | same | same | same | same |
| Touch / Level | velocity curve | same | same | same | — (no velocity) | same |
| Velocity mode | Normal / Light / Fixed | velocity sets level and filter-envelope depth | velocity sets level | velocity sets **timbre** (bark) and level | ignored: keys are on/off | velocity sets level and filter-envelope depth |
| Octave −/+, Transpose | shift incoming notes | same | same | same | same | same |
| Sustain (CC64) | pedal with half-pedal | holds notes | holds notes | lifts the dampers | holds notes | holds notes |
| Pitch wheel | ignored (can be allowed in Settings) | ±2 semitones | ±2 semitones | ±2 semitones | **off** (set a range in Settings) | ±2 semitones |
| Resonance | — | ladder resonance (top = self-oscillation) | — | — | — | filter resonance (its own setting) |
| Glide | — | portamento, 0–1.2 s (Mono voices only) | — | — | — | — |
| Switch in the preset row | — | Voices: Poly / Mono (button toggles) | — | — | Leslie: Slow / Fast (button toggles) | — |
| Panel | — | — | — | — | drawbars and percussion | — |

Each instrument keeps its own Tone setting, and the piano's Tone and Shimmer
settings are untouched while another instrument is selected (the piano's tone
filter and shimmer are bypassed there). The mod wheel position is shared,
because there's one wheel. Resonance, Glide, Voices, Leslie, Instrument and
Preset have MIDI-learn chips too.

### Pitch bend

The Oxygen's spring-loaded pitch wheel sends MIDI pitch bend (14-bit; centre
8192 is no bend). The range is set per instrument in **Settings → Pitch bend**,
0–12 semitones up and down separately. The default is ±2, except on the organ,
where it's off (0). There's a one-click "up 2 / down 12" option. The real-time instruments smooth the wheel per sample (about 3 ms), so there's
no zipper noise; on the Moog-style Synth in Mono the bend rides on top of glide. The Grand
Piano ignores the wheel unless "Allow" is ticked there (it then bends the playback
rate, for fun). A small BEND meter under the sustain bar shows the wheel.

## Piano: samples and engine

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

| Fader | Gain | Piano | Moog-style Bass (Poly) | Strings | Electric Piano / Suitcase | Organ / Jazz | Polysynth / 80s Pad |
|---|---|---|---|---|---|---|---|
| 25 % | −24.8 dB | −30.9 / −47.3 | −36.2 / −45.9 | −30.7 / −44.9 | −31.1 / −44.8 | −33.0 / −43.9 | −34.2 / −46.3 |
| 50 % | −12.0 dB | −18.1 / −34.6 | −23.4 / −33.1 | −17.9 / −32.1 | −18.3 / −32.0 | −20.3 / −31.1 | −21.5 / −33.5 |
| 80 % | 0 dB | −6.1 / −22.6 | −11.4 / −21.1 | −5.9 / −20.1 | −6.3 / −20.0 | −8.3 / −19.1 | −9.5 / −21.5 |
| 100 % | +6.0 dB | −2.0 (limiter) / −17.4 | −5.4 / −15.1 | −2.0 (limiter) / −14.5 | −2.0 (limiter) / −14.5 | −2.3 / −13.1 | −3.5 / −15.5 |

(peak / RMS dBFS. The organ is matched on loudness rather than peak: a sustained
organ chord has a much smaller peak-to-RMS ratio than a struck piano chord, so
its RMS sits close to the piano's while its peak is a couple of dB lower. The
80s Pad's slow attack keeps its first-second level down.)

Levels of the real-time instruments (first second, default volume and room;
the C4-E4-G4 triad, except Mono voices: one note, C3 on Bass, C4 otherwise),
peak / RMS dBFS:

| Preset / voices | Light 40 | Light 64 | Light 100 | Normal 40 | Normal 64 | Normal 100 |
|---|---|---|---|---|---|---|
| Moog-style Synth / Bass / Poly | −12.6 / −22.3 | −11.4 / −21.1 | −10.2 / −19.9 | −17.2 / −26.8 | −15.3 / −24.9 | −11.9 / −21.6 |
| Moog-style Synth / Bass / Mono | −10.1 / −20.0 | −8.9 / −18.8 | −7.6 / −17.6 | −14.8 / −24.6 | −12.9 / −22.7 | −9.4 / −19.3 |
| Moog-style Synth / Lead / Poly | −11.2 / −23.1 | −10.0 / −22.0 | −8.8 / −20.8 | −15.8 / −27.6 | −13.9 / −25.7 | −10.5 / −22.5 |
| Moog-style Synth / Lead / Mono | −12.2 / −20.5 | −11.1 / −19.4 | −9.9 / −18.2 | −16.8 / −24.9 | −14.9 / −23.1 | −11.6 / −19.8 |
| Moog-style Synth / Poly / Poly | −10.8 / −22.5 | −9.7 / −21.4 | −8.5 / −20.2 | −15.5 / −27.1 | −13.6 / −25.2 | −10.2 / −21.8 |
| Moog-style Synth / Poly / Mono | −11.2 / −20.2 | −10.0 / −19.0 | −8.8 / −17.8 | −15.9 / −24.8 | −13.9 / −22.9 | −10.5 / −19.5 |
| String Machine / Strings | −6.5 / −20.7 | −5.9 / −20.1 | −5.4 / −19.6 | −9.6 / −23.8 | −8.1 / −22.3 | −6.2 / −20.4 |
| String Machine / Phased | −8.1 / −20.6 | −7.5 / −20.0 | −6.9 / −19.5 | −11.2 / −23.7 | −9.6 / −22.1 | −7.7 / −20.3 |
| String Machine / Dark | −5.6 / −20.0 | −5.0 / −19.5 | −4.4 / −18.9 | −8.6 / −23.1 | −7.1 / −21.6 | −5.2 / −19.7 |
| Electric Piano / Suitcase | −7.7 / −21.2 | −6.3 / −20.0 | −5.0 / −18.9 | −15.8 / −28.9 | −11.6 / −24.9 | −6.9 / −20.5 |
| Electric Piano / Bright Tines | −8.1 / −23.1 | −6.5 / −21.8 | −4.8 / −20.5 | −16.9 / −30.9 | −12.4 / −26.8 | −7.2 / −22.3 |
| Electric Piano / Wurli | −7.9 / −24.6 | −5.6 / −22.9 | −2.7 / −21.0 | −17.4 / −32.7 | −12.9 / −28.6 | −6.6 / −23.6 |
| Organ / Jazz (no velocity) | −8.3 / −19.1 | −8.3 / −19.1 | −8.3 / −19.1 | −8.3 / −19.1 | −8.3 / −19.1 | −8.3 / −19.1 |
| Organ / Gospel | −8.3 / −19.5 | −8.3 / −19.5 | −8.3 / −19.5 | −8.3 / −19.5 | −8.3 / −19.5 | −8.3 / −19.5 |
| Organ / Rock | −10.5 / −21.0 | −10.5 / −21.0 | −10.5 / −21.0 | −10.5 / −21.0 | −10.5 / −21.0 | −10.5 / −21.0 |
| Organ / Ballad | −7.8 / −20.4 | −7.8 / −20.4 | −7.8 / −20.4 | −7.8 / −20.4 | −7.8 / −20.4 | −7.8 / −20.4 |
| Polysynth / 80s Pad | −10.4 / −22.5 | −9.5 / −21.5 | −8.4 / −20.4 | −14.3 / −26.5 | −12.7 / −24.9 | −9.9 / −21.9 |
| Polysynth / Brass Stab | −9.5 / −21.0 | −8.5 / −19.9 | −7.3 / −18.8 | −13.6 / −25.0 | −11.9 / −23.4 | −8.9 / −20.4 |
| Polysynth / Synthwave Lead | −9.7 / −20.5 | −8.7 / −19.5 | −7.6 / −18.5 | −13.7 / −24.4 | −12.1 / −22.9 | −9.1 / −20.0 |
| Polysynth / Juno Strings | −8.1 / −19.3 | −7.0 / −18.3 | −5.9 / −17.2 | −12.1 / −23.3 | −10.4 / −21.7 | −7.5 / −18.7 |
| Polysynth / OB Brass | −7.9 / −18.1 | −6.9 / −17.1 | −5.8 / −16.0 | −11.8 / −22.1 | −10.2 / −20.5 | −7.3 / −17.5 |

The Electric Piano's soft notes are quieter mostly because they're darker:
its velocity range is wider than the other instruments' on purpose.

Chords with a low note (Moog-style Synth, Poly, Light, velocity 100): C4-E4-G4
held, then C2 added. Level at each note's fundamental (±2 Hz band over 2 s, dB),
and how much the held notes change when C2 comes in. The check fails if a note
is more than 12 dB under the loudest, a held note drops by more than 1.5 dB, or
the limiter acts:

| Preset | C2 | C4 | E4 | G4 | Spread | C4 / E4 / G4 change | Limiter |
|---|---|---|---|---|---|---|---|
| Bass | −42.7 | −35.2 | −34.5 | −33.3 | 9.4 | −0.2 / 0.0 / 0.0 | 0.0 |
| Lead | −31.4 | −33.8 | −33.9 | −34.3 | 2.8 | +1.1 / +0.2 / −0.1 | 0.0 |
| Poly | −33.5 | −34.1 | −34.1 | −34.6 | 1.1 | 0.0 / 0.0 / 0.0 | 0.0 |

(The Bass C2 reads lowest because its two saws, 7 cents apart, beat slowly over
the 2-second window; its harmonics and sub-octave carry it.)

The same check on the newer instruments is measured over 4 s, so partials of C2
that nearly coincide with an upper note (C2's 6th harmonic is 0.4 Hz from G4)
average over a whole beat. On the Electric Piano, whose notes decay, all four
keys are struck together:

| Instrument / preset | C2 | C4 | E4 | G4 | Spread | C4 / E4 / G4 change | Limiter |
|---|---|---|---|---|---|---|---|
| Electric Piano / Suitcase | −38.0 | −42.4 | −43.9 | −45.2 | 7.2 | 0.0 / 0.0 / 0.0 | 0.0 |
| Electric Piano / Bright Tines | −40.6 | −45.6 | −47.3 | −48.6 | 8.0 | 0.0 / 0.0 / 0.0 | 0.0 |
| Electric Piano / Wurli | −45.8 | −52.4 | −54.4 | −55.9 | 10.1 | 0.0 / 0.0 / 0.0 | 0.0 |
| Organ / Jazz | −36.6 | −36.1 | −35.9 | −29.5 | 7.1 | −0.3 / −0.2 / −0.2 | 0.0 |
| Organ / Gospel | −38.8 | −38.4 | −38.1 | −31.8 | 7.0 | −0.7 / −0.6 / −0.7 | 0.0 |
| Organ / Rock | −46.6 | −40.4 | −40.3 | −36.4 | 10.2 | +4.1 / +3.8 / +1.8 | 0.0 |
| Organ / Ballad | −35.3 | −35.0 | −34.8 | −34.6 | 0.7 | 0.0 / −0.1 / −0.1 | 0.0 |
| Polysynth / 80s Pad | −40.6 | −37.5 | −38.5 | −35.6 | 5.0 | +0.7 / +0.5 / +0.2 | 0.0 |
| Polysynth / Brass Stab | −35.5 | −31.2 | −31.5 | −31.3 | 4.3 | +0.3 / 0.0 / +0.1 | 0.0 |
| Polysynth / Synthwave Lead | −36.0 | −34.4 | −35.4 | −35.9 | 1.6 | −0.9 / +0.2 / +0.4 | 0.0 |
| Polysynth / Juno Strings | −38.9 | −28.9 | −29.7 | −30.0 | 10.0 | +0.3 / +0.3 / +0.1 | 0.0 |
| Polysynth / OB Brass | −30.8 | −29.4 | −30.2 | −38.6 | 9.2 | +0.5 / +0.1 / +0.7 | 0.0 |

(On the organ an upper note can *gain* level when C2 is added: C2's upper
drawbars play the very same tonewheels as C4, E4 and G4. On Jazz and Gospel, G4
reads higher because C4's 5⅓' drawbar is the G4 wheel.)

The synth render test also plays a 25-second workout through every preset (bass
line, legato with glide, fast repeats, a cutoff/drive sweep with resonance at
maximum and the mod wheel all the way up, which also switches the organ's Leslie
to fast and back, the lowest and highest keys, pedalled chords, a ten-note
fortissimo chord, quiet notes). It checks for clipping, clicks, NaN/denormal
samples and that everything stops. The Moog-style Synth plays it in both Poly
and Mono. A last check steps through all six instruments with notes held and
ringing: no clicks, and nothing left sounding.

### CPU

The whole engine (instrument, reverb and limiter) in 64-frame blocks, as a
percentage of one core in real time, measured by the synth render test: on a
small Linux VM, and with `--synth-test` on the Windows desktop the app was
tested on (the release build). Ten notes are held (two octaves of a chord,
velocity 100, pedal down):

| Instrument / preset | Linux VM | Windows PC |
|---|---|---|
| Engine idle (reverb + limiter, no notes) | 0.26 % | 0.51 % |
| Moog-style Synth (Bass / Lead / Poly) | 1.62 / 1.72 / 1.65 % | 3.18 / 3.26 / 3.15 % |
| String Machine (Strings / Phased / Dark) | 0.90 / 1.02 / 0.86 % | 1.43 / 1.70 / 1.74 % |
| Electric Piano (Suitcase / Bright Tines / Wurli) | 0.54 / 0.54 / 0.54 % | 1.04 / 1.06 / 1.09 % |
| Organ (Jazz / Gospel / Rock / Ballad) | 1.36 / 1.41 / 1.40 / 1.27 % | 2.38 / 2.39 / 2.49 / 2.23 % |
| **Organ / Rock, all 61 keys held, Leslie fast** | **1.68 %** | **3.09 %** |
| Polysynth (80s Pad / Brass Stab / Synthwave Lead / Juno Strings / OB Brass) | 1.92 / 1.75 / 1.97 / 2.05 / 1.28 % | 3.71 / 3.36 / 3.79 / 3.57 / 2.38 % |

The organ sums its sound per tonewheel rather than per key, so holding every
key costs about the same as holding three.

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
For the newer instruments: the Electric Piano gets brighter with velocity
(steadily, by over 10 dB of upper harmonics from soft to hard), low notes ring
longer than high ones, and the dampers work. The organ's tonewheel tuning is
checked against the gear ratios, its percussion is single-triggered and its
1' drawbar drops out with percussion on. Velocity doesn't change the organ at
all, and the Leslie ramps with inertia (horn fast, drum slow, in both
directions) and spreads the sound in stereo. Polysynth voice stealing takes
released voices before held ones, then the oldest, with no clicks. Every
preset of the new instruments keeps all notes of C2 + C4-E4-G4 audible, and the
mod wheel and a mapped button both drive the Leslie.

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
| `src/synth.rs` | oscillators, ladder filter, envelopes, the Moog-style synth (poly/mono, note priority) and its presets |
| `src/strings.rs` | the string machine: divide-down oscillators, ensemble, phaser |
| `src/ep.rs` | the electric piano: tine/reed model, pickup non-linearity, hammer and damper noises, tremolo |
| `src/organ.rs` | the organ: tonewheel generator, drawbars, percussion, key click, overdrive, rotating speaker |
| `src/poly.rs` | the polysynth: Juno-, Prophet- and Oberheim-style voices, voice allocation, BBD chorus |
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

## Credits

- **Piano samples:** Salamander Grand Piano V3 by Alexander Holm (Yamaha C5),
  FLAC edition by the FreePats project, CC BY 3.0 (see below).
- **Real-time instruments** (Moog-style Synth, String Machine, Electric Piano,
  Organ, Polysynth): written for this project from published descriptions of how
  the originals work. These include the tonewheel generator's gear ratios, the
  drawbar footages and single-trigger percussion, the rotating speaker's
  crossover and rotor speeds, tine and reed pickup behaviour, and the Juno-style
  chorus. No samples, code or patches were taken from any manufacturer or
  product. They're covered by this repository's MIT license.
- **Trademarks:** Moog, Rhodes, Wurlitzer, Hammond, B3, Leslie, Juno, Roland,
  Prophet, Sequential and Oberheim are trademarks of their owners. They're used
  here only to describe what each instrument imitates; this project isn't
  affiliated with, or endorsed by, any of them.
- **Libraries:** egui/eframe, midir, the `windows` crate, claxon, hound, rtrb,
  serde and the others listed in `Cargo.toml`, under their own licenses.

## License

The code, the real-time instruments and the icon are released under the
[MIT License](LICENSE).

The piano samples are **not** part of this repository and are not covered by the
MIT License. The app downloads the Salamander Grand Piano V3 by Alexander Holm,
licensed under Creative Commons Attribution 3.0 (see
`packaging/LICENSE-SAMPLES.txt`).
