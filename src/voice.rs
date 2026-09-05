use serde::Deserialize;

use crate::{acoustics::Point3Meters, voice_name::VoiceName};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceDetails {
    pub description: &'static str,
    pub source: &'static str,
    pub fidelity: &'static str,
}

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

    pub const fn details(&self) -> VoiceDetails {
        self.voice_type.details()
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
    GamelanMetallophone,
    NoitechBellA,
    NoitechBellB,
    NoitechBellG,
    NoitechBellH,
    #[serde(rename = "noitech-bell-h-v2")]
    NoitechBellHV2,
    NoitechBellI,
    NoitechBellJ,
    NoitechBellK,
    NoitechBellL,
    NoitechBellM,
    IconoclastBellG,
    IconoclastBellH,
    IconoclastIndustrialBar,
    CtpianoBars,
    CtpianoDkSquare,
    CtpianoEmphaenharm,
    CtpianoHiSaw,
    CtpianoLoSaw,
    CtpianoLoSquare,
    CtpianoTriangleDrop,
    RadlerDullSaw,
    RadlerHarmonics,
    LegacyNoitechEnharmonic,
    #[serde(alias = "surge-xt")]
    SurgeXtPiano,
    SurgeXtDistortedElectricGuitar,
    SurgeXtClarinet,
}

