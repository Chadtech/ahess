# Audio architecture

Status: working design

Last updated: 2026-07-29

This document records the intended direction for pitch systems, score
interpretation, instruments, playback, stereo spatialization, and acoustic
effects. It is a reference for incremental implementation; it is not a claim
that every type name or configuration shape below is final.

## Goals

- Projects select reusable workspace tuning systems, while different projects
  may still choose different systems and notation.
- A new tuning can usually be described with project data rather than a new
  Rust implementation.
- Pitched and triggered voices can coexist in the same score.
- Complex instruments can be implemented as handwritten Rust DSP while their
  meaningful parameters remain project configuration.
- Instrument synthesis is independent of tuning, position, room acoustics,
  and output channel layout.
- Playback can produce two genuinely different output channels and eventually
  model sources positioned in a configured three-dimensional space.
- Invalid pitch systems, voice configurations, and acoustic values are
  rejected at system boundaries rather than handled repeatedly in the audio
  callback.

## Core decisions

### Tuning systems are reusable workspace resources

The workspace owns named tuning-system definitions with stable
`TuningSystemId` values. A project normally stores one identifier and carries
the resolved `PitchSystem` while it is loaded. This lets several projects share
one definition and makes an edit to that definition effective the next time
each project is loaded. Pitched voices resolve their score cells through the
project's resolved system. Triggered voices ignore it.

The built-in `western-twelve-tone` system is always available and cannot be
edited or deleted. User systems live as one TOML file per identifier under
`tuning-systems/`. A display-name edit preserves the identifier, and a system
referenced by a project cannot be deleted.

The pitch system remains non-optional initially. This ensures that every
pitched voice has a valid resolver without introducing a separate
"project has tuning" flag. A percussion-only project may carry an unused pitch
system. Embedded pitch-system data is accepted only as a compatibility form for
projects created before the workspace library. This can be revisited if
percussion-only projects become an important workflow.

### Score interpretation depends on the voice

A score cell is stored as raw text so the editor can preserve empty,
half-typed, or temporarily invalid content. When preparing playback or
reporting editor issues, each column is interpreted according to its voice:

- A pitched voice resolves notation to a validated frequency.
- A triggered voice resolves notation to a hit, sample, or other trigger.
- A blank cell is a rest in either kind of column.

There should not be a long-lived representation in which a triggered voice can
contain a pitched event or a pitched voice can contain a drum hit. Prepared
playback data should retain the distinction with enums.

### Handwritten code defines DSP behavior

Project configuration should select an implemented instrument and provide its
meaningful parameters. It should not become a general-purpose DSP programming
language.

For example, a `MetallicChoir` or `BellCloud` instrument can be implemented in
its own Rust module. TOML may select it and configure brightness, instability,
partial spread, or decay. The algorithm itself remains code.

### Instruments do not write hardware channels

An ordinary instrument produces a source signal. A later acoustic stage uses
the voice position and project scene to turn that source into a stereo frame.
This keeps every instrument from independently implementing panning, distance,
and room behavior.

An intrinsically spatial instrument can eventually expose multiple mono
emitters with positions local to the voice. That is preferable to allowing an
instrument to write directly to CPAL channel buffers.

### Persisted specifications and runtime state are separate

`Project` contains serializable, comparable specifications. The audio engine
contains mutable runtime state such as oscillator phases, active notes, filter
history, delay buffers, and reverb tails.

Stable `VoiceId` values identify runtime voices across live project updates so
compatible state can be preserved. Changing an instrument to an incompatible
kind recreates that voice's runtime state.

## Intended pipeline

```text
raw score cell
    |
    v
voice-specific resolver
    |-- pitched notation --> project pitch system --> frequency event
    `-- trigger notation ---------------------------> trigger event
    |
    v
scheduled, typed voice events
    |
    v
handwritten instrument runtime
    |
    v
voice-local signal processing
    |
    v
position + project acoustic scene
    |
    v
stereo source contribution
    |
    v
mix + project/master effects
    |
    v
