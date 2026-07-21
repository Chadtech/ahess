use std::{error::Error, fmt};

use serde::Deserialize;

pub const SPEED_OF_SOUND_METERS_PER_SECOND: f64 = 343.0;
const EAR_SPACING_METERS: f64 = 0.18;
const REFERENCE_DISTANCE_METERS: f64 = 1.0;
const MAX_ABSOLUTE_COORDINATE_METERS: f64 = 100_000.0;
const MAX_ROOM_DIMENSION_METERS: f64 = 100.0;
const MAX_DIRECT_DISTANCE_METERS: f64 = 1_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3Meters {
    x: f64,
    y: f64,
    z: f64,
}

// Point3Meters rejects NaN in its constructor and deserializer, so its
// derived PartialEq is reflexive and it can safely fulfill Eq's contract.
impl Eq for Point3Meters {}

impl Point3Meters {
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, AcousticError> {
        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || x.abs() > MAX_ABSOLUTE_COORDINATE_METERS
            || y.abs() > MAX_ABSOLUTE_COORDINATE_METERS
            || z.abs() > MAX_ABSOLUTE_COORDINATE_METERS
        {
            return Err(AcousticError::new(
                "acoustic coordinates must be finite and no farther than 100000 meters from the origin",
            ));
        }
        Ok(Self { x, y, z })
    }

    pub const fn origin() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }

    pub const fn z(self) -> f64 {
        self.z
    }

    fn distance_to(self, other: Self) -> f64 {
        let x = self.x - other.x;
        let y = self.y - other.y;
        let z = self.z - other.z;
        (x * x + y * y + z * z).sqrt()
    }

    fn with_x(self, x: f64) -> Self {
        Self { x, ..self }
    }

    fn with_y(self, y: f64) -> Self {
        Self { y, ..self }
    }

    fn with_z(self, z: f64) -> Self {
        Self { z, ..self }
    }
}

impl<'de> Deserialize<'de> for Point3Meters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct StoredPoint {
            x: f64,
            y: f64,
            z: f64,
        }

        let stored = StoredPoint::deserialize(deserializer)?;
        Self::new(stored.x, stored.y, stored.z).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangularRoom {
    width: f64,
    length: f64,
    height: f64,
    reflection_gain: f64,
}

// RectangularRoom applies the same finite-number guarantee as Point3Meters.
impl Eq for RectangularRoom {}

impl RectangularRoom {
    pub fn new(
        width: f64,
        length: f64,
        height: f64,
        reflection_gain: f64,
    ) -> Result<Self, AcousticError> {
        if !width.is_finite()
            || !length.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || length <= 0.0
            || height <= 0.0
            || width > MAX_ROOM_DIMENSION_METERS
            || length > MAX_ROOM_DIMENSION_METERS
            || height > MAX_ROOM_DIMENSION_METERS
        {
            return Err(AcousticError::new(
                "room width, length, and height must be finite, greater than zero, and no greater than 100 meters",
            ));
        }
        if !reflection_gain.is_finite() || !(0.0..=1.0).contains(&reflection_gain) {
            return Err(AcousticError::new(
                "room reflection gain must be a finite number from zero through one",
            ));
        }
        Ok(Self {
            width,
            length,
            height,
            reflection_gain,
        })
    }

    pub const fn width(self) -> f64 {
        self.width
    }

    pub const fn length(self) -> f64 {
        self.length
    }

    pub const fn height(self) -> f64 {
        self.height
    }

    pub const fn reflection_gain(self) -> f64 {
        self.reflection_gain
    }

    pub fn center(self) -> Point3Meters {
        Point3Meters {
            x: self.width / 2.0,
            y: self.length / 2.0,
            z: self.height / 2.0,
        }
    }

    pub fn contains(self, position: Point3Meters) -> bool {
        (0.0..=self.width).contains(&position.x)
            && (0.0..=self.length).contains(&position.y)
            && (0.0..=self.height).contains(&position.z)
    }
}