impl VoiceType {
    pub const ALL: [Self; 30] = [
        Self::Sin,
        Self::Saw,
        Self::HarmonicSaw,
        Self::GamelanMetallophone,
        Self::NoitechBellA,
        Self::NoitechBellB,
        Self::NoitechBellG,
        Self::NoitechBellH,
        Self::NoitechBellHV2,
        Self::NoitechBellI,
        Self::NoitechBellJ,
        Self::NoitechBellK,
        Self::NoitechBellL,
        Self::NoitechBellM,
        Self::IconoclastBellG,
        Self::IconoclastBellH,
        Self::IconoclastIndustrialBar,
        Self::CtpianoBars,
        Self::CtpianoDkSquare,
        Self::CtpianoEmphaenharm,
        Self::CtpianoHiSaw,
        Self::CtpianoLoSaw,
        Self::CtpianoLoSquare,
        Self::CtpianoTriangleDrop,
        Self::RadlerDullSaw,
        Self::RadlerHarmonics,
        Self::LegacyNoitechEnharmonic,
        Self::SurgeXtPiano,
        Self::SurgeXtDistortedElectricGuitar,
        Self::SurgeXtClarinet,
    ];
    #[cfg(test)]
    pub(crate) const BUILT_IN: [Self; 27] = [
        Self::Sin,
        Self::Saw,
        Self::HarmonicSaw,
        Self::GamelanMetallophone,
        Self::NoitechBellA,
        Self::NoitechBellB,
        Self::NoitechBellG,
        Self::NoitechBellH,
        Self::NoitechBellHV2,
        Self::NoitechBellI,
        Self::NoitechBellJ,
        Self::NoitechBellK,
        Self::NoitechBellL,
        Self::NoitechBellM,
        Self::IconoclastBellG,
        Self::IconoclastBellH,
        Self::IconoclastIndustrialBar,
        Self::CtpianoBars,
        Self::CtpianoDkSquare,
        Self::CtpianoEmphaenharm,
        Self::CtpianoHiSaw,
        Self::CtpianoLoSaw,
        Self::CtpianoLoSquare,
        Self::CtpianoTriangleDrop,
        Self::RadlerDullSaw,
        Self::RadlerHarmonics,
        Self::LegacyNoitechEnharmonic,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
            Self::HarmonicSaw => "harmonic saw",
            Self::GamelanMetallophone => "gamelan metallophone",
            Self::NoitechBellA => "Noitech Bell A",
            Self::NoitechBellB => "Noitech Bell B",
            Self::NoitechBellG => "Noitech Bell G",
            Self::NoitechBellH => "Noitech Bell H",
            Self::NoitechBellHV2 => "Noitech Bell H v2",
            Self::NoitechBellI => "Noitech Bell I",
            Self::NoitechBellJ => "Noitech Bell J",
            Self::NoitechBellK => "Noitech Bell K",
            Self::NoitechBellL => "Noitech Bell L",
            Self::NoitechBellM => "Noitech Bell M",
            Self::IconoclastBellG => "Iconoclast Bell G",
            Self::IconoclastBellH => "Iconoclast Bell H",
            Self::IconoclastIndustrialBar => "Iconoclast industrial bar",
            Self::CtpianoBars => "Ctpiano bars",
            Self::CtpianoDkSquare => "Ctpiano DK square",
            Self::CtpianoEmphaenharm => "Ctpiano emphaenharm",
            Self::CtpianoHiSaw => "Ctpiano hi saw",
            Self::CtpianoLoSaw => "Ctpiano lo saw",
            Self::CtpianoLoSquare => "Ctpiano lo square",
            Self::CtpianoTriangleDrop => "Ctpiano triangle drop",
            Self::RadlerDullSaw => "Radler dull saw",
            Self::RadlerHarmonics => "Radler harmonics",
            Self::LegacyNoitechEnharmonic => "legacy Noitech enharmonic triangle",
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
            Self::GamelanMetallophone => "gamelan-metallophone",
            Self::NoitechBellA => "noitech-bell-a",
            Self::NoitechBellB => "noitech-bell-b",
            Self::NoitechBellG => "noitech-bell-g",
            Self::NoitechBellH => "noitech-bell-h",
            Self::NoitechBellHV2 => "noitech-bell-h-v2",
            Self::NoitechBellI => "noitech-bell-i",
            Self::NoitechBellJ => "noitech-bell-j",
            Self::NoitechBellK => "noitech-bell-k",
            Self::NoitechBellL => "noitech-bell-l",
            Self::NoitechBellM => "noitech-bell-m",
            Self::IconoclastBellG => "iconoclast-bell-g",
            Self::IconoclastBellH => "iconoclast-bell-h",
            Self::IconoclastIndustrialBar => "iconoclast-industrial-bar",
            Self::CtpianoBars => "ctpiano-bars",
            Self::CtpianoDkSquare => "ctpiano-dk-square",
            Self::CtpianoEmphaenharm => "ctpiano-emphaenharm",
            Self::CtpianoHiSaw => "ctpiano-hi-saw",
            Self::CtpianoLoSaw => "ctpiano-lo-saw",
            Self::CtpianoLoSquare => "ctpiano-lo-square",
            Self::CtpianoTriangleDrop => "ctpiano-triangle-drop",
            Self::RadlerDullSaw => "radler-dull-saw",
            Self::RadlerHarmonics => "radler-harmonics",
            Self::LegacyNoitechEnharmonic => "legacy-noitech-enharmonic",
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

    pub(crate) const fn uses_recovered_runtime(self) -> bool {
        matches!(
            self,
            Self::NoitechBellG
                | Self::NoitechBellH
                | Self::NoitechBellHV2
                | Self::NoitechBellI
                | Self::NoitechBellJ
                | Self::NoitechBellK
                | Self::NoitechBellL
                | Self::NoitechBellM
                | Self::IconoclastBellG
                | Self::IconoclastBellH
                | Self::IconoclastIndustrialBar
                | Self::CtpianoBars
                | Self::CtpianoDkSquare
                | Self::CtpianoEmphaenharm
                | Self::CtpianoHiSaw
                | Self::CtpianoLoSaw
                | Self::CtpianoLoSquare
                | Self::CtpianoTriangleDrop
                | Self::LegacyNoitechEnharmonic
        )
    }

    pub const fn details(self) -> VoiceDetails {
        match self {
            Self::Sin => VoiceDetails {
                description: "A single sine oscillator shaped by the project beat envelope.",
                source: "Ahess built-in voice",
                fidelity: "Native Ahess implementation.",
            },
            Self::Saw => VoiceDetails {
                description: "A direct saw oscillator shaped by the project beat envelope.",
                source: "Ahess built-in voice",
                fidelity: "Native Ahess implementation.",
            },
            Self::HarmonicSaw => VoiceDetails {
                description: "A band-limited additive saw assembled from sine harmonics.",
                source: "Ahess built-in voice",
                fidelity: "Native Ahess implementation.",
            },
            Self::GamelanMetallophone => VoiceDetails {
                description: "A bronze-bar voice with a dense sine-built mallet-noise impulse, measured gamelan-like inharmonic modes, resonator tones, independent fade-outs, and restrained four-hertz ombak shimmer.",
                source: "Ahess original voice informed by published measurements of Balinese gangsa and Javanese gender/saron spectra",
                fidelity: "Purpose-built additive model rather than a sampled or recovered historical instrument; every audible component is synthesized from sine waves.",
            },
            Self::NoitechBellA => VoiceDetails {
                description: "Sixteen sine components, nested fades, and a short square-wave strike.",
                source: "Chadtech/Ntv1.bYhS2 — bells20150804/buildBells.coffee",
                fidelity: "Source-faithful fixed five-second body with overlapping tails.",
            },
            Self::NoitechBellB => VoiceDetails {
                description: "Seven audible sine components with independent durations and fades.",
                source: "Chadtech/Ntv1.bYhS2 — bells20150804/buildBellsB.coffee",
                fidelity: "Source-faithful four-second body; fractional sample-count enharmonics remain silent.",
            },
            Self::NoitechBellG => VoiceDetails {
                description: "A broad bell with a subharmonic, thirteen main partials, nested fades, and tiny enharmonic attacks.",
                source: "Chadtech/BellsJobot — buildBellsG.coffee",
                fidelity: "Native additive core with the source expensiveE.wav dry-plus-convolved response restored at its 0.15 gain.",
            },
            Self::NoitechBellH => VoiceDetails {
                description: "A leaner Bell G relative with phase-shifted low components and nested fades.",
                source: "Chadtech/BellsJobot — buildBellsH.coffee",
                fidelity: "Native additive core with the source expensiveE.wav dry-plus-convolved response restored at its 0.25 gain.",
            },
            Self::NoitechBellHV2 => VoiceDetails {
                description: "Bell H with gently beating bronze resonances and settling gong-like upper tones. Note volume controls strike strength: soft notes are rounder; hard notes bring out upper tones and shimmer.",
                source: "Ahess variation on Chadtech/BellsJobot — buildBellsH.coffee; src/recovered_voice.rs",
                fidelity: "An original church-bell and gong-inspired variation, retaining Bell H's nine core modes and expensiveE.wav convolution at 0.25; not a measured physical model.",
            },
            Self::NoitechBellI => VoiceDetails {
                description: "A tidy eight-component Bell G descendant with short upper-partial decays.",
                source: "Chadtech/bellsTemplate — buildBellsI.coffee",
                fidelity: "Native additive core; fractional sample-count enharmonics remain silent, and the source expensiveE.wav response is restored at its 0.15 gain.",
            },
            Self::NoitechBellJ => VoiceDetails {
                description: "A long, sustained thirteen-component bell with stretched upper partials.",
                source: "Chadtech/bellsTemplate — buildBellsJ.coffee",
                fidelity: "Native additive core; fractional sample-count enharmonics remain silent, and the source expensiveE.wav response is restored at its 0.15 gain.",
            },
            Self::NoitechBellK => VoiceDetails {
                description: "A clean seven-component stretched harmonic ladder.",
                source: "Chadtech/bellsTemplate — buildBellsK.coffee",
                fidelity: "Native additive core with the source expensiveE.wav dry-plus-convolved response restored at its 0.15 gain.",
            },
            Self::NoitechBellL => VoiceDetails {
                description: "A sparse bell made from ratios 1, 2.01, and 4.04.",
                source: "Chadtech/Iconoclast — old-voices/buildBellsL.coffee",
                fidelity: "Native additive core with the source home_clap_1.wav dry-plus-convolved response restored at its 0.05 gain.",
            },
            Self::NoitechBellM => VoiceDetails {
                description: "A slightly stretched seven-component quasi-harmonic ladder.",
                source: "Chadtech/Iconoclast — old-voices/buildBellsM.coffee",
                fidelity: "Native additive core with the source home_clap_1.wav dry-plus-convolved response restored at its 0.05 gain.",
            },
            Self::IconoclastBellG => VoiceDetails {
                description: "The later live Bell G rewrite with thirteen explicit partials and pitch instability.",
                source: "Chadtech/Iconoclast — bells-G.coffee",
                fidelity: "Preserves the additive profile, fade powers, and deterministic per-note detuning.",
            },
            Self::IconoclastBellH => VoiceDetails {
                description: "The later compact Bell H rewrite with nine components and phase offsets.",
                source: "Chadtech/Iconoclast — bells-H.coffee",
                fidelity: "Preserves the additive profile, phase offsets, and nested fades.",
            },
            Self::IconoclastIndustrialBar => VoiceDetails {
                description: "A short inharmonic metal-bar profile spanning ratios 0.5 through 9.2.",
                source: "Chadtech/Iconoclast — bells-R.coffee",
                fidelity: "Repairs the source dir/dur typo and retains the intended per-component durations.",
            },
            Self::CtpianoBars => VoiceDetails {
                description: "Layered decaying enharmonic triangle banks with a sine fundamental.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe Bars/JITEuropeBarsGenerate.py",
                fidelity: "Translated from the concrete JIT Europe recipe; tuning remains project-owned in Ahess.",
            },
            Self::CtpianoDkSquare => VoiceDetails {
                description: "A forty-millisecond square strike built from fifteen odd sine harmonics.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe DKsquare/JITEuropeDKsquareGenerate.py",
                fidelity: "Preserves the source duration and harmonic recipe.",
            },
            Self::CtpianoEmphaenharm => VoiceDetails {
                description: "Three thirty-partial enharmonic triangle banks plus a strong sine fundamental.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe Emphaenharm/jitEuropeEmphaenharmGenerate.py",
                fidelity: "Preserves the source layer ratios and deterministic level profile.",
            },
            Self::CtpianoHiSaw => VoiceDetails {
                description: "A two-second additive saw built from sixty sine harmonics.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe Hisaw/JITEuropeHisawGenerate.py",
                fidelity: "Band-limited at the active output rate instead of aliasing above Nyquist.",
            },
            Self::CtpianoLoSaw => VoiceDetails {
                description: "A warmer two-second additive saw built from fifteen sine harmonics.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe Losaw/JITEuropeLosawGenerate.py",
                fidelity: "Band-limited at the active output rate instead of aliasing above Nyquist.",
            },
            Self::CtpianoLoSquare => VoiceDetails {
                description: "A two-second square voice built from fifteen odd sine harmonics.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe Losquare/JITEuropeLosquareGenerate.py",
                fidelity: "Band-limited at the active output rate instead of aliasing above Nyquist.",
            },
            Self::CtpianoTriangleDrop => VoiceDetails {
                description: "Three enharmonic triangle layers and a subharmonic sine layer.",
                source: "Chadtech/Chadtech-v4.00--Ctpiano — JIT Europe Triangledrop/jitEuropeTriangleDropGenerate.py",
                fidelity: "Preserves the source layer ratios and two-second fixed duration.",
            },
            Self::RadlerDullSaw => VoiceDetails {
                description: "Ten sine harmonics weighted by normalized binomial coefficients and harmonic number.",
                source: "Chadtech/Radler-ui — engine-src/Part/DullSaw.hs and audio-src/Mono.hs",
                fidelity: "Preserves Radler's tiltedSin degree-ten spectrum with the Ahess beat envelope.",
            },
            Self::RadlerHarmonics => VoiceDetails {
                description: "The concrete three-component example encoded by Radler's configurable harmonic voice.",
                source: "Chadtech/Radler-ui — engine-src/Part/Harmonics.hs",
                fidelity: "Uses Radler's documented example ratios 1, 2, and 3 with levels 1, 0.5, and 0.2.",
            },
            Self::LegacyNoitechEnharmonic => VoiceDetails {
                description: "An early Noitech stretched odd-harmonic triangle with harmonic-dependent decay.",
                source: "Chadtech/Chadtech-v1.20--Noitech — Noitech.py",
                fidelity: "A fixed voice distilled from the source's parameterized enharmonic triangle family.",
            },
            Self::SurgeXtPiano => VoiceDetails {
                description: "The installed Surge XT Grand Piano patch tuned through MTS-ESP.",
                source: "Surge XT factory content — Grand Piano",
                fidelity: "Hosted in-process with exact-frequency MTS-ESP tuning.",
            },
            Self::SurgeXtDistortedElectricGuitar => VoiceDetails {
                description: "The installed Surge XT distorted electric guitar patch tuned through MTS-ESP.",
                source: "Surge XT factory content — distorted electric guitar",
                fidelity: "Hosted in-process with exact-frequency MTS-ESP tuning and project-owned reverb.",
            },
            Self::SurgeXtClarinet => VoiceDetails {
                description: "The installed John Valentine clarinet patch tuned through MTS-ESP.",
                source: "Surge XT factory content — John Valentine clarinet",
                fidelity: "Hosted in-process with exact-frequency MTS-ESP tuning and project-owned reverb.",
            },
        }
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

    #[test]
    fn bell_h_v2_has_a_separate_persisted_identity() {
        let stored: StoredVoiceType = toml::from_str("voice_type = \"noitech-bell-h-v2\"").unwrap();
        assert_eq!(stored.voice_type, VoiceType::NoitechBellHV2);
        assert_eq!(stored.voice_type.config_value(), "noitech-bell-h-v2");
    }

    #[test]
    fn every_voice_type_has_complete_details() {
        for voice_type in VoiceType::ALL {
            let details = voice_type.details();
            assert!(!details.description.trim().is_empty());
            assert!(!details.source.trim().is_empty());
            assert!(!details.fidelity.trim().is_empty());
        }
    }
}