device channel mapper
```

Pitch resolution happens before the real-time callback. The callback receives
prepared events and numeric frequencies; it does not parse score text or
project configuration.

### Timing variance is deterministic humanization

`timing_variance` is the greatest number of samples by which a note may be
late. Playback derives a separate local seed from the project seed, the
one-based absolute arrangement beat, and the stable `VoiceId` for every
voice/beat pair. It then draws that event's delay from a normal distribution
clipped to the inclusive range from zero through `timing_variance`. The
distribution is centered on half the configured range, with each boundary
three standard deviations from the center. Only the roughly 0.27% of normal
draws outside those boundaries are clipped, so every seed produces a delay in
one draw with no retry or fallback path. The effective maximum is capped at one
sample less than the beat length so a delayed note always has time to sound.

Using absolute arrangement beats and stable voice IDs means the timing for an
event does not change when a different loop slice is selected, voices are
reordered, or unrelated score cells are edited. Changing the project seed
deterministically changes the complete timing pattern. Random sampling happens
while playback data is prepared, never in the real-time audio callback.

An isolated part loop has no arrangement position, so its audition uses
one-based part-local beats as its deterministic timing seed positions. The
same part may therefore have different humanization when auditioned alone than
when played at a later position in the arrangement.

Inserting or deleting score rows is a structural timing edit: it changes the
part length, shifts later absolute arrangement beats, and therefore may change
their deterministic timing variance. The editor stops active playback before
publishing that coordinated project-and-score update. Clearing rows keeps the
part length and absolute beat positions unchanged and can update live playback
like an ordinary cell edit.

### Frequency variance is deterministic detuning

`frequency_variance` is the greatest fractional distance by which a pitched
event may vary above or below its resolved frequency. For example, `0.01`
allows a frequency to vary by up to one percent in either direction. Playback
derives a local seed from a frequency-variance domain, the project seed, the
one-based absolute arrangement beat, and the stable `VoiceId`. It samples a
normal distribution centered on the resolved frequency and clips it to the
inclusive configured range, with each boundary three standard deviations from
the center. The fractional offset is applied to the resolved frequency before
the prepared event reaches the audio callback.

The frequency and timing seed domains are separate, so adding frequency
variance does not reuse a timing draw. Absolute arrangement beats and stable
voice IDs give frequency variance the same edit, reorder, and overlapping-loop
stability as timing variance. A value of zero preserves the resolved frequency
exactly. Projects created before this setting existed load with zero frequency
variance. Values must be at least zero and less than one, which keeps the lower
bound positive. Sampling happens during playback preparation rather than in the
real-time audio callback.

Like timing variance, an isolated part audition uses one-based part-local beat
positions because it has no arrangement position.

## Domain sketch

The exact names may change, but the model should preserve these relationships.

```rust
pub struct Project {
    // Existing project fields...
    tuning_system_id: Option<TuningSystemId>,
    pitch_system: PitchSystem,
    acoustic_scene: AcousticScene,
    voices: Vec<Voice>,
}

pub struct Voice {
    id: VoiceId,
    name: VoiceName,
    instrument: InstrumentSpec,
    position: Point3Meters,
}

pub enum InstrumentSpec {
    Pitched(PitchedInstrumentSpec),
    Triggered(TriggeredInstrumentSpec),
}

pub enum PitchedInstrumentSpec {
    Sin,
    Saw,
    BellCloud {
        partial_spread: Cents,
        decay: Duration,
    },
}

