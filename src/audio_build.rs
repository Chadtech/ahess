use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::{
    acoustics::StereoFrame,
    playback::{OfflineRenderer, PlaybackLoop},
    project::{self, Project},
};

pub(crate) const BUILD_DIRECTORY: &str = "build";
const BUILD_MANIFEST: &str = ".ahess-audio-files";
const WAV_HEADER_SIZE: u64 = 58;
const WAV_DATA_SIZE_OFFSET: u64 = 54;
const WAV_FACT_SAMPLE_LENGTH_OFFSET: u64 = 46;
const WAV_RIFF_SIZE_OFFSET: u64 = 4;
const WAV_RIFF_OVERHEAD: u64 = 50;
const STEREO_FLOAT_FRAME_BYTES: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BuildSampleRate {
    Hz44100,
    Hz48000,
    Hz96000,
}

impl BuildSampleRate {
    pub(crate) const ALL: [Self; 3] = [Self::Hz44100, Self::Hz48000, Self::Hz96000];
    pub(crate) const DEFAULT: Self = Self::Hz48000;

    pub(crate) const fn hz(self) -> u32 {
        match self {
            Self::Hz44100 => 44_100,
            Self::Hz48000 => 48_000,
            Self::Hz96000 => 96_000,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Hz44100 => "44.1 kHz",
            Self::Hz48000 => "48 kHz",
            Self::Hz96000 => "96 kHz",
        }
    }

    pub(crate) fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub(crate) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|sample_rate| *sample_rate == self)
            .expect("every build sample rate is in the dropdown")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlannedAudioFile {
    pub(crate) label: String,
    pub(crate) file_name: String,
}

