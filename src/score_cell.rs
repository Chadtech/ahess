//! One editable score value, with optional exact sub-beat events.
use crate::pitch_system::{Note, PitchSystem, ResolvePitchError, Strike, StrikeDuration, Volume};
use serde::{Deserialize, Serialize};

pub const TICKS_PER_BEAT: i32 = 96;
const PREFIX: &str = "!notes:";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BeatOffset(i32);
impl BeatOffset {
    pub fn from_ticks(ticks: i32) -> Result<Self, ResolvePitchError> {
        if (-96..96).contains(&ticks) {
            Ok(Self(ticks))
        } else {
            Err(error(
                "note offsets must be between -1 beat and less than +1 beat",
            ))
        }
    }
    pub fn ticks(self) -> i32 {
        self.0
    }
    pub fn beats(self) -> f64 {
        f64::from(self.0) / 96.0
    }
}

pub fn fraction(ticks: i32) -> String {
    if ticks == 0 {
        return "0".into();
    }
    let mut n = ticks.abs();
    let mut d = TICKS_PER_BEAT;
    let (mut a, mut b) = (n, d);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    n /= a;
    d /= a;
    let sign = if ticks < 0 { "-" } else { "" };
    if d == 1 {
        format!("{sign}{n}")
    } else {
        format!("{sign}{n}/{d}")
    }
}

pub fn duration_from_text(text: &str) -> Result<StrikeDuration, ResolvePitchError> {
    let text = text.trim();
    if text.is_empty() || text == "default" {
        return Ok(StrikeDuration::VoiceDefault);
    }
    let value = if let Some((n, d)) = text.split_once('/') {
        n.parse::<f64>().map_err(|_| error("invalid duration"))?
            / d.parse::<f64>().map_err(|_| error("invalid duration"))?
    } else {
        text.parse::<f64>().map_err(|_| error("invalid duration"))?
    };
    let ticks = value * 96.0;
    if !ticks.is_finite()
        || !(1.0..=24480.0).contains(&ticks)
        || (ticks - ticks.round()).abs() > 1e-7
    {
        return Err(error(
            "duration must be positive, at most 255 beats, and a multiple of 1/96 beat",
        ));
    }
    Ok(StrikeDuration::FractionalBeats(
        crate::pitch_system::BeatDurationTicks::new(ticks.round() as u32)?,
    ))
}

pub fn duration_text(duration: StrikeDuration) -> String {
    match duration {
        StrikeDuration::VoiceDefault => "default".into(),
        _ => fraction((duration.beats() * 96.0).round() as i32),
    }
}

