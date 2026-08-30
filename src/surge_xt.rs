use std::{
    error::Error,
    ffi::c_void,
    fmt, fs,
    mem::size_of,
    path::Path,
    ptr::{self, NonNull},
};

use objc2_audio_toolbox::{
    kAudioUnitProperty_ClassInfo, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global,
    kAudioUnitScope_Output, AudioComponent, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentInstance, AudioComponentInstanceDispose, AudioComponentInstanceNew,
    AudioUnitGetProperty, AudioUnitInitialize, AudioUnitRender, AudioUnitRenderActionFlags,
    AudioUnitSetProperty, AudioUnitUninitialize, MusicDeviceMIDIEvent,
};
use objc2_core_audio_types::{
    kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM, AudioBuffer, AudioBufferList,
    AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
};
use objc2_core_foundation::{
    CFData, CFDictionary, CFMutableDictionary, CFRetained, CFString, CFType,
};

const SURGE_COMPONENT_TYPE: u32 = u32::from_be_bytes(*b"aumu");
const SURGE_COMPONENT_SUBTYPE: u32 = u32::from_be_bytes(*b"SgXT");
const SURGE_COMPONENT_MANUFACTURER: u32 = u32::from_be_bytes(*b"VmbA");
const FXP_HEADER_BYTES: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurgeXtPatch {
    GrandPiano,
    DistortedElectricGuitar,
}

impl SurgeXtPatch {
    const fn name(self) -> &'static str {
        match self {
            Self::GrandPiano => "Grand Piano",
            Self::DistortedElectricGuitar => "Distorted Electric Guitar",
        }
    }

    const fn path(self) -> &'static str {
        match self {
            Self::GrandPiano => "/Library/Application Support/Surge XT/patches_3rdparty/John Valentine/Keys/Grand Piano.fxp",
            Self::DistortedElectricGuitar => "/Library/Application Support/Surge XT/patches_3rdparty/John Valentine/Guitars/Distorted Electric Guitar.fxp",
        }
    }

    fn prepare_chunk(self, chunk: &mut [u8]) -> Result<(), SurgeXtError> {
        match self {
            Self::GrandPiano => Ok(()),
            Self::DistortedElectricGuitar => remove_distorted_guitar_reverb(chunk),
        }
    }
}

pub(crate) struct SurgeXt {
    instance: AudioComponentInstance,
    sample_time: f64,
}

// Audio Unit instances are created and initialized before playback, then moved
// into the one render thread that owns them. They are never accessed from two
// threads concurrently.
unsafe impl Send for SurgeXt {}

impl SurgeXt {
    pub(crate) fn is_available() -> bool {
        !find_component().is_null()
    }

