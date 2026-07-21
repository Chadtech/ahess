use serde::Deserialize;

use crate::{acoustics::Point3Meters, voice_name::VoiceName};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(u64);

impl VoiceId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    id: VoiceId,
    pub name: VoiceName,
    pub voice_type: VoiceType,
    position: Point3Meters,
}

impl Voice {
    pub fn new(id: impl Into<VoiceId>, name: impl Into<VoiceName>, voice_type: VoiceType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            voice_type,
            position: Point3Meters::origin(),
        }
    }

    pub const fn id(&self) -> VoiceId {
        self.id
    }

    pub const fn position(&self) -> Point3Meters {
        self.position
    }

    pub fn with_position(mut self, position: Point3Meters) -> Self {
        self.position = position;
        self
    }
}

impl From<u64> for VoiceId {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceType {
    Sin,
    Saw,
}

impl VoiceType {
    pub const ALL: [Self; 2] = [Self::Sin, Self::Saw];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
        }
    }
}
