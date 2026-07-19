use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Write as _},
    num::NonZeroU64,
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

    fn resolve(&self, value: &str) -> Result<Option<FrequencyHz>, ResolvePitchError> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(None);
        }

        let (period_index, degree_index) = match self.notation {
            PeriodicNotation::RadlerDigits { place_value } => {
                let notation = value.parse::<u64>().map_err(|_| {
                    ResolvePitchError::new(format!(
                        "expected non-negative place-value-{} pitch notation; got {value:?}",
                        place_value.get()
                    ))
                })?;
                let degree = notation % place_value.get();
                if degree >= self.degrees.len() as u64 {
                    return Err(ResolvePitchError::new(format!(
                        "pitch {value:?} uses degree {degree}, but {:?} has degrees 0 through {}",
                        self.name,
                        self.degrees.len() - 1
                    )));
                }
                (notation / place_value.get(), degree as usize)
            }
            PeriodicNotation::WesternTwelveTone => {
                if value == "-" || value.eq_ignore_ascii_case("rest") {
                    return Ok(None);
                }
                let note_number = parse_western_note_number(value)?;
                (u64::from(note_number / 12), usize::from(note_number % 12))
            }
        };

        let period_multiplier = self.period.multiplier().powf(period_index as f64);
        let degree_multiplier = self.degrees[degree_index].multiplier();
        FrequencyHz::new(self.fundamental.as_hz() * period_multiplier * degree_multiplier)
            .map(Some)
            .map_err(|_| {
                ResolvePitchError::new(format!(
                    "pitch {value:?} resolves outside the supported frequency range"
                ))
            })
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

    fn resolve(&self, value: &str) -> Result<Option<FrequencyHz>, ResolvePitchError> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(None);
        }
        self.pitches.get(value).copied().map(Some).ok_or_else(|| {
            ResolvePitchError::new(format!("pitch {value:?} is not defined in {:?}", self.name))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PitchSystem {
    Periodic(PeriodicPitchSystem),
    Explicit(ExplicitPitchSystem),
}

impl PitchSystem {
    pub fn periodic(system: PeriodicPitchSystem) -> Self {
        Self::Periodic(system)
    }

    pub fn explicit(system: ExplicitPitchSystem) -> Self {
        Self::Explicit(system)
    }

    pub fn resolve_cell(&self, value: &str) -> Result<Option<FrequencyHz>, ResolvePitchError> {
        match self {
            Self::Periodic(system) => system.resolve(value),
            Self::Explicit(system) => system.resolve(value),
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
    fn new(message: impl Into<String>) -> Self {
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
            system.resolve_cell("34").unwrap(),
            Some(FrequencyHz::new(350.0).unwrap())
        );
        assert_eq!(system.resolve_cell("  ").unwrap(), None);
        assert!(system.resolve_cell("35").is_err());
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

        let resolved = system.resolve_cell("11").unwrap().unwrap().as_hz();
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
            system.resolve_cell(" - ").unwrap(),
            Some(FrequencyHz::new(197.3).unwrap())
        );
        assert_eq!(system.resolve_cell(" ").unwrap(), None);
        assert!(system.resolve_cell("ember").is_err());
    }

    #[test]
    fn western_compatibility_resolves_notes_numbers_and_historical_rests() {
        let system = PitchSystem::western_twelve_tone();

        assert!((system.resolve_cell("A4").unwrap().unwrap().as_hz() - 440.0).abs() < 1e-10);
        assert!((system.resolve_cell("69").unwrap().unwrap().as_hz() - 440.0).abs() < 1e-10);
        assert_eq!(system.resolve_cell("rest").unwrap(), None);
        assert_eq!(system.resolve_cell("-").unwrap(), None);
        assert!(system.resolve_cell("H4").is_err());
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
