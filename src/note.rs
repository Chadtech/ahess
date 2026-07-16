use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Note {
    midi: u8,
}

impl Note {
    pub const fn from_midi(midi: u8) -> Self {
        Self { midi }
    }

    pub const fn midi(self) -> u8 {
        self.midi
    }

    pub fn parse_cell(value: &str) -> Result<Option<Self>, ParseNoteError> {
        let value = value.trim();
        if value.is_empty() || value == "-" || value.eq_ignore_ascii_case("rest") {
            return Ok(None);
        }

        if value.bytes().all(|byte| byte.is_ascii_digit()) {
            let midi = value
                .parse::<u16>()
                .map_err(|_| ParseNoteError::new(value))?;
            if midi > 127 {
                return Err(ParseNoteError::new(value));
            }
            return Ok(Some(Self::from_midi(midi as u8)));
        }

        parse_named_note(value).map(Some)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseNoteError {
    value: String,
}

impl ParseNoteError {
    fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl fmt::Display for ParseNoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expected a note such as C4, C#4, Db4, or a MIDI number from 0 to 127; got {:?}",
            self.value
        )
    }
}

impl Error for ParseNoteError {}

fn parse_named_note(value: &str) -> Result<Note, ParseNoteError> {
    let mut chars = value.chars();
    let letter = chars
        .next()
        .map(|letter| letter.to_ascii_uppercase())
        .ok_or_else(|| ParseNoteError::new(value))?;
    let pitch_class = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(ParseNoteError::new(value)),
    };

    let remainder = chars.as_str();
    let (accidental, octave) = match remainder.as_bytes().first().copied() {
        Some(b'#') => (1, &remainder[1..]),
        Some(b'b') | Some(b'B') => (-1, &remainder[1..]),
        _ => (0, remainder),
    };
    if octave.is_empty() {
        return Err(ParseNoteError::new(value));
    }

    let octave = octave
        .parse::<i16>()
        .map_err(|_| ParseNoteError::new(value))?;
    let midi = (octave + 1) * 12 + pitch_class + accidental;
    if !(0..=127).contains(&midi) {
        return Err(ParseNoteError::new(value));
    }

    Ok(Note::from_midi(midi as u8))
}

#[cfg(test)]
mod tests {
    use super::Note;

    #[test]
    fn parses_named_notes_accidentals_and_midi_numbers() {
        assert_eq!(Note::parse_cell("C4").unwrap(), Some(Note::from_midi(60)));
        assert_eq!(
            Note::parse_cell(" c#4 ").unwrap(),
            Some(Note::from_midi(61))
        );
        assert_eq!(Note::parse_cell("Db4").unwrap(), Some(Note::from_midi(61)));
        assert_eq!(Note::parse_cell("A4").unwrap(), Some(Note::from_midi(69)));
        assert_eq!(Note::parse_cell("127").unwrap(), Some(Note::from_midi(127)));
    }

    #[test]
    fn parses_blank_and_explicit_rests() {
        assert_eq!(Note::parse_cell("").unwrap(), None);
        assert_eq!(Note::parse_cell(" - ").unwrap(), None);
        assert_eq!(Note::parse_cell("REST").unwrap(), None);
    }

    #[test]
    fn rejects_invalid_or_out_of_range_notes() {
        assert!(Note::parse_cell("H4").is_err());
        assert!(Note::parse_cell("C").is_err());
        assert!(Note::parse_cell("C-2").is_err());
        assert!(Note::parse_cell("128").is_err());
    }
}