pub(crate) fn planned_audio_files(project: &Project) -> Vec<PlannedAudioFile> {
    let project_stem =
        project::project_directory_name(&project.name).unwrap_or_else(|| "project".to_string());
    let mut files = vec![PlannedAudioFile {
        label: "whole piece".to_string(),
        file_name: format!("{project_stem}-mix.wav"),
    }];
    files.extend(project.voices().iter().enumerate().map(|(index, voice)| {
        let voice_stem = project::project_directory_name(voice.name.as_str())
            .unwrap_or_else(|| format!("voice-{}", voice.id().value()));
        PlannedAudioFile {
            label: format!("voice: {}", voice.name.as_str()),
            file_name: format!("{project_stem}-voice-{:02}-{voice_stem}.wav", index + 1),
        }
    }));
    files
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AudioBuildResult {
    pub(crate) directory: PathBuf,
    pub(crate) file_count: usize,
    pub(crate) frame_count: u64,
    pub(crate) sample_rate: u32,
}

impl AudioBuildResult {
    pub(crate) fn duration_seconds(&self) -> f64 {
        self.frame_count as f64 / f64::from(self.sample_rate)
    }
}

pub(crate) fn build_project_audio(
    project_directory: impl AsRef<Path>,
    project: &Project,
    playback_loop: PlaybackLoop,
    sample_rate: BuildSampleRate,
) -> Result<AudioBuildResult, AudioBuildError> {
    let mut renderer = OfflineRenderer::new(playback_loop, sample_rate.hz());
    if renderer.voice_count() != project.voices().len() {
        return Err(AudioBuildError::new(
            "the prepared arrangement does not match the project voices",
        ));
    }
    ensure_wav_size_can_hold(renderer.score_frame_count())?;

    let directory = project_directory.as_ref().join(BUILD_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|source| {
        AudioBuildError::io(
            format!("failed to create audio build directory {:?}", directory),
            source,
        )
    })?;

    let plans = planned_audio_files(project);
    let pending_paths = plans
        .iter()
        .map(|plan| directory.join(format!(".{}.pending", plan.file_name)))
        .collect::<Vec<_>>();
    let target_paths = plans
        .iter()
        .map(|plan| directory.join(&plan.file_name))
        .collect::<Vec<_>>();
    let mut writers = Vec::with_capacity(pending_paths.len());
    for pending_path in &pending_paths {
        if pending_path.exists() {
            fs::remove_file(pending_path).map_err(|source| {
                AudioBuildError::io(
                    format!("failed to replace pending audio file {:?}", pending_path),
                    source,
                )
            })?;
        }
        match FloatStereoWavWriter::create(pending_path, sample_rate.hz()) {
            Ok(writer) => writers.push(writer),
            Err(error) => {
                drop(writers);
                remove_files(&pending_paths);
                return Err(error);
            }
        }
    }

    let render_result = render_to_writers(&mut renderer, &mut writers);
    let finish_result = if render_result.is_ok() {
        writers
            .iter_mut()
            .try_for_each(FloatStereoWavWriter::finish)
    } else {
        Ok(())
    };
    drop(writers);
    let frame_count = match render_result {
        Ok(frame_count) => match finish_result {
            Ok(()) => frame_count,
            Err(error) => {
                remove_files(&pending_paths);
                return Err(error);
            }
        },
        Err(error) => {
            remove_files(&pending_paths);
            return Err(error);
        }
    };
    if pending_paths.iter().any(|path| !path.exists()) {
        remove_files(&pending_paths);
        return Err(AudioBuildError::new(
            "an audio build file disappeared before it could be published",
        ));
    }

    for (pending_path, target_path) in pending_paths.iter().zip(&target_paths) {
        if target_path.exists() {
            fs::remove_file(target_path).map_err(|source| {
                AudioBuildError::io(
                    format!("failed to replace audio file {:?}", target_path),
                    source,
                )
            })?;
        }
        fs::rename(pending_path, target_path).map_err(|source| {
            AudioBuildError::io(
                format!("failed to publish audio file {:?}", target_path),
                source,
            )
        })?;
    }
    update_build_manifest(&directory, &plans)?;

    Ok(AudioBuildResult {
        directory,
        file_count: plans.len(),
        frame_count,
        sample_rate: sample_rate.hz(),
    })
}

fn render_to_writers(
    renderer: &mut OfflineRenderer,
    writers: &mut [FloatStereoWavWriter],
) -> Result<u64, AudioBuildError> {
    let mut frame_count = 0_u64;
    while let Some((mix, voice_frames)) = renderer.next_frame() {
        ensure_wav_size_can_hold(frame_count + 1)?;
        writers[0].write_frame(mix)?;
        for (writer, frame) in writers[1..].iter_mut().zip(voice_frames) {
            writer.write_frame(*frame)?;
        }
        frame_count += 1;
    }
    Ok(frame_count)
}

fn ensure_wav_size_can_hold(frame_count: u64) -> Result<(), AudioBuildError> {
    let data_size = frame_count
        .checked_mul(STEREO_FLOAT_FRAME_BYTES)
        .ok_or_else(|| AudioBuildError::new("the audio build is too large for a WAV file"))?;
    if data_size > u64::from(u32::MAX) - WAV_RIFF_OVERHEAD {
        return Err(AudioBuildError::new(
            "the audio build is too large for a standard WAV file",
        ));
    }
    Ok(())
}

fn update_build_manifest(
    directory: &Path,
    plans: &[PlannedAudioFile],
) -> Result<(), AudioBuildError> {
    let manifest_path = directory.join(BUILD_MANIFEST);
    let current_files = plans
        .iter()
        .map(|plan| plan.file_name.as_str())
        .collect::<BTreeSet<_>>();
    if let Ok(previous_manifest) = fs::read_to_string(&manifest_path) {
        for previous_file in previous_manifest.lines() {
            if current_files.contains(previous_file) || !safe_generated_file_name(previous_file) {
                continue;
            }
            let stale_path = directory.join(previous_file);
            if stale_path.exists() {
                fs::remove_file(&stale_path).map_err(|source| {
                    AudioBuildError::io(
                        format!("failed to remove stale audio file {:?}", stale_path),
                        source,
                    )
                })?;
            }
        }
    }

    let pending_manifest = directory.join(format!("{BUILD_MANIFEST}.pending"));
    let contents = plans
        .iter()
        .map(|plan| plan.file_name.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&pending_manifest, contents).map_err(|source| {
        AudioBuildError::io(
            format!(
                "failed to write audio build manifest {:?}",
                pending_manifest
            ),
            source,
        )
    })?;
    if manifest_path.exists() {
        fs::remove_file(&manifest_path).map_err(|source| {
            AudioBuildError::io(
                format!("failed to replace audio build manifest {:?}", manifest_path),
                source,
            )
        })?;
    }
    fs::rename(&pending_manifest, &manifest_path).map_err(|source| {
        AudioBuildError::io(
            format!("failed to publish audio build manifest {:?}", manifest_path),
            source,
        )
    })
}

fn safe_generated_file_name(file_name: &str) -> bool {
    let path = Path::new(file_name);
    path.components().count() == 1
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
}

fn remove_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

struct FloatStereoWavWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    frame_count: u64,
}