    pub(crate) fn new(sample_rate: f64) -> Result<Self, SurgeXtError> {
        let component = find_component();
        if component.is_null() {
            return Err(SurgeXtError::new(
                "Surge XT Audio Unit aumu/SgXT/VmbA is not installed",
            ));
        }

        let mut instance = ptr::null_mut();
        check_status(
            "create Surge XT Audio Unit",
            // SAFETY: `component` came from AudioComponentFindNext and the out
            // pointer remains valid for the duration of the call.
            unsafe { AudioComponentInstanceNew(component, NonNull::from(&mut instance)) },
        )?;
        if instance.is_null() {
            return Err(SurgeXtError::new(
                "creating Surge XT succeeded without returning an Audio Unit instance",
            ));
        }

        let format = AudioStreamBasicDescription {
            mSampleRate: sample_rate,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagsNativeFloatPacked,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let configured = check_status(
            "configure Surge XT output format",
            // SAFETY: `instance` is live and `format` is a valid stereo,
            // interleaved native-float stream description.
            unsafe {
                AudioUnitSetProperty(
                    instance,
                    kAudioUnitProperty_StreamFormat,
                    kAudioUnitScope_Output,
                    0,
                    (&format as *const AudioStreamBasicDescription).cast::<c_void>(),
                    size_of::<AudioStreamBasicDescription>() as u32,
                )
            },
        );
        if let Err(error) = configured {
            // SAFETY: `instance` was created successfully and has not been disposed.
            unsafe { AudioComponentInstanceDispose(instance) };
            return Err(error);
        }

        if let Err(error) = check_status(
            "initialize Surge XT",
            // SAFETY: `instance` is live and its stream format is configured.
            unsafe { AudioUnitInitialize(instance) },
        ) {
            // SAFETY: `instance` was created successfully and has not been disposed.
            unsafe { AudioComponentInstanceDispose(instance) };
            return Err(error);
        }

        Ok(Self {
            instance,
            sample_time: 0.0,
        })
    }

    pub(crate) fn new_with_patch(
        sample_rate: f64,
        patch: SurgeXtPatch,
    ) -> Result<Self, SurgeXtError> {
        let mut surge = Self::new(sample_rate)?;
        surge.load_patch(patch)?;
        Ok(surge)
    }

    fn load_patch(&mut self, patch: SurgeXtPatch) -> Result<(), SurgeXtError> {
        let path = Path::new(patch.path());
        let fxp = fs::read(path).map_err(|error| {
            SurgeXtError::new(format!(
                "Surge XT {} patch is unavailable at {}: {error}; install the Surge XT factory resources",
                patch.name(),
                path.display()
            ))
        })?;
        let mut patch_chunk = fxp_chunk(&fxp, patch.name())?.to_vec();
        patch.prepare_chunk(&mut patch_chunk)?;

        let mut class_info: *const CFDictionary = ptr::null();
        let mut byte_count = size_of::<*const CFDictionary>() as u32;
        check_status(
            "read Surge XT Audio Unit state",
            // SAFETY: the Audio Unit is live and the output pointer and size
            // remain valid for the complete property call.
            unsafe {
                AudioUnitGetProperty(
                    self.instance,
                    kAudioUnitProperty_ClassInfo,
                    kAudioUnitScope_Global,
                    0,
                    NonNull::from(&mut class_info).cast::<c_void>(),
                    NonNull::from(&mut byte_count),
                )
            },
        )?;
        let class_info = NonNull::new(class_info.cast_mut())
            .ok_or_else(|| SurgeXtError::new("Surge XT returned no Audio Unit state"))?;
        // SAFETY: `kAudioUnitProperty_ClassInfo` returns an owned CF property
        // list. It is a dictionary for an Audio Unit v2 instance.
        let class_info = unsafe { CFRetained::from_raw(class_info) };
        // SAFETY: `class_info` is the Audio Unit's valid state dictionary.
        let mutable = unsafe { CFMutableDictionary::new_copy(None, 0, Some(&class_info)) }
            .ok_or_else(|| SurgeXtError::new("copy Surge XT Audio Unit state"))?;
        let mutable = unsafe { mutable.cast_unchecked::<CFString, CFType>() };
        let state_key = CFString::from_str("jucePluginState");
        let patch_data = CFData::from_bytes(&patch_chunk);
        mutable.set(&state_key, patch_data.as_ref());

        let state: *const CFMutableDictionary = mutable.as_opaque();
        check_status(
            &format!("load Surge XT {} patch", patch.name()),
            // SAFETY: the Audio Unit is live, and `state` points to a retained
            // property-list dictionary for the duration of the call.
            unsafe {
                AudioUnitSetProperty(
                    self.instance,
                    kAudioUnitProperty_ClassInfo,
                    kAudioUnitScope_Global,
                    0,
                    (&state as *const *const CFMutableDictionary).cast::<c_void>(),
                    size_of::<*const CFMutableDictionary>() as u32,
                )
            },
        )
    }

    pub(crate) fn note_on(
        &mut self,
        channel: u8,
        note: u8,
        velocity: u8,
        sample_offset: u32,
    ) -> Result<(), SurgeXtError> {
        self.midi_event(0x90 | (channel & 0x0f), note, velocity, sample_offset)
    }

    pub(crate) fn note_off(
        &mut self,
        channel: u8,
        note: u8,
        sample_offset: u32,
    ) -> Result<(), SurgeXtError> {
        self.midi_event(0x80 | (channel & 0x0f), note, 0, sample_offset)
    }

    fn midi_event(
        &mut self,
        status: u8,
        data1: u8,
        data2: u8,
        sample_offset: u32,
    ) -> Result<(), SurgeXtError> {
        check_status(
            "send MIDI event to Surge XT",
            // SAFETY: `instance` is a live initialized music-device Audio Unit.
            unsafe {
                MusicDeviceMIDIEvent(
                    self.instance,
                    u32::from(status),
                    u32::from(data1),
                    u32::from(data2),
                    sample_offset,
                )
            },
        )
    }

    pub(crate) fn render(&mut self, interleaved_stereo: &mut [f32]) -> Result<(), SurgeXtError> {
        if !interleaved_stereo.len().is_multiple_of(2) {
            return Err(SurgeXtError::new(
                "Surge XT output buffer must contain complete stereo frames",
            ));
        }
        let frame_count = interleaved_stereo.len() / 2;
        let frame_count = u32::try_from(frame_count)
            .map_err(|_| SurgeXtError::new("Surge XT render block is too large"))?;
        let byte_count = u32::try_from(std::mem::size_of_val(interleaved_stereo))
            .map_err(|_| SurgeXtError::new("Surge XT render buffer is too large"))?;
        let mut buffer_list = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: 2,
                mDataByteSize: byte_count,
                mData: interleaved_stereo.as_mut_ptr().cast::<c_void>(),
            }],
        };
        let mut timestamp: AudioTimeStamp = unsafe { std::mem::zeroed() };
        timestamp.mSampleTime = self.sample_time;
        timestamp.mFlags = AudioTimeStampFlags::SampleTimeValid;
        let mut flags = AudioUnitRenderActionFlags(0);

        check_status(
            "render Surge XT",
            // SAFETY: the Audio Unit is initialized, and the timestamp and
            // interleaved output buffer remain valid for the complete call.
            unsafe {
                AudioUnitRender(
                    self.instance,
                    &mut flags,
                    NonNull::from(&mut timestamp),
                    0,
                    frame_count,
                    NonNull::from(&mut buffer_list),
                )
            },
        )?;
        self.sample_time += f64::from(frame_count);
        Ok(())
    }
}