#[derive(Clone, Debug)]
pub struct CellEvent {
    pub offset: BeatOffset,
    pub notation: String,
    pub note: Note,
    pub attack_sharpness: Option<crate::voice::AttackSharpness>,
}
impl CellEvent {
    pub fn strike(&self, system: &PitchSystem) -> Result<Strike, ResolvePitchError> {
        system.resolve_note(&self.note)
    }
    pub fn from_fields(
        system: &PitchSystem,
        offset: BeatOffset,
        pitch: &str,
        duration: &str,
        volume: &str,
    ) -> Result<Option<Self>, ResolvePitchError> {
        let Some(note) = system.parse_note(pitch)? else {
            return Ok(None);
        };
        if note.duration() != StrikeDuration::VoiceDefault || note.volume().as_byte() != 255 {
            return Err(error(
                "enter only a pitch here; use the duration and volume columns for its details",
            ));
        }
        let duration = duration_from_text(duration)?;
        let volume = if volume.trim().is_empty() {
            note.volume()
        } else {
            let v = volume.trim();
            if v.len() != 2 {
                return Err(error(
                    "volume must be two hexadecimal digits, 00 through FF",
                ));
            }
            Volume::from_byte(
                u8::from_str_radix(v, 16)
                    .map_err(|_| error("volume must be two hexadecimal digits, 00 through FF"))?,
            )
        };
        let note = note.with_details(duration, volume);
        system.resolve_note(&note)?;
        Ok(Some(Self {
            offset,
            notation: system.pitch_text(note.pitch()),
            note,
            attack_sharpness: None,
        }))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEvent {
    offset: i32,
    pitch: String,
    duration: String,
    volume: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attack_sharpness: Option<crate::voice::AttackSharpness>,
}

pub fn has_details(value: &str) -> bool {
    value.trim().starts_with(PREFIX)
}

pub fn parse(system: &PitchSystem, value: &str) -> Result<Vec<CellEvent>, ResolvePitchError> {
    // Exact explicit tuning keys retain priority over expression syntax.
    if let Some(encoded) = value
        .trim()
        .strip_prefix(PREFIX)
        .filter(|_| !system.is_exact_key(value.trim()))
    {
        let rows: Vec<StoredEvent> = serde_json::from_str(encoded)
            .map_err(|e| error(format!("invalid note details: {e}")))?;
        if rows.len() > 192 {
            return Err(error("a cell may contain at most 192 note events"));
        }
        let mut events = Vec::new();
        for row in rows {
            let mut event = CellEvent::from_fields(
                system,
                BeatOffset::from_ticks(row.offset)?,
                &row.pitch,
                &row.duration,
                &format!("{:02X}", row.volume),
            )?
            .ok_or_else(|| error("a stored note event must have a pitch"))?;
            event.attack_sharpness = row.attack_sharpness;
            events.push(event);
        }
        events.sort_by_key(|e| e.offset);
        if events
            .windows(2)
            .any(|pair| pair[0].offset == pair[1].offset)
        {
            return Err(error("each fractional position may contain only one note"));
        }
        Ok(events)
    } else {
        let Some(note) = system.parse_note(value)? else {
            return Ok(Vec::new());
        };
        system.resolve_note(&note)?;
        Ok(vec![CellEvent {
            offset: BeatOffset(0),
            notation: system.pitch_text(note.pitch()),
            note,
            attack_sharpness: None,
        }])
    }
}

pub fn encode(events: &[CellEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }
    let rows: Vec<_> = events
        .iter()
        .map(|e| StoredEvent {
            offset: e.offset.0,
            pitch: e.notation.clone(),
            duration: duration_text(e.note.duration()),
            volume: e.note.volume().as_byte(),
            attack_sharpness: e.attack_sharpness,
        })
        .collect();
    format!(
        "{PREFIX}{}",
        serde_json::to_string(&rows).expect("note detail values are serializable")
    )
}

pub fn summary(value: &str) -> Option<String> {
    let rows: Vec<StoredEvent> = serde_json::from_str(value.trim().strip_prefix(PREFIX)?).ok()?;
    Some(
        rows.iter()
            .find(|e| e.offset == 0)
            .or(rows.first())
            .map(|e| e.pitch.clone())
            .unwrap_or_default(),
    )
}
fn error(message: impl Into<String>) -> ResolvePitchError {
    ResolvePitchError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attack_override_round_trips_and_rejects_out_of_range_values() {
        let system = PitchSystem::western_twelve_tone();
        let mut events = parse(&system, "C4").unwrap();
        assert!(events[0].attack_sharpness.is_none());
        for value in [0, 37, 100] {
            events[0].attack_sharpness = Some(crate::voice::AttackSharpness::new(value).unwrap());
            let encoded = encode(&events);
            assert_eq!(
                parse(&system, &encoded).unwrap()[0].attack_sharpness,
                events[0].attack_sharpness
            );
        }
        let invalid =
            encode(&events).replace("\"attack_sharpness\":100", "\"attack_sharpness\":101");
        assert!(parse(&system, &invalid).is_err());
    }
    #[test]
    fn fractions_are_exact_and_bounded() {
        assert_eq!(fraction(-24), "-1/4");
        assert_eq!(duration_from_text("1/3").unwrap().beats(), 1.0 / 3.0);
        for text in ["0", "-1", "NaN", "1/0", "256", "1/7"] {
            assert!(duration_from_text(text).is_err(), "{text}");
        }
        assert!(BeatOffset::from_ticks(96).is_err());
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;
    #[test]
    fn cell_round_trip_validates_every_note_and_preserves_explicit_duration() {
        let system = PitchSystem::western_twelve_tone();
        let events = vec![
            CellEvent::from_fields(
                &system,
                BeatOffset::from_ticks(-24).unwrap(),
                "C4",
                "1/4",
                "80",
            )
            .unwrap()
            .unwrap(),
            CellEvent::from_fields(
                &system,
                BeatOffset::from_ticks(0).unwrap(),
                "G4",
                "default",
                "FF",
            )
            .unwrap()
            .unwrap(),
            CellEvent::from_fields(
                &system,
                BeatOffset::from_ticks(48).unwrap(),
                "E4",
                "1/3",
                "B0",
            )
            .unwrap()
            .unwrap(),
        ];
        let value = encode(&events);
        let parsed = parse(&system, &value).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].offset.ticks(), -24);
        assert_eq!(parsed[2].note.duration().beats(), 1.0 / 3.0);
        assert_eq!(parsed[0].note.volume().as_byte(), 128);
        assert_eq!(encode(&parsed), value);
        assert!(system.resolve_strike(&value).unwrap().is_some());
        assert!(parse(&system, &value.replace("\"64\"", "\"not-a-pitch\"")).is_err());
        assert!(parse(&system, &encode(&[events[0].clone(), events[0].clone()])).is_err());
    }
}
