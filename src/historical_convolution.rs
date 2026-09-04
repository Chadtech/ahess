use fft_convolver::FFTConvolver;
use rubato::{audioadapter_buffers::owned::InterleavedOwned, Fft, FixedSync, Resampler};

use crate::voice::VoiceType;

const SOURCE_SAMPLE_RATE: usize = 44_100;
const CONVOLUTION_BLOCK_SIZE: usize = 128;
const EXPENSIVE_E_WAV: &[u8] =
    include_bytes!("../assets/impulse-responses/recovered-noitech/expensiveE.wav");
const HOME_CLAP_1_WAV: &[u8] =
    include_bytes!("../assets/impulse-responses/recovered-noitech/home_clap_1.wav");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoricalImpulse {
    ExpensiveE,
    HomeClap1,
}

impl HistoricalImpulse {
    const fn wav(self) -> &'static [u8] {
        match self {
            Self::ExpensiveE => EXPENSIVE_E_WAV,
            Self::HomeClap1 => HOME_CLAP_1_WAV,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HistoricalConvolutionSpec {
    impulse: HistoricalImpulse,
    wet_gain: f32,
}

impl HistoricalConvolutionSpec {
    const fn for_voice(voice_type: VoiceType) -> Option<Self> {
        let (impulse, wet_gain) = match voice_type {
            VoiceType::NoitechBellG
            | VoiceType::NoitechBellI
            | VoiceType::NoitechBellJ
            | VoiceType::NoitechBellK => (HistoricalImpulse::ExpensiveE, 0.15),
            VoiceType::NoitechBellH => (HistoricalImpulse::ExpensiveE, 0.25),
            VoiceType::NoitechBellL | VoiceType::NoitechBellM => {
                (HistoricalImpulse::HomeClap1, 0.05)
            }
            _ => return None,
        };
        Some(Self { impulse, wet_gain })
    }
}

/// Restores the dry-plus-convolved topology used by the historical Noitech
/// bell builders. The first response block is evaluated directly so the
/// strike remains sample-aligned; the remaining response is processed one
/// fixed block ahead with partitioned FFT convolution.
pub(crate) struct HistoricalBellConvolver {
    response_len: usize,
    direct_response: Vec<f32>,
    direct_history: Vec<f32>,
    direct_history_position: usize,
    tail_convolver: Option<FFTConvolver<f32>>,
    tail_input: [f32; CONVOLUTION_BLOCK_SIZE],
    tail_output: [f32; CONVOLUTION_BLOCK_SIZE],
    next_tail_output: [f32; CONVOLUTION_BLOCK_SIZE],
    block_position: usize,
    tail_samples_remaining: usize,
}

impl HistoricalBellConvolver {
    pub(crate) fn for_voice(voice_type: VoiceType, sample_rate: f32) -> Option<Self> {
        let spec = HistoricalConvolutionSpec::for_voice(voice_type)?;
        let target_sample_rate = sample_rate.round().max(1.0) as usize;
        let mut response = prepare_response(spec.impulse, target_sample_rate)
            .expect("bundled historical impulse responses must remain valid");
        for sample in &mut response {
            *sample *= spec.wet_gain;
        }
        Some(Self::from_prepared_response(response))
    }

    fn from_prepared_response(response: Vec<f32>) -> Self {
        let response_len = response.len();
        let direct_len = response_len.min(CONVOLUTION_BLOCK_SIZE);
        let direct_response = response[..direct_len].to_vec();
        let tail_convolver = if response_len > CONVOLUTION_BLOCK_SIZE {
            let mut convolver = FFTConvolver::default();
            convolver
                .init(CONVOLUTION_BLOCK_SIZE, &response[CONVOLUTION_BLOCK_SIZE..])
                .expect("a fixed nonzero convolution block size must initialize");
            Some(convolver)
        } else {
            None
        };

        Self {
            response_len,
            direct_history: vec![0.0; direct_response.len()],
            direct_response,
            direct_history_position: 0,
            tail_convolver,
            tail_input: [0.0; CONVOLUTION_BLOCK_SIZE],
            tail_output: [0.0; CONVOLUTION_BLOCK_SIZE],
            next_tail_output: [0.0; CONVOLUTION_BLOCK_SIZE],
            block_position: 0,
            tail_samples_remaining: 0,
        }
    }

    pub(crate) fn process(&mut self, input: f32, source_is_active: bool) -> (f32, bool) {
        let direct = self.process_direct(input);
        let tail = self.process_tail(input);
        let tail_was_active = self.tail_samples_remaining > 0;

        if source_is_active {
            self.tail_samples_remaining = self.response_len.saturating_sub(1);
        } else if tail_was_active {
            self.tail_samples_remaining -= 1;
        }

        (input + direct + tail, source_is_active || tail_was_active)
    }

    fn process_direct(&mut self, input: f32) -> f32 {
        if self.direct_response.is_empty() {
            return 0.0;
        }

        self.direct_history[self.direct_history_position] = input;
        let mut history_position = self.direct_history_position;
        let mut output = 0.0;
        for coefficient in &self.direct_response {
            output += coefficient * self.direct_history[history_position];
            history_position = if history_position == 0 {
                self.direct_history.len() - 1
            } else {
                history_position - 1
            };
        }
        self.direct_history_position =
            (self.direct_history_position + 1) % self.direct_history.len();
        output
    }

    fn process_tail(&mut self, input: f32) -> f32 {
        let Some(convolver) = &mut self.tail_convolver else {
            return 0.0;
        };

        let output = self.tail_output[self.block_position];
        self.tail_input[self.block_position] = input;
        self.block_position += 1;

        if self.block_position == CONVOLUTION_BLOCK_SIZE {
            convolver
                .process(&self.tail_input, &mut self.next_tail_output)
                .expect("equal fixed-size convolution buffers must process");
            std::mem::swap(&mut self.tail_output, &mut self.next_tail_output);
            self.tail_input.fill(0.0);
            self.next_tail_output.fill(0.0);
            self.block_position = 0;
        }

        output
    }
}

fn prepare_response(
    impulse: HistoricalImpulse,
    target_sample_rate: usize,
) -> Result<Vec<f32>, String> {
    let response = decode_legacy_pcm16_wav(impulse.wav())?;
    if target_sample_rate == SOURCE_SAMPLE_RATE {
        return Ok(response);
    }

    let input_len = response.len();
    let input = InterleavedOwned::new_from(response, 1, input_len)
        .map_err(|error| format!("invalid impulse response buffer: {error}"))?;
    let mut resampler = Fft::<f32>::new(
        SOURCE_SAMPLE_RATE,
        target_sample_rate,
        1_024,
        1,
        FixedSync::Both,
    )
    .map_err(|error| format!("failed to prepare impulse-response resampler: {error}"))?;
    resampler
        .process_all(&input, input_len, None)
        .map(InterleavedOwned::take_data)
        .map_err(|error| format!("failed to resample impulse response: {error}"))
}

/// The original Go convolver decoded negative PCM values by subtracting
/// 65,535 rather than 65,536. Preserve that one-unit asymmetry here so the
/// bundled responses match the historical generator's input values.
fn decode_legacy_pcm16_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("impulse response is not a RIFF/WAVE file".to_string());
    }

