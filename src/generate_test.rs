use crate::ahess_error::AhessError;
use rodio;
use rodio::{source::Source, OutputStream};
use std::time::Duration;

struct Audio {
    freq: f32,
    sample: f32,
    volume: f32,
}

const SAMPLE_RATE: u32 = 44100;
const SAMPLE_RATE_FL: f32 = 44100.0;

impl Iterator for Audio {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let r = f32::sin((2.0 * std::f32::consts::PI) * self.freq * (self.sample * SAMPLE_RATE_FL))
            * self.volume;
        self.sample += 1.0;
        Some(r)
    }
}

impl Source for Audio {
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
//
// impl Iterator for Audio {
//     type Item = f32;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         todo!()
//     }
// }
//
// impl Source for Audio {
//     fn current_frame_len(&self) -> Option<usize> {
//         None
//     }
//
//     fn channels(&self) -> u16 {
//         1
//     }
//
//     fn sample_rate(&self) -> u32 {
//         44100
//     }
//
//     fn total_duration(&self) -> Option<Duration> {
//         None
//     }
// }

pub fn run() -> Result<(), AhessError> {
    let (stream, stream_handle) = OutputStream::try_default().unwrap();

    let audio = Audio {
        freq: 400.0,
        sample: 0.0,
        volume: 0.20,
    };
    stream_handle.play_raw(audio).unwrap();

    let audio = Audio {
        freq: 800.0,
        sample: 0.0,
        volume: 0.25,
    };
    stream_handle.play_raw(audio).unwrap();

    let audio = Audio {
        freq: 1200.0,
        sample: 0.0,
        volume: 0.15,
    };
    stream_handle.play_raw(audio).unwrap();

    println!("Started");

    std::thread::sleep(Duration::from_millis(10000));
    // let output_device = rodio::default_output_device().unwrap();

    // rodio::play_raw(
    //     rodio::default_output_device().unwrap(),
    //     rodio::source::SineWave::new(440).take_duration(std::time::Duration::from_secs(2)),
    // );

    Ok(())
}