impl<'de> Deserialize<'de> for RectangularRoom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct StoredRoom {
            width: f64,
            length: f64,
            height: f64,
            reflection_gain: f64,
        }

        let stored = StoredRoom::deserialize(deserializer)?;
        Self::new(
            stored.width,
            stored.length,
            stored.height,
            stored.reflection_gain,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct AcousticScene {
    #[serde(default)]
    listener: Point3Meters,
    #[serde(default)]
    room: Option<RectangularRoom>,
}

impl AcousticScene {
    pub fn new(
        listener: Point3Meters,
        room: Option<RectangularRoom>,
    ) -> Result<Self, AcousticError> {
        let scene = Self { listener, room };
        scene.validate_listener()?;
        Ok(scene)
    }

    pub const fn listener(&self) -> Point3Meters {
        self.listener
    }

    pub const fn room(&self) -> Option<RectangularRoom> {
        self.room
    }

    pub fn validate_source(&self, source: Point3Meters) -> Result<(), AcousticError> {
        if self.room.is_some_and(|room| !room.contains(source)) {
            return Err(AcousticError::new(format!(
                "voice position ({}, {}, {}) must be inside the acoustic room",
                source.x, source.y, source.z
            )));
        }
        if self.listener.distance_to(source) > MAX_DIRECT_DISTANCE_METERS {
            return Err(AcousticError::new(
                "voice must be no farther than 1000 meters from the listener",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), AcousticError> {
        self.validate_listener()
    }

    pub(crate) fn append_config(&self, output: &mut String) {
        if self == &Self::default() {
            return;
        }

        output.push_str("\n[acoustic_scene]\nlistener = ");
        append_point(output, self.listener);
        output.push('\n');
        if let Some(room) = self.room {
            output.push_str("room = { width = ");
            append_float(output, room.width);
            output.push_str(", length = ");
            append_float(output, room.length);
            output.push_str(", height = ");
            append_float(output, room.height);
            output.push_str(", reflection_gain = ");
            append_float(output, room.reflection_gain);
            output.push_str(" }\n");
        }
    }

    fn validate_listener(&self) -> Result<(), AcousticError> {
        let Some(room) = self.room else {
            return Ok(());
        };
        if !room.contains(self.listener) {
            return Err(AcousticError::new(
                "listener position must be inside the acoustic room",
            ));
        }

        let half_ear_spacing = EAR_SPACING_METERS / 2.0;
        let left_ear = self.listener.with_x(self.listener.x - half_ear_spacing);
        let right_ear = self.listener.with_x(self.listener.x + half_ear_spacing);
        if !room.contains(left_ear) || !room.contains(right_ear) {
            return Err(AcousticError::new(
                "listener must be at least half the ear spacing from either side wall",
            ));
        }
        Ok(())
    }
}

pub(crate) fn append_point(output: &mut String, point: Point3Meters) {
    output.push_str("{ x = ");
    append_float(output, point.x);
    output.push_str(", y = ");
    append_float(output, point.y);
    output.push_str(", z = ");
    append_float(output, point.z);
    output.push_str(" }");
}

fn append_float(output: &mut String, value: f64) {
    use std::fmt::Write as _;
    write!(output, "{value:?}").expect("writing to a String cannot fail");
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct StereoFrame {
    pub left: f32,
    pub right: f32,
}

impl StereoFrame {
    pub const SILENCE: Self = Self {
        left: 0.0,
        right: 0.0,
    };

    pub fn add(&mut self, other: Self) {
        self.left += other.left;
        self.right += other.right;
    }

    pub fn scale(self, gain: f32) -> Self {
        Self {
            left: self.left * gain,
            right: self.right * gain,
        }
    }

    pub fn clamp(self) -> Self {
        Self {
            left: self.left.clamp(-1.0, 1.0),
            right: self.right.clamp(-1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AcousticPath {
    left_delay_samples: f64,
    right_delay_samples: f64,
    left_gain: f32,
    right_gain: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct SpatializerSpec {
    paths: Vec<AcousticPath>,
    maximum_delay_samples: usize,
}

impl SpatializerSpec {
    fn new(scene: &AcousticScene, source: Point3Meters, sample_rate: f64) -> Self {
        let mut paths = Vec::with_capacity(if scene.room.is_some() { 7 } else { 1 });
        paths.push(acoustic_path(scene.listener, source, 1.0, sample_rate));

        if let Some(room) = scene.room {
            let reflected_sources = [
                source.with_x(-source.x),
                source.with_x(2.0 * room.width - source.x),
                source.with_y(-source.y),
                source.with_y(2.0 * room.length - source.y),
                source.with_z(-source.z),
                source.with_z(2.0 * room.height - source.z),
            ];
            paths.extend(reflected_sources.into_iter().map(|image| {
                acoustic_path(scene.listener, image, room.reflection_gain, sample_rate)
            }));
        }

        let maximum_delay_samples = paths
            .iter()
            .flat_map(|path| [path.left_delay_samples, path.right_delay_samples])
            .fold(0.0_f64, f64::max)
            .ceil() as usize;

        Self {
            paths,
            maximum_delay_samples,
        }
    }
}

fn acoustic_path(
    listener: Point3Meters,
    image_source: Point3Meters,
    path_gain: f64,
    sample_rate: f64,
) -> AcousticPath {
    let half_ear_spacing = EAR_SPACING_METERS / 2.0;
    let left_ear = listener.with_x(listener.x - half_ear_spacing);
    let right_ear = listener.with_x(listener.x + half_ear_spacing);
    let center_distance = listener.distance_to(image_source);
    let left_distance = left_ear.distance_to(image_source);
    let right_distance = right_ear.distance_to(image_source);
    let average_ear_distance = (left_distance + right_distance) / 2.0;
    let meters_to_samples = sample_rate / SPEED_OF_SOUND_METERS_PER_SECOND;
    let left_delay_samples =
        (center_distance + left_distance - average_ear_distance).max(0.0) * meters_to_samples;
    let right_delay_samples =
        (center_distance + right_distance - average_ear_distance).max(0.0) * meters_to_samples;

    AcousticPath {
        left_delay_samples,
        right_delay_samples,
        left_gain: (path_gain * distance_attenuation(left_distance)) as f32,
        right_gain: (path_gain * distance_attenuation(right_distance)) as f32,
    }
}

fn distance_attenuation(distance: f64) -> f64 {
    if distance <= REFERENCE_DISTANCE_METERS {
        1.0
    } else {
        REFERENCE_DISTANCE_METERS / distance
    }
}

pub(crate) struct VoiceSpatializer {
    spec: SpatializerSpec,
    delay_line: DelayLine,
    tail_samples_remaining: usize,
}

impl VoiceSpatializer {
    pub fn new(scene: &AcousticScene, source: Point3Meters, sample_rate: f64) -> Self {
        let spec = SpatializerSpec::new(scene, source, sample_rate);
        let delay_line = DelayLine::new(spec.maximum_delay_samples + 2);
        Self {
            spec,
            delay_line,
            tail_samples_remaining: 0,
        }
    }

    pub fn process(&mut self, source_sample: f32, source_is_active: bool) -> (StereoFrame, bool) {
        self.delay_line.push(source_sample);
        if source_is_active {
            self.tail_samples_remaining = self.spec.maximum_delay_samples + 1;
        } else {
            self.tail_samples_remaining = self.tail_samples_remaining.saturating_sub(1);
        }

        let mut frame = StereoFrame::SILENCE;
        for path in &self.spec.paths {
            frame.left += self.delay_line.read(path.left_delay_samples) * path.left_gain;
            frame.right += self.delay_line.read(path.right_delay_samples) * path.right_gain;
        }
        self.delay_line.advance();
        (frame, source_is_active || self.tail_samples_remaining > 0)
    }
}

struct DelayLine {
    samples: Vec<f32>,
    write_index: usize,
}

impl DelayLine {
    fn new(length: usize) -> Self {
        Self {
            samples: vec![0.0; length.max(2)],
            write_index: 0,
        }
    }

    fn push(&mut self, sample: f32) {
        self.samples[self.write_index] = sample;
    }

    fn advance(&mut self) {
        self.write_index = (self.write_index + 1) % self.samples.len();
    }

    fn read(&self, delay_samples: f64) -> f32 {
        let whole_samples = delay_samples.floor() as usize;
        let fraction = (delay_samples - whole_samples as f64) as f32;
        let newest = self.sample_at_age(whole_samples);
        let older = self.sample_at_age(whole_samples + 1);
        newest + (older - newest) * fraction
    }

    fn sample_at_age(&self, age: usize) -> f32 {
        let age = age % self.samples.len();
        let index = (self.write_index + self.samples.len() - age) % self.samples.len();
        self.samples[index]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcousticError {
    message: String,
}

impl AcousticError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AcousticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AcousticError {}

#[cfg(test)]
mod tests {
    use super::{
        AcousticScene, DelayLine, Point3Meters, RectangularRoom, SpatializerSpec, VoiceSpatializer,
    };

    #[test]
    fn acoustic_values_reject_non_finite_and_out_of_range_numbers() {
        assert!(Point3Meters::new(f64::NAN, 0.0, 0.0).is_err());
        assert!(Point3Meters::new(100_001.0, 0.0, 0.0).is_err());
        assert!(RectangularRoom::new(0.0, 4.0, 3.0, 0.25).is_err());
        assert!(RectangularRoom::new(101.0, 4.0, 3.0, 0.25).is_err());
        assert!(RectangularRoom::new(5.0, 4.0, 3.0, 1.01).is_err());
        assert!(AcousticScene::default()
            .validate_source(Point3Meters::new(1_001.0, 0.0, 0.0).unwrap())
            .is_err());
    }

    #[test]
    fn a_room_requires_the_listener_and_both_ears_to_be_inside() {
        let room = RectangularRoom::new(5.0, 4.0, 3.0, 0.25).unwrap();

        assert!(AcousticScene::new(Point3Meters::new(2.5, 2.0, 1.6).unwrap(), Some(room)).is_ok());
        assert!(
            AcousticScene::new(Point3Meters::new(0.05, 2.0, 1.6).unwrap(), Some(room)).is_err()
        );
    }

    #[test]
    fn a_centered_free_field_voice_preserves_equal_channels_without_delay() {
        let scene = AcousticScene::default();
        let spec = SpatializerSpec::new(&scene, Point3Meters::origin(), 48_000.0);

        assert_eq!(spec.paths.len(), 1);
        assert_eq!(spec.paths[0].left_delay_samples, 0.0);
        assert_eq!(spec.paths[0].right_delay_samples, 0.0);
        assert!((spec.paths[0].left_gain - 1.0).abs() < 1e-6);
        assert!((spec.paths[0].right_gain - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mirrored_sources_swap_direct_ear_delays_and_gains() {
        let scene = AcousticScene::default();
        let left =
            SpatializerSpec::new(&scene, Point3Meters::new(-2.0, 2.0, 0.0).unwrap(), 48_000.0);
        let right =
            SpatializerSpec::new(&scene, Point3Meters::new(2.0, 2.0, 0.0).unwrap(), 48_000.0);

        assert!(
            (left.paths[0].left_delay_samples - right.paths[0].right_delay_samples).abs() < 1e-9
        );
        assert!(
            (left.paths[0].right_delay_samples - right.paths[0].left_delay_samples).abs() < 1e-9
        );
        assert!((left.paths[0].left_gain - right.paths[0].right_gain).abs() < 1e-6);
        assert!((left.paths[0].right_gain - right.paths[0].left_gain).abs() < 1e-6);
    }

    #[test]
    fn a_lateral_source_reaches_both_ears_without_hard_panning() {
        let scene = AcousticScene::default();
        let spec =
            SpatializerSpec::new(&scene, Point3Meters::new(-2.0, 0.0, 0.0).unwrap(), 48_000.0);
        let direct = spec.paths[0];

        assert!(direct.left_gain > direct.right_gain);
        assert!(direct.right_gain > 0.0);
        assert!(direct.left_delay_samples < direct.right_delay_samples);
    }

    #[test]
    fn a_rectangular_room_adds_one_reflection_from_each_surface() {
        let room = RectangularRoom::new(5.0, 4.0, 3.0, 0.25).unwrap();
        let listener = Point3Meters::new(2.5, 2.0, 1.5).unwrap();
        let scene = AcousticScene::new(listener, Some(room)).unwrap();
        let source = Point3Meters::new(1.0, 3.0, 1.0).unwrap();

        let spec = SpatializerSpec::new(&scene, source, 48_000.0);

        assert_eq!(spec.paths.len(), 7);
        assert!(spec.paths[1..].iter().all(|reflection| {
            reflection.left_delay_samples > spec.paths[0].left_delay_samples
                || reflection.right_delay_samples > spec.paths[0].right_delay_samples
        }));
    }

    #[test]
    fn fractional_delay_line_interpolates_and_advances() {
        let mut delay = DelayLine::new(4);
        delay.push(1.0);
        assert_eq!(delay.read(0.0), 1.0);
        assert_eq!(delay.read(0.5), 0.5);
        delay.advance();
        delay.push(0.0);
        assert_eq!(delay.read(1.0), 1.0);
    }

    #[test]
    fn delayed_audio_survives_after_the_source_becomes_inactive() {
        let scene = AcousticScene::default();
        let source = Point3Meters::new(0.0, 3.43, 0.0).unwrap();
        let mut spatializer = VoiceSpatializer::new(&scene, source, 1_000.0);

        spatializer.process(1.0, true);
        let mut heard = false;
        for _ in 0..20 {
            let (frame, active) = spatializer.process(0.0, false);
            heard |= frame.left != 0.0 || frame.right != 0.0;
            if heard {
                assert!(active);
                break;
            }
        }

        assert!(heard);
    }
}
