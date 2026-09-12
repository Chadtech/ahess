# VSCO 2 Community Edition instruments for Ahess

Source: **Versilian Studios / Sam Gossner**, VSCO 2 Community Edition.

- Official library and CC0 declaration: https://versilian-studios.com/vsco-community/
- Original recordings: https://github.com/sgossner/VSCO-2-CE
- Pinned source revision: `440300901dfe9275fd84e0b7763af1f8443ae62e`
- License: **CC0 1.0** (https://creativecommons.org/publicdomain/zero/1.0/).
- The upstream README is retained in `UPSTREAM-README.txt`.

This is an Ahess adaptation, not an official Versilian product. It contains
49 recordings: 13 cello-section vibrato sustains, 9 flute non-vibrato sustains,
11 clarinet sustains, and 16 harp plucks. Source WAV files remain unmodified in
the build-time download cache. `manifest.json` records source paths, Git blob
hashes, measured reference pitches, preprocessing parameters, and SHA-256 hashes
of each prepared asset. `tools/vsco/sources.json` pins the download inventory.
Seven extra harp recordings are downloaded for inspection but not bundled.

## Playback behavior

- Each voice uses one recorded dynamic layer. Note volume changes gain rather
  than switching timbres; these are not full SFZ or legato implementations.
- Stereo recordings are averaged to mono to enter Ahess's existing per-voice
  positioning and room processing. The source recordings' room sound remains.
- Leading silence is trimmed with 10 ms of pre-onset retained. Recordings receive
  gain adjustment and short end fades, and are quantized to signed little-endian
  PCM16 at 44.1 kHz. Sustains retain the first 3.8 seconds; harp keeps up to ten.
- Cello, flute, and clarinet use an 80 ms crossfade at a sustain-loop boundary
  between 1.25 and 3.5 seconds, with a 120 ms release after note-off. Harp is
  unlooped, decays naturally for default-duration notes, and uses a 5 ms fade
  ending at an explicit duration's cutoff.
- Each note selects the two recordings bracketing the target pitch (or the
  nearest edge pair). Both advance independently
  at `source_rate / output_rate * target_hz / reference_hz`. Original vibrato,
  ensemble beating, and pitch motion are preserved; calibration aligns the
  representative pitch, not every moment of an acoustic performance.
- Each layer receives a seeded random gain in [0.2, 1.0), divided by
  `1 + pitch_distance_in_semitones / 3`. The two gains are normalized to sum to
  one and multiplied by note volume. Every score event gets a separate blend;
  project seed, voice ID, absolute beat, and event index make it reproducible
  across playback, loop selection, and exports. Change the project seed to
  reshuffle. Two layers occupy one note slot; no additional assets are needed.
- Filename octave conventions differ: sustained instruments use C3 for middle C,
  while harp uses C4. The clarinet file named F#5 measures near concert F6,
  roughly 100 cents below its label. Playback uses the measured reference.
- Seven prefiltered sample levels (powers-of-two decimation) plus cubic
  interpolation reduce aliasing when transposing upward. Large shifts still
  alter timbre and sample duration. Frequencies at or above 45% of the output
  sample rate are silent, avoiding near-Nyquist tonal playback.
- Up to 128 notes can overlap per voice. If exhausted, new notes are dropped
  until a slot frees; existing notes are never abruptly stolen. Samples and
  note storage are embedded/fixed; note-on never loads, allocates, or decodes.

## Reproduction

From the repository root, with Python, NumPy, SciPy, and curl installed:

```sh
python3 tools/vsco/download.py /tmp/ahess-vsco-originals
python3 tools/vsco/prepare.py /tmp/ahess-vsco-originals
rustfmt --edition 2021 src/voice_rendering/vsco_samples.rs
```

The download script verifies each original using its pinned Git blob hash.
The generated PCM files and Rust tables are committed assets, so ordinary Rust
builds need neither Python nor network access. Asset generation was performed
with the NumPy/SciPy versions recorded below; floating-point library changes may
produce small differences in regenerated assets.

Generation environment: NumPy 1.26.4, SciPy 1.14.1.
