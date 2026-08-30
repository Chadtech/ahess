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
    volume_adjustment: Option<VoiceVolumeAdjustment>,
}

impl Voice {
    pub fn new(id: impl Into<VoiceId>, name: impl Into<VoiceName>, voice_type: VoiceType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            voice_type,
            position: Point3Meters::origin(),
            volume_adjustment: None,
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

    pub const fn volume_adjustment(&self) -> Option<VoiceVolumeAdjustment> {
        self.volume_adjustment
    }

    pub fn with_volume_adjustment(
        mut self,
        volume_adjustment: Option<VoiceVolumeAdjustment>,
    ) -> Self {
        self.volume_adjustment = volume_adjustment;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Deserialize)]
#[serde(try_from = "f64")]
pub struct VoiceVolumeAdjustment(f32);

impl VoiceVolumeAdjustment {
    pub fn new(multiplier: f64) -> Result<Self, VoiceVolumeAdjustmentError> {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(VoiceVolumeAdjustmentError);
        }
        let multiplier = multiplier as f32;
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(VoiceVolumeAdjustmentError);
        }

        Ok(Self(multiplier))
    }

    pub const fn multiplier(self) -> f32 {
        self.0
    }
}

// `VoiceVolumeAdjustment::new` excludes NaN, so equality is reflexive.
impl Eq for VoiceVolumeAdjustment {}

impl TryFrom<f64> for VoiceVolumeAdjustment {
    type Error = VoiceVolumeAdjustmentError;

    fn try_from(multiplier: f64) -> Result<Self, Self::Error> {
        Self::new(multiplier)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceVolumeAdjustmentError;

impl std::fmt::Display for VoiceVolumeAdjustmentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("voice volume adjustment must be a finite decimal greater than zero")
    }
}

impl std::error::Error for VoiceVolumeAdjustmentError {}

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
    NoitechBellA,
    #[serde(alias = "surge-xt")]
    SurgeXtPiano,
    SurgeXtDistortedElectricGuitar,
    SurgeXtClarinet,
}

impl VoiceType {
    pub const ALL: [Self; 7] = [
        Self::Sin,
        Self::Saw,
        Self::HarmonicSaw,
        Self::NoitechBellA,
        Self::SurgeXtPiano,
        Self::SurgeXtDistortedElectricGuitar,
        Self::SurgeXtClarinet,
    ];
    #[cfg(test)]
    pub(crate) const BUILT_IN: [Self; 4] =
        [Self::Sin, Self::Saw, Self::HarmonicSaw, Self::NoitechBellA];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
            Self::HarmonicSaw => "harmonic saw",
            Self::NoitechBellA => "Noitech Bell A",
            Self::SurgeXtPiano => "Surge XT Piano",
            Self::SurgeXtDistortedElectricGuitar => "Surge XT distorted electric guitar",
            Self::SurgeXtClarinet => "Surge XT clarinet",
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
            Self::HarmonicSaw => "harmonic-saw",
            Self::NoitechBellA => "noitech-bell-a",
            Self::SurgeXtPiano => "surge-xt-piano",
            Self::SurgeXtDistortedElectricGuitar => "surge-xt-distorted-electric-guitar",
            Self::SurgeXtClarinet => "surge-xt-clarinet",
        }
    }

    pub(crate) const fn uses_surge_xt(self) -> bool {
        matches!(
            self,
            Self::SurgeXtPiano | Self::SurgeXtDistortedElectricGuitar | Self::SurgeXtClarinet
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