pub enum TriggeredInstrumentSpec {
    DrumKit(DrumKitSpec),
    Sample(SampleSpec),
}
```

The instrument variant determines its input contract. A separate
`uses_tuning: bool` would permit contradictory states and should not be added.

Prepared playback data keeps those contracts separate:

```rust
pub enum PreparedVoice {
    Pitched {
        id: VoiceId,
        instrument: PitchedInstrumentSpec,
        events: Vec<Option<PitchedEvent>>,
        delays: Vec<u32>,
    },
    Triggered {
        id: VoiceId,
        instrument: TriggeredInstrumentSpec,
        events: Vec<Option<TriggerEvent>>,
        delays: Vec<u32>,
    },
}
```

The engine constructs mutable renderers from those specifications:

```rust
enum VoiceRuntime {
    Pitched(PitchedVoiceRuntime),
    Triggered(TriggeredVoiceRuntime),
}
```

This also creates a place for sound to continue after a beat ends. An event
starts or changes an instrument; the runtime renders every sample afterward,
including releases, resonances, and effect tails.

## Pitch systems

The public operation needed by score preparation is intentionally small:

```rust
impl PitchSystem {
    pub fn resolve_cell(
        &self,
        cell: &str,
    ) -> Result<Option<FrequencyHz>, ResolvePitchError>;
}
```

The initial design supports two representations:

```rust
pub enum PitchSystem {
    Periodic(PeriodicPitchSystem),
    Explicit(ExplicitPitchSystem),
}
```

### Periodic systems

A periodic system contains:

- A validated positive fundamental frequency.
- A repeating period, often but not necessarily `2/1`.
- A non-empty collection of degree intervals.
- A notation strategy that converts score text to a period and degree.

Intervals should support exact ratios and cents:

```rust
pub enum Interval {
    Ratio(Ratio),
    Cents(Cents),
}
```

The first notation strategy should reproduce the Radler digit notation. With a
place value of ten, `34` means period 3 and degree 4. Given a 25 Hz fundamental,
a `2/1` period, and a `7/4` fourth degree, it resolves to:

```text
25 Hz * 2^3 * 7/4 = 350 Hz
```

An illustrative workspace tuning-system file is:

```toml
id = "slendro-sketch"

[pitch_system]
kind = "periodic"
name = "Slendro sketch"
fundamental_hz = 25.0
period = "2/1"
degrees = ["1/1", "8/7", "21/16", "32/21", "7/4"]

[pitch_system.notation]
kind = "radler_digits"
place_value = 10
```

Intervals use either a positive ratio (`"3/2"`) or a cents value (`"700c"`).
The compatibility twelve-tone system is represented as a periodic system with
`western_twelve_tone` notation. That notation accepts note names, note numbers
from 0 through 127, and the historical `-` and `rest` spellings.

Custom systems reserve only a blank cell as a rest. Radler digit notation does
not reserve `-` or `rest`, and an explicit system may define either as a pitch
token. Explicit tokens are case-sensitive after surrounding whitespace is
trimmed.

### Explicit systems

An explicit system maps arbitrary notation directly to frequencies. It is the
escape hatch for experiments that do not fit a reusable notation strategy:

```toml
[pitch_system]
kind = "explicit"
name = "Ember system"

