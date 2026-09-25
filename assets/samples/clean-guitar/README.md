# Clean electric guitar

Real Gretsch Anniversary hollowbody guitar recordings by Brian Wood, from
Karoryfer Samples' **Black And Green Guitars**. CC0-1.0; original license included.

- Upstream: https://github.com/sfzinstruments/karoryfer.black-and-green-guitars
- Pinned revision: `b3b3249d37dc977a1a297bd2dc053e6d9b6b805c`
- Source articulation: `Samples/green/ord/twang_*`, medium and firm picking,
  takes 1 and 2, eleven roots from MIDI 40 through 80 at four-semitone intervals.
- Source octave names are one higher than scientific pitch notation: upstream
  `e3` maps to MIDI 40 / E2. The SFZ mapping (default key center 60 when absent)
  establishes nominal roots; measured reference frequencies calibrate playback.
- Source inventory with Git blob hashes: `tools/clean-guitar/sources.json`.
  `manifest.json` records measured pitch, variability, trimming, gain, and SHA256
  for each generated asset. NumPy 1.26.4 / SciPy 1.14.1 used for generation.

Preparation downmixes to mono, resamples to 44.1 kHz if necessary, removes DC,
aligns to 1 ms before the pick transient, retains up to 8 seconds of natural
ringing, peak-matches each take to 0.8, and applies a 0.5 ms leading edge and
50 ms terminal fade. Seven prefiltered PCM16 mip levels support upward retuning.
There is no distortion, compressor, synthesized attack, or artificial sustain
loop. Upstream string/pick noise and tone remain part of the recordings.

Each event selects the nearest nominal root and one of its two seeded takes.
Volume below 0.65 uses medium picking; 0.65 and above uses firm picking. This is
a discrete articulation switch, not continuous velocity modeling. Score volume
still multiplies output gain. The selected sample uses its measured frequency
for exact fractional-rate retuning: choosing a root never quantizes the score.
Recorded pitch settling remains; large transpositions alter tone and duration.

Default-duration notes ring naturally; explicit score durations damp over the
last 5 ms of the gate. Chord timing is authored in the score; the voice does not
insert notes, infer guitar voicings, or automatically delay a chord into a strum.
128 fixed note slots permit overlap; excess new notes are dropped until a slot
frees. Existing VSCO attack trimming is deliberately not applied to guitar.

Rebuild from the repository root:

```sh
python3 tools/clean-guitar/download.py /tmp/ahess-guitar-originals
python3 tools/clean-guitar/prepare.py /tmp/ahess-guitar-originals
rustfmt --edition 2021 src/voice_rendering/clean_guitar_samples.rs
```

Regular builds use committed PCM assets and need no network, Python, sampler
plugin, or external sound library. The added embedded PCM footprint is about
39 MiB. These recordings are not Apple's GarageBand instrument.

## Audition and verification

```sh
AHESS_GUITAR_DEMO_ROOT=/tmp/ahess-guitar-audition cargo test --offline clean_guitar_write_audition_project -- --ignored --nocapture
```

The generated project and 48 kHz stereo WAV demonstrate medium then firm plucks
(0–4 seconds), authored 15.625 ms string-to-string strums (4–8 seconds), and
short damped notes (8–12 seconds), using custom frequencies based on 129.5 Hz.
No automatic speaker playback occurs. This is a candidate clean-guitar voice,
not a verified match to a specific GarageBand preset or the user's prior piece.

Validation on 2026-09-14: full offline suite, 363 passed / 16 ignored; offline
build passed. Tests cover picker selection, project persistence, arbitrary
frequency measurements at 44.1/48/96 kHz, deterministic take selection,
overlapping notes, smooth explicit cutoffs, and live/offline/stem parity.
Across the 44 source assets, peak amplitude in the first 50 ms is 1.70–6.77
times the peak at 0.5–0.75 seconds (median 2.46), supporting the intended
prominent attack. Measurement does not establish subjective similarity to the
GarageBand reference. The standalone installed-app UI was not visually checked.
