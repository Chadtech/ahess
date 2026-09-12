use crate::score_cell;

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Write as _},
    num::{NonZeroU64, NonZeroU8},
};

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct FrequencyHz(f64);

impl FrequencyHz {
    pub fn new(value: f64) -> Result<Self, PitchSystemError> {
        let rendered_value = value as f32;
        if !value.is_finite()
            || value <= 0.0
            || !rendered_value.is_finite()
            || rendered_value <= 0.0
        {
            return Err(PitchSystemError::new(
                "frequency must be a positive finite value supported by the audio engine",
            ));
        }
        Ok(Self(value))
    }

    pub fn from_config(value: &str) -> Result<Self, PitchSystemError> {
        let value = value.trim();
        let frequency = value
            .parse::<f64>()
            .map_err(|_| PitchSystemError::new("frequency must be a number"))?;
        Self::new(frequency)
    }

    pub const fn as_hz(self) -> f64 {
        self.0
    }

    pub fn as_hz_f32(self) -> f32 {
        self.0 as f32
    }
}

// `FrequencyHz::new` excludes NaN, so equality is reflexive.
impl Eq for FrequencyHz {}

/// Validated positive duration, bounded to the score's 255-beat maximum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BeatDurationTicks(std::num::NonZeroU32);
impl BeatDurationTicks {
    pub fn new(ticks: u32) -> Result<Self, ResolvePitchError> {
        if !(1..=255 * 96).contains(&ticks) {
            return Err(ResolvePitchError::new(
                "duration must be between 1/96 and 255 beats",
            ));
        }
        Ok(Self(std::num::NonZeroU32::new(ticks).unwrap()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrikeDuration {
    VoiceDefault,
    ExplicitBeats(NonZeroU8),
    FractionalBeats(BeatDurationTicks),
}

impl StrikeDuration {
    pub fn beats(self) -> f64 {
        match self {
            Self::VoiceDefault => 1.0,
            Self::ExplicitBeats(n) => f64::from(n.get()),
            Self::FractionalBeats(n) => f64::from(n.0.get()) / 96.0,
        }
    }
    pub fn samples(self, beat_length: u32) -> u32 {
        (self.beats() * f64::from(beat_length)).round().max(1.0) as u32
    }

    pub const fn beats_or_one(self) -> u8 {
        match self {
            Self::VoiceDefault => 1,
            Self::ExplicitBeats(beats) => beats.get(),
            Self::FractionalBeats(ticks) => ticks.0.get().div_ceil(96) as u8,
        }
    }

    pub const fn explicit_beats(self) -> Option<u8> {
        match self {
            Self::VoiceDefault => None,
            Self::ExplicitBeats(beats) => Some(beats.get()),
            Self::FractionalBeats(ticks) => Some(ticks.0.get().div_ceil(96) as u8),
        }
    }
}

/// A parsed pitch, before applying a tuning. Constructed only by the note parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pitch(PitchNotation);

#[derive(Clone, Debug, Eq, PartialEq)]
enum PitchNotation {
    Radler(u64),
    Western(u8),
    Named(String),
}

/// Exact score volume; conversion to amplitude happens during resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Volume(u8);

impl Volume {
    pub const fn from_byte(value: u8) -> Self {
        Self(value)
    }
    pub const fn as_byte(self) -> u8 {
        self.0
    }
    pub fn amplitude(self) -> f32 {
        f32::from(self.0) / 255.0
    }
}

/// Valid notation, independent of its eventual frequency in a tuning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Note {
    pitch: Pitch,
    duration: StrikeDuration,
    volume: Volume,
}

impl Note {
    pub fn with_details(mut self, duration: StrikeDuration, volume: Volume) -> Self {
        self.duration = duration;
        self.volume = volume;
        self
    }

    pub fn pitch(&self) -> &Pitch {
        &self.pitch
    }
    pub const fn duration(&self) -> StrikeDuration {
        self.duration
    }
    pub const fn volume(&self) -> Volume {
        self.volume
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    frequency: FrequencyHz,
    duration: StrikeDuration,
    volume: f32,
}

impl Strike {
    pub const fn frequency(self) -> FrequencyHz {
        self.frequency
    }

    pub const fn duration_beats(self) -> u8 {
        self.duration.beats_or_one()
    }

    pub const fn duration(self) -> StrikeDuration {
        self.duration
    }

    pub const fn volume(self) -> f32 {
        self.volume
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ratio {
    numerator: NonZeroU64,
    denominator: NonZeroU64,
}

impl Ratio {
    pub fn new(numerator: u64, denominator: u64) -> Result<Self, PitchSystemError> {
        let numerator = NonZeroU64::new(numerator)
            .ok_or_else(|| PitchSystemError::new("an interval ratio numerator must be positive"))?;
        let denominator = NonZeroU64::new(denominator).ok_or_else(|| {
            PitchSystemError::new("an interval ratio denominator must be positive")
        })?;
        Ok(Self {
            numerator,
            denominator,
        })
    }

    fn multiplier(self) -> f64 {
        self.numerator.get() as f64 / self.denominator.get() as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Cents(f64);

impl Cents {
    pub fn new(value: f64) -> Result<Self, PitchSystemError> {
        let multiplier = 2.0_f64.powf(value / 1200.0);
        if !value.is_finite() || !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(PitchSystemError::new(
                "a cents interval must produce a positive finite multiplier",
            ));
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> f64 {
        self.0
    }
}

// `Cents::new` excludes NaN, so equality is reflexive.
impl Eq for Cents {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interval {
    Ratio(Ratio),
    Cents(Cents),
}

impl Interval {
    pub fn ratio(numerator: u64, denominator: u64) -> Result<Self, PitchSystemError> {
        Ratio::new(numerator, denominator).map(Self::Ratio)
    }

    pub fn cents(value: f64) -> Result<Self, PitchSystemError> {
        Cents::new(value).map(Self::Cents)
    }

    pub fn from_config(value: &str) -> Result<Self, PitchSystemError> {
        let value = value.trim();
        if let Some((numerator, denominator)) = value.split_once('/') {
            if denominator.contains('/') {
                return Err(PitchSystemError::invalid_interval(value));
            }
            let numerator = numerator
                .trim()
                .parse::<u64>()
                .map_err(|_| PitchSystemError::invalid_interval(value))?;
            let denominator = denominator
                .trim()
                .parse::<u64>()
                .map_err(|_| PitchSystemError::invalid_interval(value))?;
            return Self::ratio(numerator, denominator)
                .map_err(|_| PitchSystemError::invalid_interval(value));
        }

        let cents = value
            .strip_suffix("cents")
            .or_else(|| value.strip_suffix("cent"))
            .or_else(|| value.strip_suffix('c'))
            .ok_or_else(|| PitchSystemError::invalid_interval(value))?
            .trim()
            .parse::<f64>()
            .map_err(|_| PitchSystemError::invalid_interval(value))?;
        Self::cents(cents).map_err(|_| PitchSystemError::invalid_interval(value))
    }

    fn multiplier(self) -> f64 {
        match self {
            Self::Ratio(ratio) => ratio.multiplier(),
            Self::Cents(cents) => 2.0_f64.powf(cents.value() / 1200.0),
        }
    }

    pub fn config_value(self) -> String {
        match self {
            Self::Ratio(ratio) => format!("{}/{}", ratio.numerator.get(), ratio.denominator.get()),
            Self::Cents(cents) => format!("{}c", cents.value()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodicNotation {
    RadlerDigits { place_value: NonZeroU64 },
    WesternTwelveTone,
}

impl PeriodicNotation {
    pub fn radler_digits(place_value: u64) -> Result<Self, PitchSystemError> {
        NonZeroU64::new(place_value)
            .map(|place_value| Self::RadlerDigits { place_value })
            .ok_or_else(|| PitchSystemError::new("notation place value must be positive"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeriodicPitchSystem {
    name: String,
    fundamental: FrequencyHz,
    period: Interval,
    degrees: Vec<Interval>,
    notation: PeriodicNotation,
}

impl PeriodicPitchSystem {
    pub fn new(
        name: impl Into<String>,
        fundamental: FrequencyHz,
        period: Interval,
        degrees: Vec<Interval>,
        notation: PeriodicNotation,
    ) -> Result<Self, PitchSystemError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(PitchSystemError::new(
                "a periodic pitch system name must not be empty",
            ));
        }
        if degrees.is_empty() {
            return Err(PitchSystemError::new(
                "a periodic pitch system must contain at least one degree",
            ));
        }
        match notation {
            PeriodicNotation::RadlerDigits { place_value }
                if degrees.len() as u64 > place_value.get() =>
            {
                return Err(PitchSystemError::new(format!(
                    "notation place value {} cannot represent {} degrees",
                    place_value,
                    degrees.len()
                )));
            }
            PeriodicNotation::WesternTwelveTone if degrees.len() != 12 => {
                return Err(PitchSystemError::new(
                    "western twelve-tone notation requires exactly 12 degrees",
                ));
            }
            _ => {}
        }

        Ok(Self {
            name,
            fundamental,
            period,
            degrees,
            notation,
        })
    }

    pub fn fundamental(&self) -> FrequencyHz {
        self.fundamental
    }

    pub fn period(&self) -> Interval {
        self.period
    }

    pub fn degrees(&self) -> &[Interval] {
        &self.degrees
    }

    pub fn notation(&self) -> PeriodicNotation {
        self.notation
    }

    fn resolve(&self, pitch: &Pitch) -> Result<FrequencyHz, ResolvePitchError> {
        let (period_index, degree_index) = match (&pitch.0, self.notation) {
            (PitchNotation::Radler(notation), PeriodicNotation::RadlerDigits { place_value }) => {
                let degree = notation % place_value.get();
                if degree >= self.degrees.len() as u64 {
                    return Err(ResolvePitchError::new(format!(
                        "pitch {notation} uses degree {degree}, but {:?} has degrees 0 through {}",
                        self.name,
                        self.degrees.len() - 1
                    )));
                }
                (notation / place_value.get(), degree as usize)
            }
            (PitchNotation::Western(note), PeriodicNotation::WesternTwelveTone) => {
                (u64::from(note / 12), usize::from(note % 12))
            }
            _ => {
                return Err(ResolvePitchError::new(
                    "pitch notation does not match this tuning",
                ))
            }
        };
        let period_multiplier = self.period.multiplier().powf(period_index as f64);
        let degree_multiplier = self.degrees[degree_index].multiplier();
        FrequencyHz::new(self.fundamental.as_hz() * period_multiplier * degree_multiplier).map_err(
            |err| {
                ResolvePitchError::new(format!(
                    "pitch {pitch:?} resolves outside the supported frequency range: {err}"
                ))
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplicitPitchSystem {
    name: String,
    pitches: BTreeMap<String, FrequencyHz>,
}

impl ExplicitPitchSystem {
    pub fn new(
        name: impl Into<String>,
        pitches: BTreeMap<String, FrequencyHz>,
    ) -> Result<Self, PitchSystemError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(PitchSystemError::new(
                "an explicit pitch system name must not be empty",
            ));
        }
        if pitches.is_empty() {
            return Err(PitchSystemError::new(
                "an explicit pitch system must contain at least one pitch",
            ));
        }
        if let Some(token) = pitches
            .keys()
            .find(|token| token.is_empty() || token.trim() != token.as_str())
        {
            return Err(PitchSystemError::new(format!(
                "explicit pitch token {token:?} must be non-empty and have no surrounding whitespace"
            )));
        }
        Ok(Self { name, pitches })
    }

    pub fn pitches(&self) -> &BTreeMap<String, FrequencyHz> {
        &self.pitches
    }

    fn resolve(&self, pitch: &Pitch) -> Result<FrequencyHz, ResolvePitchError> {
        let PitchNotation::Named(token) = &pitch.0 else {
            return Err(ResolvePitchError::new(
                "pitch notation does not match this tuning",
            ));
        };
        self.pitches.get(token).copied().ok_or_else(|| {
            ResolvePitchError::new(format!("pitch {token:?} is not defined in {:?}", self.name))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PitchSystem {
    Periodic(PeriodicPitchSystem),
    Explicit(ExplicitPitchSystem),
}

impl PitchSystem {
    pub fn is_exact_key(&self, value: &str) -> bool {
        match self {
            Self::Explicit(system) => system.pitches.contains_key(value),
            Self::Periodic(_) => false,
        }
    }
    pub fn pitch_text(&self, pitch: &Pitch) -> String {
        match &pitch.0 {
            PitchNotation::Radler(n) => format!("{n:02}"),
            PitchNotation::Western(n) => n.to_string(),
            PitchNotation::Named(n) => n.clone(),
        }
    }

    /// Shared score-text boundary for validation, playback, and export.
    pub fn resolve_strike(&self, value: &str) -> Result<Option<Strike>, ResolvePitchError> {
        if score_cell::has_details(value) && !self.is_exact_key(value.trim()) {
            let events = score_cell::parse(self, value)?;
            return events
                .iter()
                .find(|e| e.offset.ticks() == 0)
                .or(events.first())
                .map(|e| e.strike(self))
                .transpose();
        }
        self.parse_note(value)?
            .as_ref()
            .map(|note| self.resolve_note(note))
            .transpose()
    }

    pub fn resolve_note(&self, note: &Note) -> Result<Strike, ResolvePitchError> {
        Ok(Strike {
            frequency: self.resolve_pitch(note.pitch())?,
            duration: note.duration(),
            volume: note.volume().amplitude(),
        })
    }

    /// Parse notation without calculating frequency. Blank cells are rests.
    pub fn parse_note(&self, value: &str) -> Result<Option<Note>, ResolvePitchError> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(None);
        }
        let (value, volume_override) = self.split_volume_suffix(value)?;
        let (pitch, duration, volume) = match self {
            Self::Periodic(system)
                if matches!(system.notation(), PeriodicNotation::RadlerDigits { .. }) =>
            {
                match value.len() {
                    2 => (value, StrikeDuration::VoiceDefault, Volume::from_byte(255)),
                    6 if value.is_ascii() => {
                        let duration = u8::from_str_radix(&value[2..4], 16).map_err(|_| {
                            ResolvePitchError::new(format!(
                                "strike {value:?} must use two hexadecimal duration digits"
                            ))
                        })?;
                        if duration == 0 {
                            return Err(ResolvePitchError::new(format!(
                                "strike {value:?} must have a duration from 01 through FF beats"
                            )));
                        }
                        let volume = u8::from_str_radix(&value[4..6], 16).map_err(|_| {
                            ResolvePitchError::new(format!(
                                "strike {value:?} must end with two hexadecimal volume digits"
                            ))
                        })?;
                        (
                            &value[..2],
                            StrikeDuration::ExplicitBeats(
                                NonZeroU8::new(duration)
                                    .expect("zero strike durations were rejected above"),
                            ),
                            Volume::from_byte(volume),
                        )
                    }
                    _ => {
                        return Err(ResolvePitchError::new(format!(
                            "expected a two-character Radler note or six-character note-duration-volume strike; got {value:?}"
                        )))
                    }
                }
            }
            _ => (value, StrikeDuration::VoiceDefault, Volume::from_byte(255)),
        };
        let pitch = match self {
            Self::Periodic(system) => match system.notation() {
                PeriodicNotation::RadlerDigits { place_value } => {
                    PitchNotation::Radler(pitch.parse::<u64>().map_err(|_| {
                        ResolvePitchError::new(format!(
                            "expected non-negative place-value-{} pitch notation; got {pitch:?}",
                            place_value.get()
                        ))
                    })?)
                }
                PeriodicNotation::WesternTwelveTone => {
                    if pitch == "-" || pitch.eq_ignore_ascii_case("rest") {
                        return Ok(None);
                    }
                    PitchNotation::Western(parse_western_note_number(pitch)?)
                }
            },
            Self::Explicit(_) => PitchNotation::Named(pitch.to_owned()),
        };
        Ok(Some(Note {
            pitch: Pitch(pitch),
            duration,
            volume: volume_override.unwrap_or(volume),
        }))
    }

    // An exact explicit key always wins over expression syntax.
    fn split_volume_suffix<'a>(
        &self,
        value: &'a str,
    ) -> Result<(&'a str, Option<Volume>), ResolvePitchError> {
        if match self {
            Self::Explicit(system) => system.pitches.contains_key(value),
            Self::Periodic(_) => false,
        } {
            return Ok((value, None));
        }
        let Some((pitch, suffix)) = value.rsplit_once('@') else {
            return Ok((value, None));
        };
        let volume = if suffix.len() == 2 {
            u8::from_str_radix(suffix, 16).ok()
        } else {
            None
        }
        .ok_or_else(|| {
            ResolvePitchError::new("volume suffix must be @ followed by two hexadecimal digits")
        })?;
        Ok((pitch, Some(Volume::from_byte(volume))))
    }

    pub fn with_note_volume(
        &self,
        value: &str,
        volume: Volume,
    ) -> Result<String, ResolvePitchError> {
        let Some(note) = self.parse_note(value)? else {
            return Ok(value.to_owned());
        };
        self.resolve_note(&note)?;
        if note.volume == volume {
            return Ok(value.to_owned());
        }
        let (base, _) = self.split_volume_suffix(value.trim())?;
        let text = if matches!(&note.pitch.0, PitchNotation::Radler(_)) && base.len() == 6 {
            format!("{}{:02X}", &base[..4], volume.as_byte())
        } else {
            format!("{base}@{:02X}", volume.as_byte())
        };
        let parsed = self.parse_note(&text)?;
        if parsed.as_ref().is_none_or(|parsed| {
            parsed.pitch != note.pitch
                || parsed.duration != note.duration
                || parsed.volume != volume
        }) {
            return Err(ResolvePitchError::new(
                "volume expression conflicts with a named pitch",
            ));
        }
        Ok(text)
    }

    pub fn transpose_note(&self, value: &str, steps: i32) -> Result<String, ResolvePitchError> {
        let Some(note) = self.parse_note(value)? else {
            return Ok(value.to_owned());
        };
        self.resolve_note(&note)?;
        if steps == 0 {
            return Ok(value.to_owned());
        }
        let outside =
            || ResolvePitchError::new("transposition is outside the notation's pitch range");
        let (base, override_volume) = self.split_volume_suffix(value.trim())?;
        let mut text = match (self, &note.pitch.0) {
            (Self::Periodic(system), PitchNotation::Radler(number)) => {
                let PeriodicNotation::RadlerDigits { place_value } = system.notation else {
                    return Err(outside());
                };
                let count = system.degrees.len() as i64;
                let index = (number / place_value.get()) as i64 * count
                    + (number % place_value.get()) as i64
                    + i64::from(steps);
                if index < 0 {
                    return Err(outside());
                }
                let pitch = (index / count) as u64 * place_value.get() + (index % count) as u64;
                if pitch > 99 {
                    return Err(outside());
                }
                format!("{pitch:02}{}", &base[2..])
            }
            (Self::Periodic(_), PitchNotation::Western(number)) => {
                let pitch = i32::from(*number) + steps;
                if !(0..=127).contains(&pitch) {
                    return Err(outside());
                }
                if base.parse::<u8>().is_ok() {
                    pitch.to_string()
                } else {
                    let names = [
                        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                    ];
                    format!("{}{}", names[pitch as usize % 12], pitch / 12 - 1)
                }
            }
            (Self::Explicit(system), PitchNotation::Named(name)) => {
                let mut pitches = system.pitches.iter().collect::<Vec<_>>();
                pitches.sort_by(|a, b| {
                    a.1.as_hz()
                        .total_cmp(&b.1.as_hz())
                        .then_with(|| a.0.cmp(b.0))
                });
                let index = pitches
                    .iter()
                    .position(|(key, _)| *key == name)
                    .ok_or_else(outside)? as i64
                    + i64::from(steps);
                if index < 0 {
                    return Err(outside());
                }
                pitches
                    .get(index as usize)
                    .ok_or_else(outside)?
                    .0
                    .to_string()
            }
            _ => return Err(outside()),
        };
        if let Some(volume) = override_volume {
            text = self.with_note_volume(&text, volume)?;
        }
        self.resolve_strike(&text)?;
        Ok(text)
    }

    pub fn periodic(system: PeriodicPitchSystem) -> Self {
        Self::Periodic(system)
    }

    pub fn explicit(system: ExplicitPitchSystem) -> Self {
        Self::Explicit(system)
    }

    pub fn resolve_pitch(&self, pitch: &Pitch) -> Result<FrequencyHz, ResolvePitchError> {
        match self {
            Self::Periodic(system) => system.resolve(pitch),
            Self::Explicit(system) => system.resolve(pitch),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Periodic(system) => &system.name,
            Self::Explicit(system) => &system.name,
        }
    }

    pub fn western_twelve_tone() -> Self {
        let degrees = (0..12)
            .map(|degree| Interval::cents(f64::from(degree * 100)).unwrap())
            .collect();
        Self::Periodic(
            PeriodicPitchSystem::new(
                "twelve-tone equal temperament",
                FrequencyHz::new(440.0 * 2.0_f64.powf(-69.0 / 12.0)).unwrap(),
                Interval::ratio(2, 1).unwrap(),
                degrees,
                PeriodicNotation::WesternTwelveTone,
            )
            .unwrap(),
        )
    }

    pub(crate) fn append_config(&self, output: &mut String) {
        match self {
            Self::Periodic(system) => {
                output.push_str("\n[pitch_system]\nkind = \"periodic\"\nname = ");
                output.push_str(&toml_string(&system.name));
                output.push_str("\nfundamental_hz = ");
                output.push_str(&system.fundamental.as_hz().to_string());
                output.push_str("\nperiod = ");
                output.push_str(&toml_string(&system.period.config_value()));
                output.push_str("\ndegrees = [");
                for (index, degree) in system.degrees.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&toml_string(&degree.config_value()));
                }
                output.push_str("]\n\n[pitch_system.notation]\n");
                match system.notation {
                    PeriodicNotation::RadlerDigits { place_value } => {
                        output.push_str("kind = \"radler_digits\"\nplace_value = ");
                        output.push_str(&place_value.get().to_string());
                        output.push('\n');
                    }
                    PeriodicNotation::WesternTwelveTone => {
                        output.push_str("kind = \"western_twelve_tone\"\n");
                    }
                }
            }
            Self::Explicit(system) => {
                output.push_str("\n[pitch_system]\nkind = \"explicit\"\nname = ");
                output.push_str(&toml_string(&system.name));
                output.push_str("\n\n[pitch_system.pitches]\n");
                for (token, frequency) in &system.pitches {
                    output.push_str(&toml_string(token));
                    output.push_str(" = ");
                    output.push_str(&frequency.as_hz().to_string());
                    output.push('\n');
                }
            }
        }
    }
}

impl Default for PitchSystem {
    fn default() -> Self {
        Self::western_twelve_tone()
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredPitchSystem {
    Periodic {
        name: String,
        fundamental_hz: f64,
        period: String,
        degrees: Vec<String>,
        notation: StoredPeriodicNotation,
    },
    Explicit {
        name: String,
        pitches: BTreeMap<String, f64>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredPeriodicNotation {
    RadlerDigits { place_value: u64 },
    WesternTwelveTone,
}

impl TryFrom<StoredPitchSystem> for PitchSystem {
    type Error = PitchSystemError;

    fn try_from(stored: StoredPitchSystem) -> Result<Self, Self::Error> {
        match stored {
            StoredPitchSystem::Periodic {
                name,
                fundamental_hz,
                period,
                degrees,
                notation,
            } => {
                let notation = match notation {
                    StoredPeriodicNotation::RadlerDigits { place_value } => {
                        PeriodicNotation::radler_digits(place_value)?
                    }
                    StoredPeriodicNotation::WesternTwelveTone => {
                        PeriodicNotation::WesternTwelveTone
                    }
                };
                let degrees = degrees
                    .iter()
                    .map(|degree| Interval::from_config(degree))
                    .collect::<Result<Vec<_>, _>>()?;
                PeriodicPitchSystem::new(
                    name,
                    FrequencyHz::new(fundamental_hz)?,
                    Interval::from_config(&period)?,
                    degrees,
                    notation,
                )
                .map(Self::Periodic)
            }
            StoredPitchSystem::Explicit { name, pitches } => {
                let pitches = pitches
                    .into_iter()
                    .map(|(token, frequency)| Ok((token, FrequencyHz::new(frequency)?)))
                    .collect::<Result<BTreeMap<_, _>, PitchSystemError>>()?;
                ExplicitPitchSystem::new(name, pitches).map(Self::Explicit)
            }
        }
    }
}

impl<'de> Deserialize<'de> for PitchSystem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        StoredPitchSystem::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PitchSystemError {
    message: String,
}

impl PitchSystemError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn invalid_interval(value: &str) -> Self {
        Self::new(format!(
            "invalid interval {value:?}; expected a positive ratio such as \"3/2\" or cents such as \"700c\""
        ))
    }
}

impl fmt::Display for PitchSystemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PitchSystemError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvePitchError {
    message: String,
}

impl ResolvePitchError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ResolvePitchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ResolvePitchError {}

fn parse_western_note_number(value: &str) -> Result<u8, ResolvePitchError> {
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        let note_number = value
            .parse::<u16>()
            .map_err(|_| invalid_western_pitch(value))?;
        return u8::try_from(note_number)
            .ok()
            .filter(|note| *note <= 127)
            .ok_or_else(|| invalid_western_pitch(value));
    }

    let mut chars = value.chars();
    let letter = chars
        .next()
        .map(|letter| letter.to_ascii_uppercase())
        .ok_or_else(|| invalid_western_pitch(value))?;
    let pitch_class = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(invalid_western_pitch(value)),
    };

    let remainder = chars.as_str();
    let (accidental, octave) = match remainder.as_bytes().first().copied() {
        Some(b'#') => (1, &remainder[1..]),
        Some(b'b') | Some(b'B') => (-1, &remainder[1..]),
        _ => (0, remainder),
    };
    if octave.is_empty() {
        return Err(invalid_western_pitch(value));
    }

    let octave = octave
        .parse::<i16>()
        .map_err(|_| invalid_western_pitch(value))?;
    let note_number = (octave + 1) * 12 + pitch_class + accidental;
    u8::try_from(note_number)
        .ok()
        .filter(|note| *note <= 127)
        .ok_or_else(|| invalid_western_pitch(value))
}

fn invalid_western_pitch(value: &str) -> ResolvePitchError {
    ResolvePitchError::new(format!(
        "expected a note such as C4, C#4, Db4, or a note number from 0 to 127; got {value:?}"
    ))
}

fn toml_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() && u32::from(ch) <= 0xffff => {
                write!(output, "\\u{:04x}", u32::from(ch))
                    .expect("writing to a String cannot fail");
            }
            ch if ch.is_control() => {
                write!(output, "\\U{:08x}", u32::from(ch))
                    .expect("writing to a String cannot fail");
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ExplicitPitchSystem, FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem,
        PitchSystem,
    };

    fn radler(fundamental: f64) -> PitchSystem {
        PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "test",
                FrequencyHz::new(fundamental).unwrap(),
                Interval::ratio(2, 1).unwrap(),
                vec![Interval::ratio(1, 1).unwrap()],
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn transformations_preserve_expression_and_cross_radler_periods() {
        use super::{StrikeDuration, Volume};
        let system = radler(25.0);
        let last = if let PitchSystem::Periodic(p) = &system {
            p.degrees().len() - 1
        } else {
            unreachable!()
        };
        let pitch = format!("{}", 40 + last);
        assert_eq!(system.transpose_note(&pitch, 1).unwrap(), "50");
        assert_eq!(system.transpose_note("40027F", 1).unwrap(), "50027F");
        assert!(system.transpose_note("00", -1).is_err());
        assert_eq!(system.transpose_note("", 1).unwrap(), "");
        let volume = system
            .with_note_volume("40", Volume::from_byte(128))
            .unwrap();
        assert_eq!(volume, "40@80");
        let parsed = system.parse_note(&volume).unwrap().unwrap();
        assert_eq!(parsed.duration(), StrikeDuration::VoiceDefault);
        assert_eq!(parsed.volume().as_byte(), 128);
        assert_eq!(system.transpose_note(&volume, 1).unwrap(), "50@80");
        let western = PitchSystem::western_twelve_tone();
        assert_eq!(western.transpose_note("B4@80", 1).unwrap(), "C5@80");
        assert_eq!(western.transpose_note("rest", 1).unwrap(), "rest");
        assert!(western.transpose_note("127", 1).is_err());
        assert!(western.parse_note("C4@GG").is_err());
    }

    #[test]
    fn parsed_notes_retain_pitch_and_exact_expression_before_tuning() {
        let system = radler(25.0);
        let note = system.parse_note(" 4080ff ").unwrap().unwrap();
        let legacy = system.parse_note("40").unwrap().unwrap();
        assert_eq!(note.pitch(), legacy.pitch());
        assert_eq!(note.duration().explicit_beats(), Some(128));
        assert_eq!(note.volume().as_byte(), 255);
        assert_eq!(legacy.duration(), super::StrikeDuration::VoiceDefault);
        assert_eq!(system.resolve_pitch(note.pitch()).unwrap().as_hz(), 400.0);
        assert_eq!(
            radler(50.0)
                .resolve_note(&note)
                .unwrap()
                .frequency()
                .as_hz(),
            800.0
        );
        assert!(PitchSystem::western_twelve_tone()
            .resolve_note(&note)
            .is_err());

        for duration in 1..=255u8 {
            for volume in [0, 1, 128, 255] {
                let text = format!("40{duration:02x}{volume:02x}");
                let note = system.parse_note(&text).unwrap().unwrap();
                assert_eq!(note.duration().explicit_beats(), Some(duration));
                assert_eq!(note.volume().as_byte(), volume);
                let strike = system.resolve_note(&note).unwrap();
                assert_eq!(strike.frequency().as_hz(), 400.0);
                assert_eq!(strike.volume(), f32::from(volume) / 255.0);
            }
        }
        for invalid in [
            "4000ff", "40ggff", "4080gg", "4080", "4080fff", "é80ff", "😀ff",
        ] {
            assert!(system.parse_note(invalid).is_err(), "{invalid}");
        }
        // Syntactic validity is distinct from membership in the selected tuning.
        let missing_degree = system.parse_note("41").unwrap().unwrap();
        assert!(system.resolve_note(&missing_degree).is_err());
    }

    #[test]
    fn note_parser_preserves_notation_specific_rests_and_named_tokens() {
        let western = PitchSystem::western_twelve_tone();
        assert_eq!(
            western.parse_note("A4").unwrap(),
            western.parse_note("69").unwrap()
        );
        for rest in ["", "  ", "-", "ReSt"] {
            assert_eq!(western.parse_note(rest).unwrap(), None);
        }
        let named = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "named",
                BTreeMap::from([
                    ("4080ff".to_owned(), FrequencyHz::new(123.0).unwrap()),
                    ("rest".to_owned(), FrequencyHz::new(234.0).unwrap()),
                ]),
            )
            .unwrap(),
        );
        let note = named.parse_note("4080ff").unwrap().unwrap();
        assert_eq!(note.duration(), super::StrikeDuration::VoiceDefault);
        assert_eq!(
            named.resolve_note(&note).unwrap().frequency().as_hz(),
            123.0
        );
        assert_eq!(
            named
                .resolve_strike("rest")
                .unwrap()
                .unwrap()
                .frequency()
                .as_hz(),
            234.0
        );
        assert!(named.resolve_strike("4080FF").is_err());
        assert!(radler(25.0).resolve_note(&note).is_err());
        assert!(named
            .resolve_note(&western.parse_note("A4").unwrap().unwrap())
            .is_err());
    }

    #[test]
    fn resolves_radler_digits_with_ratios() {
        let system = PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "slendro sketch",
                FrequencyHz::new(25.0).unwrap(),
                Interval::ratio(2, 1).unwrap(),
                vec![
                    Interval::ratio(1, 1).unwrap(),
                    Interval::ratio(8, 7).unwrap(),
                    Interval::ratio(21, 16).unwrap(),
                    Interval::ratio(32, 21).unwrap(),
                    Interval::ratio(7, 4).unwrap(),
                ],
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        );

        assert_eq!(
            system
                .resolve_strike("34")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap(),
            Some(FrequencyHz::new(350.0).unwrap())
        );
        assert_eq!(
            system
                .resolve_strike("  ")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap(),
            None
        );
        assert!(system
            .resolve_strike("35")
            .map(|strike| strike.map(|strike| strike.frequency()))
            .is_err());

        let legacy = system.resolve_strike("34").unwrap().unwrap();
        assert_eq!(legacy.frequency(), FrequencyHz::new(350.0).unwrap());
        assert_eq!(legacy.duration_beats(), 1);
        assert_eq!(legacy.duration().explicit_beats(), None);
        assert_eq!(legacy.volume(), 1.0);

        let advanced = system.resolve_strike("340880").unwrap().unwrap();
        assert_eq!(advanced.frequency(), FrequencyHz::new(350.0).unwrap());
        assert_eq!(advanced.duration_beats(), 8);
        assert_eq!(advanced.duration().explicit_beats(), Some(8));
        assert!((advanced.volume() - (128.0 / 255.0)).abs() < f32::EPSILON);
        assert_eq!(
            system
                .resolve_strike("340A80")
                .unwrap()
                .unwrap()
                .duration_beats(),
            10
        );
        assert!(system.resolve_strike("340080").is_err());
        assert!(system.resolve_strike("34GG80").is_err());
        assert!(system.resolve_strike("3408GG").is_err());
        assert!(system.resolve_strike("3408").is_err());
    }

    #[test]
    fn resolves_periodic_cents() {
        let system = PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "three equal divisions",
                FrequencyHz::new(100.0).unwrap(),
                Interval::cents(1200.0).unwrap(),
                vec![
                    Interval::cents(0.0).unwrap(),
                    Interval::cents(400.0).unwrap(),
                    Interval::cents(800.0).unwrap(),
                ],
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        );

        let resolved = system
            .resolve_strike("11")
            .map(|strike| strike.map(|strike| strike.frequency()))
            .unwrap()
            .unwrap()
            .as_hz();
        let expected = 200.0 * 2.0_f64.powf(400.0 / 1200.0);
        assert!((resolved - expected).abs() < 1e-10);
    }

    #[test]
    fn explicit_system_uses_arbitrary_case_sensitive_tokens() {
        let system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([
                    ("-".to_string(), FrequencyHz::new(197.3).unwrap()),
                    ("Ember".to_string(), FrequencyHz::new(241.8).unwrap()),
                ]),
            )
            .unwrap(),
        );

        assert_eq!(
            system
                .resolve_strike(" - ")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap(),
            Some(FrequencyHz::new(197.3).unwrap())
        );
        assert_eq!(
            system
                .resolve_strike(" ")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap(),
            None
        );
        assert!(system
            .resolve_strike("ember")
            .map(|strike| strike.map(|strike| strike.frequency()))
            .is_err());
    }

    #[test]
    fn western_compatibility_resolves_notes_numbers_and_historical_rests() {
        let system = PitchSystem::western_twelve_tone();

        assert!(
            (system
                .resolve_strike("A4")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap()
                .unwrap()
                .as_hz()
                - 440.0)
                .abs()
                < 1e-10
        );
        assert!(
            (system
                .resolve_strike("69")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap()
                .unwrap()
                .as_hz()
                - 440.0)
                .abs()
                < 1e-10
        );
        assert_eq!(
            system
                .resolve_strike("rest")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap(),
            None
        );
        assert_eq!(
            system
                .resolve_strike("-")
                .map(|strike| strike.map(|strike| strike.frequency()))
                .unwrap(),
            None
        );
        assert!(system
            .resolve_strike("H4")
            .map(|strike| strike.map(|strike| strike.frequency()))
            .is_err());
    }

    #[test]
    fn rejects_invalid_pitch_system_boundaries() {
        assert_eq!(
            FrequencyHz::from_config(" 197.3 ").unwrap(),
            FrequencyHz::new(197.3).unwrap()
        );
        assert!(FrequencyHz::from_config("not a frequency").is_err());
        assert!(FrequencyHz::new(0.0).is_err());
        assert!(FrequencyHz::new(f64::NAN).is_err());
        assert!(FrequencyHz::new(f64::INFINITY).is_err());
        assert!(Interval::ratio(1, 0).is_err());
        assert!(PeriodicPitchSystem::new(
            "empty",
            FrequencyHz::new(100.0).unwrap(),
            Interval::ratio(2, 1).unwrap(),
            Vec::new(),
            PeriodicNotation::radler_digits(10).unwrap(),
        )
        .is_err());
    }
}