    let mut offset = 12_usize;
    let mut valid_format = false;
    let mut samples = Vec::new();
    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_len = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .expect("a four-byte slice converts to an array"),
        ) as usize;
        let chunk_start = offset + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_len)
            .ok_or_else(|| "impulse response chunk length overflowed".to_string())?;
        if chunk_end > bytes.len() {
            return Err("impulse response chunk extends past the file".to_string());
        }

        match chunk_id {
            b"fmt " => {
                if chunk_len < 16 {
                    return Err("impulse response format chunk is incomplete".to_string());
                }
                let format = &bytes[chunk_start..chunk_end];
                let audio_format = u16::from_le_bytes([format[0], format[1]]);
                let channels = u16::from_le_bytes([format[2], format[3]]);
                let sample_rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
                let bits_per_sample = u16::from_le_bytes([format[14], format[15]]);
                valid_format = audio_format == 1
                    && channels == 1
                    && sample_rate == SOURCE_SAMPLE_RATE as u32
                    && bits_per_sample == 16;
            }
            b"data" => {
                if !chunk_len.is_multiple_of(2) {
                    return Err("impulse response ends in a partial PCM sample".to_string());
                }
                samples.extend(bytes[chunk_start..chunk_end].chunks_exact(2).map(|sample| {
                    let raw = u16::from_le_bytes([sample[0], sample[1]]);
                    let historical = if raw > i16::MAX as u16 {
                        i32::from(raw) - 65_535
                    } else {
                        i32::from(raw)
                    };
                    historical as f32 / f32::from(i16::MAX)
                }));
            }
            _ => {}
        }

        offset = chunk_end + (chunk_len % 2);
    }

    if !valid_format {
        return Err("impulse response must be mono 16-bit PCM at 44.1 kHz".to_string());
    }
    if samples.is_empty() {
        return Err("impulse response contains no samples".to_string());
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_legacy_pcm16_wav, prepare_response, HistoricalBellConvolver,
        HistoricalConvolutionSpec, HistoricalImpulse,
    };
    use crate::voice::VoiceType;

    #[test]
    fn bundled_responses_retain_their_historical_lengths() {
        assert_eq!(
            decode_legacy_pcm16_wav(HistoricalImpulse::ExpensiveE.wav())
                .unwrap()
                .len(),
            303
        );
        assert_eq!(
            decode_legacy_pcm16_wav(HistoricalImpulse::HomeClap1.wav())
                .unwrap()
                .len(),
            15_328
        );
    }

    #[test]
    fn response_is_resampled_for_the_active_output_rate() {
        let response = prepare_response(HistoricalImpulse::ExpensiveE, 48_000).unwrap();
        assert_eq!(response.len(), 330);
        assert!(response.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn source_bells_keep_their_original_responses_and_wet_gains() {
        let expected = [
            (VoiceType::NoitechBellG, HistoricalImpulse::ExpensiveE, 0.15),
            (VoiceType::NoitechBellH, HistoricalImpulse::ExpensiveE, 0.25),
            (VoiceType::NoitechBellI, HistoricalImpulse::ExpensiveE, 0.15),
            (VoiceType::NoitechBellJ, HistoricalImpulse::ExpensiveE, 0.15),
            (VoiceType::NoitechBellK, HistoricalImpulse::ExpensiveE, 0.15),
            (VoiceType::NoitechBellL, HistoricalImpulse::HomeClap1, 0.05),
            (VoiceType::NoitechBellM, HistoricalImpulse::HomeClap1, 0.05),
        ];
        for (voice_type, impulse, wet_gain) in expected {
            assert_eq!(
                HistoricalConvolutionSpec::for_voice(voice_type),
                Some(HistoricalConvolutionSpec { impulse, wet_gain })
            );
        }
        assert_eq!(
            HistoricalConvolutionSpec::for_voice(VoiceType::NoitechBellA),
            None
        );
        assert_eq!(
            HistoricalConvolutionSpec::for_voice(VoiceType::IconoclastBellG),
            None
        );
    }

    #[test]
    fn zero_latency_partition_matches_direct_dry_plus_wet_convolution() {
        let response = (0..401)
            .map(|index| ((index as f32 * 0.37).sin() * 0.02) / (index + 1) as f32)
            .collect::<Vec<_>>();
        let input = (0..521)
            .map(|index| (index as f32 * 0.11).cos() * 0.3)
            .collect::<Vec<_>>();
        let mut convolver = HistoricalBellConvolver::from_prepared_response(response.clone());
        let mut actual = Vec::new();
        for index in 0..input.len() + response.len() - 1 {
            let source_is_active = index < input.len();
            let source = input.get(index).copied().unwrap_or(0.0);
            let (sample, active) = convolver.process(source, source_is_active);
            actual.push(sample);
            assert!(active);
        }
        let (_, active) = convolver.process(0.0, false);
        assert!(!active);

        for (index, actual) in actual.into_iter().enumerate() {
            let wet = response
                .iter()
                .enumerate()
                .filter_map(|(response_index, coefficient)| {
                    index
                        .checked_sub(response_index)
                        .and_then(|input_index| input.get(input_index))
                        .map(|input| input * coefficient)
                })
                .sum::<f32>();
            let expected = input.get(index).copied().unwrap_or(0.0) + wet;
            assert!(
                (actual - expected).abs() < 0.000_01,
                "sample {index}: actual {actual}, expected {expected}"
            );
        }
    }
}