[pitch_system.pitches]
ember = 197.3
glass = 241.8
"⟟" = 316.4
```

This avoids designing a universal notation language before concrete use cases
exist.

### Tuning controls fundamentals, not timbre

A pitched instrument receives a fundamental frequency from the pitch system.
It may generate partials, detuned oscillators, FM carriers, noise bands, or
resonances at any frequencies required by its algorithm. Those internal
frequencies do not need to be members of the project tuning.

The harmonic-saw voice is the first implemented example. It sums up to 64 sine
partials, omits partials too close to or above Nyquist for the current note and
sample rate, and rolls their amplitudes off slightly faster than the ideal
`1/n` saw series to soften the discontinuity. Its fundamental remains at the
resolved pitch. Higher partials use a small progressively stretched
inharmonicity modeled after a stiff resonating string, so they sit just above
exact integer multiples without requiring those frequencies to exist in the
project tuning.

## Triggered voices

Triggered notation is independent of the project pitch system. Possible future
cell forms include `x`, velocity values, or named hits such as `kick` and
`snare`. The grammar should be chosen when the first triggered instrument is
implemented rather than generalized prematurely.

The model uses "pitched" and "triggered" instead of "tonal" and "non-tonal."
A drum sample may eventually be pitch-controlled, and a tonal-sounding sample
may simply be triggered at one fixed pitch. The useful distinction is what
control event the instrument accepts.

## Acoustic scene and stereo output

The project owns an acoustic scene. Voices own positions within it.

```rust
pub struct AcousticScene {
    listener: Point3Meters,
    room: Option<RectangularRoom>,
}
```

Coordinates and frequencies must use validated domain types. `Point3Meters`
must reject non-finite coordinates. `FrequencyHz` must reject zero, negative,
and non-finite values. Coordinates are also bounded for numerical stability,
room dimensions are limited to 100 meters per axis, and a free-field voice may
be no farther than 1000 meters from the listener so untrusted configuration
cannot demand an effectively unbounded real-time delay buffer.

The first spatializer is implemented with headphones as its reference target,
but its two-ear propagation also remains useful through ordinary stereo
speakers. Coordinates are meters in a fixed right-handed room space: positive
X is right, positive Y is forward, and positive Z is up. A
rectangular room occupies `0..width`, `0..length`, and `0..height`. The listener
currently faces positive Y, and its virtual ears are 18 centimeters apart on
the X axis. Listener orientation can become explicit when the interface needs
it; speaker positions are not part of the acoustic scene. Hardware output
mapping remains the responsibility of the final device adapter.

The current spatializer:

- Produces an explicit `StereoFrame { left, right }`.
- Sends every direct and reflected path to both ears. Each ear receives its own
  distance attenuation and arrival time, so a lateral source favors the nearer
  ear without muting the farther ear. A centered legacy voice retains its
  previous per-channel level.
- Uses a 343 meters-per-second propagation speed and fractional delay taps.
- Preserves the small left/right arrival-time difference derived from the two
  ear positions.
- Applies unity distance gain through one meter, then pressure-amplitude
  attenuation proportional to `1 / distance`.
- Treats a source coincident with the listener as dry, centered, and
  zero-delay for compatibility.
- Keeps each voice's propagation buffer alive across beats and loop boundaries,
  so distance never truncates a note or reflection at a score boundary.
- Adds one image-source reflection from each of a configured rectangular
  room's six surfaces. A validated amplitude reflection gain from zero through
  one is shared by the initial six surfaces.

A missing acoustic scene means a free field with the listener at the origin,
and a missing voice position means the origin. Existing projects therefore
remain dry and centered. Project configuration can persist a listener, an
optional rectangular room, and a position for every voice. New-project and
project-settings dialogs can enable a rectangular room and edit its width,
length, height, and reflection gain. The room form centers the listener; when
it enables, disables, or resizes a room, it translates voice positions by the
same amount so their offsets from the listener remain stable. The add-voice and
edit-voice forms expose each voice's absolute X, Y, and Z coordinates in meters,
show the listener position and any room bounds, and reject a position outside a
configured room. A newly added voice starts at the listener position, preserving
the centered compatibility behavior until the user moves it.

Later acoustic work may add listener orientation, per-surface materials,
high-frequency damping and air absorption, higher-order reflections, and a
diffuse late room response. Distance perception should ultimately use the
direct-to-reverberant ratio rather than volume alone. These belong in the
acoustic stage because they depend on both the source and the configured scene.

The device writer is a separate final adapter. A mono device receives the
average of a stereo frame. Stereo devices receive left and right directly. A
device with more than two channels receives left and right in its first two
channels and silence in the others until a deliberate multichannel layout is
implemented.

## Effects and processing scopes

Effects should be placed according to what they mean:

1. Instrument-internal processing is part of a handwritten instrument.
2. Voice-local processing changes a source before spatialization, such as a
   filter, saturation, or local delay.
3. Acoustic processing depends on source and listener position.
4. Project/master processing runs after voice contributions are mixed.

Effect algorithms remain Rust code. Configuration may select known effects,
provide validated parameters, and eventually order an effect chain. A generic
node-graph language is not required for the initial architecture.

The master mixer retains square-root normalization by the number of
acoustically active voices. Because starting or ending one voice would otherwise
rescale every other voice in a single sample, normalization gain moves to each
new target with a 64-sample linear ramp. During silence it returns to the
single-voice gain so the next isolated voice starts at the expected level.

The first convolution configuration is a project default applied independently
to every mono voice before position and room acoustics. It is therefore a
voice-local effect selected by the project, not a master effect. The immutable
decoded response may be shared, but each voice will require its own mutable
convolution history so tails remain independent before spatialization. A later
voice setting may inherit the project response, bypass it, or select a custom
response without changing this processing scope.

The project-settings UI can import one mono PCM or IEEE-float WAV response up
to 30 seconds and 64 MiB. Imported responses are content-addressed assets under
the project directory, and project configuration stores the asset path and its
display name. Projects without a `[voice_convolution]` table have no configured
response. File selection, validation, persistence, reload, replacement,
removal, and project duplication are implemented; the convolution DSP and WAV
decoding into prepared samples remain future work.

## Real-time boundary

The audio callback should perform bounded numeric work over already-prepared
data. In particular, it should not:

- Parse score notation, TOML, ratios, or cents.
- Resolve MIDI notes or twelve-tone names.
- Allocate per sample.
- Load files or samples from disk.
- Hold a blocking project-model lock.

Frequency conversion, score validation, instrument construction, sample
loading, and large buffer allocation happen outside the callback. Live updates
publish prepared playback data; the engine reconciles runtime voices by
`VoiceId`.

Initial acoustic delay buffers are allocated before the stream starts. A live
scene or voice-position update currently recreates affected delay buffers while
the callback accepts its new playback snapshot, following the pre-existing
snapshot update pattern. Move that preparation outside the callback before
exposing continuous position automation.

## Offline audio builds

The build workspace renders the complete project arrangement to audio files.
It does not follow the selected playback loop. A build writes one stereo mix
and one stereo file per voice under the project's `build/` directory. Repeated
builds replace the files they own, and a private manifest lets the builder
remove a generated stem after its voice is removed or renamed without deleting
unrelated files.

Offline rendering consumes the same prepared `PlaybackLoop`, voice waveform,
envelope, spatializer, and master-gain behavior as real-time playback. It is a
finite renderer rather than a simulated audio device: after the last
arrangement beat it feeds silence through every voice runtime until propagation
and reflection tails are no longer active. It therefore needs no output device
and does not duplicate score interpretation or instrument DSP.

The initial build format is stereo 32-bit IEEE-float WAV. The workspace offers
44.1, 48, and 96 kHz, with 48 kHz as the default. Because the current project
model expresses beat length and timing variance in samples, selecting a sample
rate also determines the rendered duration of those sample counts. The selected
rate is explicit in the build workspace rather than inferred from audio
hardware.

Voice files contain their post-spatialization contributions with the same
master normalization ramp used by the mix. Summing the voice files therefore
reproduces the mix before its final `[-1, 1]` safety clamp. Float samples let an
individual stem preserve values outside that range instead of clipping before
the files are recombined.

## Persistence and compatibility

Current project files store `tuning_system_id` instead of duplicating a tuning
definition. Existing projects may have no pitch-system field or may contain an
embedded `[pitch_system]` table, and their voices store `voice_type = "sin"`,
`"saw"`, or `"harmonic-saw"`.

Compatibility loading should:

- Interpret a missing pitch system as the current twelve-tone/MIDI behavior.
- Resolve `tuning_system_id` through the workspace library and reject missing
  references.
- Continue to load an embedded pitch-system table as a legacy project-owned
  definition.
- Interpret sin, saw, and harmonic-saw voices as pitched instrument variants.
- Interpret a missing voice position as the acoustic center.
- Interpret a missing acoustic scene as a dry centered stereo scene.
- Write the built-in tuning-system reference when a project with no historical
  tuning field is next saved. Preserve an embedded custom definition until the
  user explicitly chooses a library system.

Compatibility is a persistence-boundary concern. MIDI note numbers and
twelve-tone names should not remain the internal pitch identity.

## Implementation sequence

Each phase should leave the application working and tested.

### Phase 1: frequency-native, reusable pitch resolution (implemented 2026-07-17)

- Add validated frequency and interval types.
- Add reusable workspace tuning systems and project references to them.
- Implement Radler digit notation and project configuration.
- Resolve score cells with the project pitch system.
- Change pitched playback data from MIDI notes to frequencies.
- Preserve old projects through a legacy loader default.

Completion criterion: two projects can interpret the same score text through
different configured periodic tunings, and playback contains no MIDI-to-Hz
conversion.

This phase is implemented. The workspace library owns validated periodic and
explicit systems, while `Project` stores a stable reference and carries the
resolved, non-optional pitch system. The landing page opens a full-page editor
for creating, duplicating, editing, and deleting user systems. New-project and
project-settings views select from the same library. Project TOML persists the
stable ID, score errors use the selected system, and prepared playback voices
contain `FrequencyHz` values. The standalone CPAL spike remains MIDI-based as
allowed below.

### Phase 2: typed voice input contracts

- Replace the flat sin/saw `VoiceType` model with pitched and triggered
  instrument specifications.
- Validate each score column according to its voice.
- Preserve existing sin and saw project files.
- Add a minimal triggered test instrument before designing full drum support.

Completion criterion: pitched and triggered columns coexist without either
being parsed through the other's notation.

### Phase 3: stateful voice runtimes

- Move oscillator phase and envelope state out of parallel engine vectors and
  into runtime voices.
- Deliver scheduled events to runtime voices.
- Allow instruments and effects to keep rendering releases and tails between
  events.
- Reconcile runtime state across live updates by `VoiceId`.

Completion criterion: a handwritten instrument can keep meaningful state and
sound after the beat that triggered it.

Oscillator phase and acoustic delay state now live in per-voice runtimes and
are reconciled by `VoiceId`. Typed event delivery and general instrument release
state remain to complete this phase.

### Phase 4: explicit stereo frames

- Make the engine and mixer produce `StereoFrame` values.
- Map those frames to CPAL device channels explicitly.
- Initially center every voice so existing projects sound materially the same.

Completion criterion: left and right channels are distinct values throughout
the engine even when the default spatializer gives them equal content.

Implemented 2026-07-19. Mono devices receive an explicit average of the stereo
frame. Stereo devices receive left and right directly. On devices with more
than two channels, the first two receive stereo and the remaining channels are
silenced until a deliberate multichannel layout is implemented.

### Phase 5: positions and initial spatializer

- Add the acoustic scene and validated three-dimensional positions.
- Add a two-ear stereo spatializer.
- Add project persistence and compatibility defaults.
- Add UI only after the configuration and audio behavior are established.

Completion criterion: moving a voice horizontally changes its stereo image in
a predictable, tested way.

The engine, domain types, persistence, compatibility defaults, direct
spatializer, centered-room UI, and voice-position UI are implemented as of
2026-07-19. Explicit listener placement remains to be designed and implemented.

### Phase 6: complex instruments and acoustic effects

- Add handwritten pitched and triggered instruments as concrete musical needs
  arise.
- Add distance and propagation behavior.
- Add voice-local, room, and master effects without collapsing their scopes.
- Consider multiple emitters only when an instrument actually needs them.

The first acoustic portion is implemented: physical distance delay survives
score boundaries, and rectangular rooms provide six first-order image-source
reflections. Surface-dependent damping and the late room response remain.

## Current code affected

- `src/pitch_system.rs`: owns validated frequencies and intervals, project
  pitch-system definitions, notation resolution, and legacy twelve-tone
  compatibility.
- `src/tuning_system.rs`: owns workspace tuning-system identifiers, library
  persistence, the immutable built-in system, and reference-aware deletion.
- `src/app/tuning_system_editor.rs`: provides the full-page workspace editor
  for periodic and explicit tuning systems.
- `src/part.rs`: score preparation must accept project/voice interpretation
  rather than calling a global note parser.
- `src/app/project_open/score.rs`: parse issues must use the selected voice's
  resolver.
- `src/voice.rs`: voice type becomes the persisted pitched/triggered instrument
  specification.
- `src/project.rs`: stores and resolves the tuning-system reference and will
  eventually store the acoustic scene and new voice representation.
- `src/playback.rs`: stores prepared events and runtime voices, then progresses
  from mono samples to explicit stereo frames. It also exposes the finite
  renderer used by offline builds.
- `src/audio_build.rs`: streams the offline mix and per-voice frames to
  project-local IEEE-float WAV files and publishes completed builds.
- `src/acoustics.rs`: owns validated points and rooms, stereo frames,
  two-ear propagation, image-source path preparation, and per-voice
  fractional delay lines.
- `src/cpal_spike.rs`: may remain MIDI-based as an isolated prototype until the
  main playback path is established.

## Open questions

These do not block Phase 1 unless noted:

- What event grammar should the first triggered voice accept?
- Does a non-empty pitched cell retrigger a note every beat, or will the score
  gain explicit hold and note-off syntax?
- Which effects require ordered user configuration, and which belong entirely
  inside a particular instrument?
- What runtime state is preserved when instrument parameters change during
  playback?

New decisions should be added to this document as they are made. If an
implementation intentionally departs from this direction, update the document
in the same change so it remains a useful reference rather than a historical
proposal.