impl FloatStereoWavWriter {
    fn create(path: &Path, sample_rate: u32) -> Result<Self, AudioBuildError> {
        let file = File::create(path).map_err(|source| {
            AudioBuildError::io(format!("failed to create audio file {:?}", path), source)
        })?;
        let mut writer = BufWriter::new(file);
        write_wav_header(&mut writer, sample_rate).map_err(|source| {
            AudioBuildError::io(format!("failed to write audio file {:?}", path), source)
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            writer,
            frame_count: 0,
        })
    }

    fn write_frame(&mut self, frame: StereoFrame) -> Result<(), AudioBuildError> {
        self.writer
            .write_all(&frame.left.to_le_bytes())
            .and_then(|_| self.writer.write_all(&frame.right.to_le_bytes()))
            .map_err(|source| {
                AudioBuildError::io(
                    format!("failed to write audio file {:?}", self.path),
                    source,
                )
            })?;
        self.frame_count += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), AudioBuildError> {
        ensure_wav_size_can_hold(self.frame_count)?;
        let data_size = (self.frame_count * STEREO_FLOAT_FRAME_BYTES) as u32;
        let riff_size = data_size + WAV_RIFF_OVERHEAD as u32;
        self.writer
            .seek(SeekFrom::Start(WAV_RIFF_SIZE_OFFSET))
            .and_then(|_| self.writer.write_all(&riff_size.to_le_bytes()))
            .and_then(|_| {
                self.writer
                    .seek(SeekFrom::Start(WAV_FACT_SAMPLE_LENGTH_OFFSET))
            })
            .and_then(|_| {
                self.writer
                    .write_all(&(self.frame_count as u32).to_le_bytes())
            })
            .and_then(|_| self.writer.seek(SeekFrom::Start(WAV_DATA_SIZE_OFFSET)))
            .and_then(|_| self.writer.write_all(&data_size.to_le_bytes()))
            .and_then(|_| self.writer.flush())
            .map_err(|source| {
                AudioBuildError::io(
                    format!("failed to finish audio file {:?}", self.path),
                    source,
                )
            })?;
        self.writer.get_ref().sync_all().map_err(|source| {
            AudioBuildError::io(format!("failed to sync audio file {:?}", self.path), source)
        })
    }
}

fn write_wav_header(writer: &mut impl Write, sample_rate: u32) -> io::Result<()> {
    let byte_rate = sample_rate
        .checked_mul(STEREO_FLOAT_FRAME_BYTES as u32)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sample rate is too large"))?;
    writer.write_all(b"RIFF")?;
    writer.write_all(&0_u32.to_le_bytes())?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&18_u32.to_le_bytes())?;
    writer.write_all(&3_u16.to_le_bytes())?;
    writer.write_all(&2_u16.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&(STEREO_FLOAT_FRAME_BYTES as u16).to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(&0_u16.to_le_bytes())?;
    writer.write_all(b"fact")?;
    writer.write_all(&4_u32.to_le_bytes())?;
    writer.write_all(&0_u32.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&0_u32.to_le_bytes())?;
    debug_assert_eq!(WAV_HEADER_SIZE, 58);
    Ok(())
}

#[derive(Debug)]
pub(crate) struct AudioBuildError {
    message: String,
}

impl AudioBuildError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(message: impl Into<String>, source: io::Error) -> Self {
        Self::new(format!("{}: {source}", message.into()))
    }
}

