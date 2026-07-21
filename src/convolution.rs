use std::{
    error::Error,
    fmt, fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Deserialize;

const IMPULSE_RESPONSE_DIRECTORY: &str = "assets/impulse-responses";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DURATION_SECONDS: u64 = 30;
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceConvolutionSpec {
    file: ImpulseResponseAssetPath,
    name: String,
}

impl VoiceConvolutionSpec {
    pub fn file(&self) -> &Path {
        Path::new(&self.file.0)
    }

    pub fn file_name(&self) -> &str {
        &self.name
    }

    pub(crate) fn file_config_value(&self) -> &str {
        &self.file.0
    }
}

impl<'de> Deserialize<'de> for VoiceConvolutionSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct StoredSpec {
            file: String,
            name: String,
        }

        let stored = StoredSpec::deserialize(deserializer)?;
        let file = ImpulseResponseAssetPath::new(stored.file).map_err(serde::de::Error::custom)?;
        validate_display_name(&stored.name).map_err(serde::de::Error::custom)?;
        Ok(Self {
            file,
            name: stored.name,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImpulseResponseAssetPath(String);

impl ImpulseResponseAssetPath {
    fn new(value: String) -> Result<Self, String> {
        let prefix = format!("{IMPULSE_RESPONSE_DIRECTORY}/");
        let Some(file_name) = value.strip_prefix(&prefix) else {
            return Err(format!(
                "impulse response files must be stored under {IMPULSE_RESPONSE_DIRECTORY}"
            ));
        };
        let Some(hash) = file_name.strip_suffix(".wav") else {
            return Err("impulse response assets must be WAV files".to_string());
        };
        if hash.len() != 16
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("impulse response asset names must use their content hash".to_string());
        }
        Ok(Self(value))
    }

    fn for_contents(contents: &[u8]) -> Self {
        let hash = contents.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
        });
        Self(format!("{IMPULSE_RESPONSE_DIRECTORY}/{hash:016x}.wav"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WavMetadata {
    sample_rate: u32,
    bits_per_sample: u16,
    frame_count: u64,
}

impl WavMetadata {
    pub const fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    pub const fn bits_per_sample(self) -> u16 {
        self.bits_per_sample
    }

    pub fn duration_seconds(self) -> f64 {
        self.frame_count as f64 / f64::from(self.sample_rate)
    }
}

#[derive(Debug)]
pub enum ImpulseResponseError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
}

impl ImpulseResponseError {
    fn invalid(path: &Path, message: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ImpulseResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Invalid { path, message } => {
                write!(
                    formatter,
                    "invalid impulse response at {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ImpulseResponseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid { .. } => None,
        }
    }
}

pub fn inspect_wav_file(path: impl AsRef<Path>) -> Result<WavMetadata, ImpulseResponseError> {
    let path = path.as_ref();
    let contents = read_wav_file(path)?;
    inspect_wav_bytes(path, &contents)
}

pub(crate) fn inspect_project_asset(
    project_directory: &Path,
    spec: &VoiceConvolutionSpec,
) -> Result<WavMetadata, ImpulseResponseError> {
    inspect_wav_file(project_directory.join(spec.file()))
}

pub(crate) fn import_wav_file(
    project_directory: &Path,
    source_path: &Path,
) -> Result<(VoiceConvolutionSpec, WavMetadata), ImpulseResponseError> {
    let contents = read_wav_file(source_path)?;
    let metadata = inspect_wav_bytes(source_path, &contents)?;
    let asset_path = ImpulseResponseAssetPath::for_contents(&contents);
    let destination_path = project_directory.join(Path::new(&asset_path.0));
    let parent = destination_path
        .parent()
        .expect("an impulse response asset always has a parent directory");
    fs::create_dir_all(parent).map_err(|source| ImpulseResponseError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination_path)
    {
        Ok(mut file) => file
            .write_all(&contents)
            .and_then(|_| file.sync_all())
            .map_err(|source| ImpulseResponseError::Io {
                path: destination_path,
                source,
            })?,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing =
                fs::read(&destination_path).map_err(|source| ImpulseResponseError::Io {
                    path: destination_path.clone(),
                    source,
                })?;
            if existing != contents {
                return Err(ImpulseResponseError::invalid(
                    &destination_path,
                    "the content-addressed asset already exists with different contents",
                ));
            }
        }
        Err(source) => {
            return Err(ImpulseResponseError::Io {
                path: destination_path,
                source,
            });
        }
    }

    let name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| validate_display_name(name).is_ok())
        .unwrap_or("impulse-response.wav")
        .to_string();
    Ok((
        VoiceConvolutionSpec {
            file: asset_path,
            name,
        },
        metadata,
    ))
}

fn read_wav_file(path: &Path) -> Result<Vec<u8>, ImpulseResponseError> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        return Err(ImpulseResponseError::invalid(
            path,
            "select a file with a .wav extension",
        ));
    }
    let file_metadata = fs::metadata(path).map_err(|source| ImpulseResponseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !file_metadata.is_file() {
        return Err(ImpulseResponseError::invalid(
            path,
            "the selected path is not a file",
        ));
    }
    if file_metadata.len() > MAX_FILE_BYTES {
        return Err(ImpulseResponseError::invalid(
            path,
            "the WAV file must be no larger than 64 MiB",
        ));
    }
    fs::read(path).map_err(|source| ImpulseResponseError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn inspect_wav_bytes(path: &Path, contents: &[u8]) -> Result<WavMetadata, ImpulseResponseError> {
    if contents.len() < 12 || &contents[0..4] != b"RIFF" || &contents[8..12] != b"WAVE" {
        return Err(ImpulseResponseError::invalid(
            path,
            "the file does not contain a RIFF/WAVE header",
        ));
    }

    let mut offset = 12_usize;
    let mut format = None;
    let mut data_bytes = 0_u64;
    while offset + 8 <= contents.len() {
        let chunk_id = &contents[offset..offset + 4];
        let chunk_size = u32::from_le_bytes(
            contents[offset + 4..offset + 8]
                .try_into()
                .expect("a four-byte slice always converts to a four-byte array"),
        ) as usize;
        let chunk_start = offset + 8;
        let chunk_end = chunk_start.checked_add(chunk_size).ok_or_else(|| {
            ImpulseResponseError::invalid(path, "a WAV chunk length overflows the file size")
        })?;
        if chunk_end > contents.len() {
            return Err(ImpulseResponseError::invalid(
                path,
                "a WAV chunk extends beyond the end of the file",
            ));
        }

        match chunk_id {
            b"fmt " => {
                if chunk_size < 16 {
                    return Err(ImpulseResponseError::invalid(
                        path,
                        "the WAV format chunk is incomplete",
                    ));
                }
                let chunk = &contents[chunk_start..chunk_end];
                format = Some(WavFormat {
                    audio_format: u16::from_le_bytes([chunk[0], chunk[1]]),
                    channels: u16::from_le_bytes([chunk[2], chunk[3]]),
                    sample_rate: u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                    block_align: u16::from_le_bytes([chunk[12], chunk[13]]),
                    bits_per_sample: u16::from_le_bytes([chunk[14], chunk[15]]),
                });
            }
            b"data" => data_bytes = data_bytes.saturating_add(chunk_size as u64),
            _ => {}
        }

        offset = chunk_end + (chunk_size % 2);
    }

    let format = format
        .ok_or_else(|| ImpulseResponseError::invalid(path, "the WAV file has no format chunk"))?;
    if !matches!(format.audio_format, 1 | 3) {
        return Err(ImpulseResponseError::invalid(
            path,
            "only PCM and IEEE-float WAV files are supported",
        ));
    }
    if format.channels != 1 {
        return Err(ImpulseResponseError::invalid(
            path,
            "the impulse response must contain exactly one channel",
        ));
    }
    if format.sample_rate == 0 {
        return Err(ImpulseResponseError::invalid(
            path,
            "the WAV sample rate must be greater than zero",
        ));
    }
    let supported_bits = match format.audio_format {
        1 => matches!(format.bits_per_sample, 8 | 16 | 24 | 32),
        3 => matches!(format.bits_per_sample, 32 | 64),
        _ => false,
    };
    if !supported_bits {
        return Err(ImpulseResponseError::invalid(
            path,
            "the WAV sample format is not supported",
        ));
    }
    let expected_block_align = format.channels * (format.bits_per_sample / 8);
    if format.block_align == 0 || format.block_align != expected_block_align {
        return Err(ImpulseResponseError::invalid(
            path,
            "the WAV block alignment does not match its sample format",
        ));
    }
    if data_bytes == 0 {
        return Err(ImpulseResponseError::invalid(
            path,
            "the WAV file contains no audio samples",
        ));
    }
    if !data_bytes.is_multiple_of(u64::from(format.block_align)) {
        return Err(ImpulseResponseError::invalid(
            path,
            "the WAV audio data ends in a partial sample frame",
        ));
    }

    let frame_count = data_bytes / u64::from(format.block_align);
    if frame_count > u64::from(format.sample_rate) * MAX_DURATION_SECONDS {
        return Err(ImpulseResponseError::invalid(
            path,
            "the impulse response must be no longer than 30 seconds",
        ));
    }

    Ok(WavMetadata {
        sample_rate: format.sample_rate,
        bits_per_sample: format.bits_per_sample,
        frame_count,
    })
}

fn validate_display_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        Err("the impulse response display name must contain visible text".to_string())
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct WavFormat {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::{import_wav_file, inspect_wav_file, ImpulseResponseAssetPath};

    #[test]
    fn valid_mono_pcm_wav_reports_its_metadata() {
        let root = temp_root("valid-wav");
        let path = root.join("room.wav");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, wav_bytes(1, 48_000, 16, &[0, 0, 1, 0])).unwrap();

        let metadata = inspect_wav_file(&path).unwrap();

        assert_eq!(metadata.sample_rate(), 48_000);
        assert_eq!(metadata.bits_per_sample(), 16);
        assert_eq!(metadata.duration_seconds(), 2.0 / 48_000.0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stereo_wav_is_rejected_before_import() {
        let root = temp_root("stereo-wav");
        let path = root.join("room.wav");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, wav_bytes(2, 44_100, 16, &[0, 0, 0, 0])).unwrap();

        let error = inspect_wav_file(&path).unwrap_err();

        assert!(error.to_string().contains("exactly one channel"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_wav_uses_a_valid_content_addressed_project_path() {
        let root = temp_root("import-wav");
        let source = root.join("small hall.wav");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();
        fs::write(&source, wav_bytes(1, 44_100, 16, &[0, 0])).unwrap();

        let (spec, _) = import_wav_file(&project_directory, &source).unwrap();

        assert_eq!(spec.file_name(), "small hall.wav");
        assert!(project_directory.join(spec.file()).is_file());
        assert!(ImpulseResponseAssetPath::new(spec.file_config_value().to_string()).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    fn wav_bytes(channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let riff_size = 36 + data.len() as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    fn temp_root(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ahess-convolution-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
