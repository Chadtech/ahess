use std::time::Duration;
use rodio::Source;
use crate::ahess_error::AhessError;

pub struct Tone {
    freq: f32,
    sample: f32,
    volume: f32,
}

const SAMPLE_RATE: u32 = 44100;
const SAMPLE_RATE_FL: f32 = 44100.0;

impl Iterator for Tone {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let r = f32::sin(2.0 * std::f32::consts::PI * self.freq * (self.sample / SAMPLE_RATE_FL))
            * self.volume;
        self.sample += 1.0;
        Some(r)
    }
}

impl Source for Tone {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Tone {
    pub fn new(freq: f32) -> Self {
        Tone {
            freq,
            sample: 0.0,
            volume: 1.0,
        }
    }

    pub fn run(self, tag: &str) -> Result<(), AhessError> {
        let (_stream, stream_handle) = rodio::OutputStream::try_default().map_err(|err| {
            AhessError::ToneStreamError {
                tag: tag.to_string(),
                error: err,
            }
        })?;

        match stream_handle.play_raw(self) {
            Ok(_) => {
                std::thread::sleep(Duration::from_secs(1));
                Ok(())
            }
            Err(err) => Err(AhessError::TonePlayError {
                tag: tag.to_string(),
                error: err,
            }),
        }
    }
}