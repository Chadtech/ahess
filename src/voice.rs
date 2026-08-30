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
    HarmonicSaw,
    #[serde(alias = "surge-xt")]
    SurgeXtPiano,
    SurgeXtDistortedElectricGuitar,
}

impl VoiceType {
    pub const ALL: [Self; 5] = [
        Self::Sin,
        Self::Saw,
        Self::HarmonicSaw,
        Self::SurgeXtPiano,
        Self::SurgeXtDistortedElectricGuitar,
    ];
    #[cfg(test)]
    pub(crate) const BUILT_IN: [Self; 3] = [Self::Sin, Self::Saw, Self::HarmonicSaw];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
            Self::HarmonicSaw => "harmonic saw",
            Self::SurgeXtPiano => "Surge XT Piano",
            Self::SurgeXtDistortedElectricGuitar => "Surge XT distorted electric guitar",
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
            Self::HarmonicSaw => "harmonic-saw",
            Self::SurgeXtPiano => "surge-xt-piano",
            Self::SurgeXtDistortedElectricGuitar => "surge-xt-distorted-electric-guitar",
        }
    }

    pub(crate) const fn uses_surge_xt(self) -> bool {
        matches!(
            self,
            Self::SurgeXtPiano | Self::SurgeXtDistortedElectricGuitar
        )
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::VoiceType;

    #[derive(Deserialize)]
    struct StoredVoiceType {
        voice_type: VoiceType,
    }

    #[test]
    fn generic_surge_xt_drafts_migrate_to_the_concrete_piano_voice() {
        let stored: StoredVoiceType = toml::from_str("voice_type = \"surge-xt\"").unwrap();

        assert_eq!(stored.voice_type, VoiceType::SurgeXtPiano);
        assert_eq!(stored.voice_type.config_value(), "surge-xt-piano");
    }
}