fn fxp_chunk<'a>(fxp: &'a [u8], patch_name: &str) -> Result<&'a [u8], SurgeXtError> {
    if fxp.len() < FXP_HEADER_BYTES
        || &fxp[0..4] != b"CcnK"
        || &fxp[8..12] != b"FPCh"
        || &fxp[16..20] != b"cjs3"
    {
        return Err(SurgeXtError::new(format!(
            "installed Surge XT {patch_name} patch is not a valid Surge chunk preset"
        )));
    }

    let chunk_bytes = u32::from_be_bytes(
        fxp[56..60]
            .try_into()
            .expect("the checked FXP header contains the chunk size"),
    ) as usize;
    let end = FXP_HEADER_BYTES
        .checked_add(chunk_bytes)
        .filter(|end| *end <= fxp.len())
        .ok_or_else(|| {
            SurgeXtError::new(format!(
                "installed Surge XT {patch_name} patch is truncated"
            ))
        })?;
    Ok(&fxp[FXP_HEADER_BYTES..end])
}

fn remove_distorted_guitar_reverb(chunk: &mut [u8]) -> Result<(), SurgeXtError> {
    // In this preset, XML `fx8` is Surge's zero-based slot 7 (Global FX 2),
    // and effect type 11 is Reverb 2. Preserve the serialized chunk length so
    // the enclosing JUCE state remains valid while turning only that effect off.
    const REVERB: &[u8] = br#"<fx8_type type="0" value="11" />"#;
    const OFF: &[u8] = br#"<fx8_type type="0" value="00" />"#;

    let mut matches = chunk
        .windows(REVERB.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == REVERB).then_some(offset));
    let Some(offset) = matches.next() else {
        return Err(SurgeXtError::new(
            "Surge XT Distorted Electric Guitar patch does not contain its expected Reverb 2 slot",
        ));
    };
    if matches.next().is_some() {
        return Err(SurgeXtError::new(
            "Surge XT Distorted Electric Guitar patch contains more than one expected Reverb 2 slot",
        ));
    }
    chunk[offset..offset + OFF.len()].copy_from_slice(OFF);
    Ok(())
}

impl Drop for SurgeXt {
    fn drop(&mut self) {
        // SAFETY: this instance is exclusively owned and disposed exactly once.
        unsafe {
            AudioUnitUninitialize(self.instance);
            AudioComponentInstanceDispose(self.instance);
        }
    }
}