impl fmt::Display for AudioBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AudioBuildError {}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        build_project_audio, planned_audio_files, BuildSampleRate, BUILD_DIRECTORY, WAV_HEADER_SIZE,
    };
    use crate::{
        part::{Part, PartScore},
        playback::{BeatRange, PlaybackLoop},
        project::{Project, Voice, VoiceType},
        seed::Seed,
    };

    #[test]
    fn builds_a_float_stereo_mix_and_one_stem_per_voice() {
        let root = temp_root("audio-build");
        let project_directory = root.join("project");
        fs::create_dir(&project_directory).unwrap();
        let part = Part::new("intro", 2);
        let project = Project::new("Arc Light", 8, 0, Seed::new(1))
            .with_voices(vec![
                Voice::new(1, "Lead Voice", VoiceType::Saw),
                Voice::new(2, "Bass", VoiceType::Sin),
            ])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![
            vec!["C4".to_string(), "C3".to_string()],
            vec!["D4".to_string(), "G2".to_string()],
        ]);
        let range = BeatRange::new(1, 2, 2).unwrap();
        let playback_loop =
            PlaybackLoop::from_project_arrangement(&project, &[(part, score)], range).unwrap();

        let result = build_project_audio(
            &project_directory,
            &project,
            playback_loop,
            BuildSampleRate::Hz48000,
        )
        .unwrap();

        assert_eq!(result.file_count, 3);
        assert_eq!(result.frame_count, 16);
        assert_eq!(result.sample_rate, 48_000);
        assert_eq!(result.duration_seconds(), 16.0 / 48_000.0);
        let plans = planned_audio_files(&project);
        assert_eq!(plans[0].file_name, "arc-light-mix.wav");
        assert_eq!(plans[1].file_name, "arc-light-voice-01-lead-voice.wav");
        let files = plans
            .iter()
            .map(|plan| read_float_wav(&result.directory.join(&plan.file_name)))
            .collect::<Vec<_>>();
        for file in &files {
            assert_eq!(file.sample_rate, 48_000);
            assert_eq!(file.samples.len(), 32);
        }
        for sample_index in 0..files[0].samples.len() {
            let stem_sum = files[1].samples[sample_index] + files[2].samples[sample_index];
            assert_eq!(files[0].samples[sample_index], stem_sum.clamp(-1.0, 1.0));
        }
        assert!(result.directory.ends_with(BUILD_DIRECTORY));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_later_build_removes_stems_that_are_no_longer_generated() {
        let root = temp_root("audio-build-stale");
        let project_directory = root.join("project");
        fs::create_dir(&project_directory).unwrap();
        let part = Part::new("intro", 1);
        let two_voice_project = Project::new("test", 8, 0, Seed::new(1))
            .with_voices(vec![
                Voice::new(1, "lead", VoiceType::Saw),
                Voice::new(2, "bass", VoiceType::Sin),
            ])
            .with_parts(vec![part.clone()]);
        let two_voice_score = PartScore::from_rows(vec![vec!["C4".to_string(), "C3".to_string()]]);
        let range = BeatRange::new(1, 1, 1).unwrap();
        let two_voice_loop = PlaybackLoop::from_project_arrangement(
            &two_voice_project,
            &[(part.clone(), two_voice_score)],
            range,
        )
        .unwrap();
        build_project_audio(
            &project_directory,
            &two_voice_project,
            two_voice_loop,
            BuildSampleRate::Hz44100,
        )
        .unwrap();
        let stale_file = project_directory
            .join(BUILD_DIRECTORY)
            .join("test-voice-02-bass.wav");
        assert!(stale_file.exists());
        let unrelated_file = project_directory.join(BUILD_DIRECTORY).join("notes.wav");
        fs::write(&unrelated_file, b"not generated by ahess").unwrap();

        let one_voice_project = Project::new("test", 8, 0, Seed::new(1))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let one_voice_score = PartScore::from_rows(vec![vec!["C4".to_string()]]);
        let one_voice_loop = PlaybackLoop::from_project_arrangement(
            &one_voice_project,
            &[(part, one_voice_score)],
            range,
        )
        .unwrap();
        build_project_audio(
            &project_directory,
            &one_voice_project,
            one_voice_loop,
            BuildSampleRate::Hz44100,
        )
        .unwrap();

        assert!(!stale_file.exists());
        assert!(unrelated_file.exists());
        fs::remove_dir_all(root).unwrap();
    }

    struct FloatWav {
        sample_rate: u32,
        samples: Vec<f32>,
    }

    fn read_float_wav(path: &Path) -> FloatWav {
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 3);
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 32);
        let sample_rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let data_size = u32::from_le_bytes(bytes[54..58].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), WAV_HEADER_SIZE as usize + data_size);
        let samples = bytes[WAV_HEADER_SIZE as usize..]
            .chunks_exact(4)
            .map(|sample| f32::from_le_bytes(sample.try_into().unwrap()))
            .collect();
        FloatWav {
            sample_rate,
            samples,
        }
    }

    fn temp_root(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ahess-{test_name}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