fn find_component() -> AudioComponent {
    let mut description = AudioComponentDescription {
        componentType: SURGE_COMPONENT_TYPE,
        componentSubType: SURGE_COMPONENT_SUBTYPE,
        componentManufacturer: SURGE_COMPONENT_MANUFACTURER,
        componentFlags: 0,
        componentFlagsMask: 0,
    };
    // SAFETY: the description remains valid for the complete lookup call.
    unsafe { AudioComponentFindNext(ptr::null_mut(), NonNull::from(&mut description)) }
}

fn check_status(operation: &str, status: i32) -> Result<(), SurgeXtError> {
    if status == 0 {
        Ok(())
    } else {
        Err(SurgeXtError::new(format!(
            "failed to {operation}: Audio Unit status {status}"
        )))
    }
}

#[derive(Debug)]
pub(crate) struct SurgeXtError {
    message: String,
}

impl SurgeXtError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SurgeXtError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SurgeXtError {}

#[cfg(test)]
mod tests {
    use super::{remove_distorted_guitar_reverb, SurgeXt, SurgeXtPatch};

    #[test]
    fn distorted_guitar_patch_removes_only_its_reverb_slot() {
        let mut chunk =
            br#"before <fx7_type type="0" value="12" /><fx8_type type="0" value="11" /> after"#
                .to_vec();
        let original_length = chunk.len();

        remove_distorted_guitar_reverb(&mut chunk).unwrap();

        assert_eq!(chunk.len(), original_length);
        assert_eq!(
            chunk,
            br#"before <fx7_type type="0" value="12" /><fx8_type type="0" value="00" /> after"#
        );
    }

    #[test]
    fn distorted_guitar_patch_requires_the_expected_reverb_slot() {
        let mut chunk = b"no reverb here".to_vec();
        let error = remove_distorted_guitar_reverb(&mut chunk).unwrap_err();

        assert!(error.to_string().contains("expected Reverb 2 slot"));
    }

    #[test]
    #[ignore = "requires the installed Surge XT Audio Unit"]
    fn installed_audio_unit_renders_a_note() {
        assert!(SurgeXt::is_available());
        let mut surge = SurgeXt::new(48_000.0).unwrap();
        surge.note_on(0, 69, 100, 0).unwrap();

        let mut rendered = vec![0.0; 512 * 2];
        let mut energy = 0.0_f32;
        for _ in 0..16 {
            surge.render(&mut rendered).unwrap();
            energy += rendered.iter().map(|sample| sample.abs()).sum::<f32>();
        }
        surge.note_off(0, 69, 0).unwrap();

        assert!(energy > 0.01, "Surge XT rendered silence");
    }

    #[test]
    #[ignore = "requires installed Surge XT resources"]
    fn installed_audio_units_load_grand_piano_for_two_voices() {
        let mut surges = [
            SurgeXt::new_with_patch(48_000.0, SurgeXtPatch::GrandPiano).unwrap(),
            SurgeXt::new_with_patch(48_000.0, SurgeXtPatch::GrandPiano).unwrap(),
        ];
        let mut energy = [0.0_f32; 2];

        for (index, surge) in surges.iter_mut().enumerate() {
            surge.note_on(0, 69, 100, 0).unwrap();
            let mut rendered = vec![0.0; 512 * 2];
            for _ in 0..16 {
                surge.render(&mut rendered).unwrap();
                energy[index] += rendered.iter().map(|sample| sample.abs()).sum::<f32>();
            }
        }

        assert!(
            energy.into_iter().all(|voice_energy| voice_energy > 0.01),
            "a Surge XT Grand Piano voice rendered silence"
        );
    }

    #[test]
    #[ignore = "requires installed Surge XT resources"]
    fn installed_audio_unit_loads_distorted_electric_guitar() {
        let mut surge =
            SurgeXt::new_with_patch(48_000.0, SurgeXtPatch::DistortedElectricGuitar).unwrap();
        surge.note_on(0, 45, 100, 0).unwrap();

        let mut rendered = vec![0.0; 512 * 2];
        let mut energy = 0.0_f32;
        for _ in 0..16 {
            surge.render(&mut rendered).unwrap();
            energy += rendered.iter().map(|sample| sample.abs()).sum::<f32>();
        }
        surge.note_off(0, 45, 0).unwrap();

        assert!(
            energy > 0.01,
            "Surge XT Distorted Electric Guitar rendered silence"
        );
    }
}
