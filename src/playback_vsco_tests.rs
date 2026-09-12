use super::*;
use crate::{
    audio_build::{build_project_audio, BuildSampleRate},
    pitch_system::{ExplicitPitchSystem, PitchSystem},
    project::{create_project, load_project, Voice},
    score_cell::{BeatOffset, CellEvent},
};

fn fixture() -> (Project, Part, PartScore) {
    let tuning = PitchSystem::explicit(
        ExplicitPitchSystem::new(
            "VSCO custom frequencies",
            [
                ("a".to_owned(), FrequencyHz::new(259.0).unwrap()),
                ("b".to_owned(), FrequencyHz::new(323.75).unwrap()),
                ("c".to_owned(), FrequencyHz::new(388.5).unwrap()),
                ("d".to_owned(), FrequencyHz::new(453.25).unwrap()),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap(),
    );
    let part = Part::new("sample voices", 24);
    let project = Project::new("VSCO 2 custom tuning", 350, 0, Seed::new(1))
        .with_pitch_system(tuning)
        .with_parts(vec![part.clone()])
        .with_voices(
            [
                VoiceType::VscoCello,
                VoiceType::VscoFlute,
                VoiceType::VscoClarinet,
                VoiceType::VscoHarp,
            ]
            .into_iter()
            .enumerate()
            .map(|(i, t)| Voice::new(i as u64 + 1, t.label(), t))
            .collect(),
        );
    let event = |pitch: &str, duration: &str, offset: i32| {
        CellEvent::from_fields(
            project.pitch_system(),
            BeatOffset::from_ticks(offset).unwrap(),
            pitch,
            duration,
            "C0",
        )
        .unwrap()
        .unwrap()
    };
    let mut rows = vec![vec![String::new(); 4]; 24];
    for voice in 0..4 {
        rows[voice * 4][voice] =
            crate::score_cell::encode(&[event("a", "2", 0), event("b", "1", 48)]);
        rows[voice * 4 + 3][voice] = "c@A0".into();
    }
    for row in 16..24 {
        for voice in 0..4 {
            rows[row][voice] = format!("{}@A0", ["a", "b", "c", "d"][(row + voice) % 4]);
        }
    }
    (project, part, PartScore::from_rows(rows))
}

#[test]
fn vsco_custom_score_events_match_live_offline_and_stems_at_all_rates() {
    let (project, part, score) = fixture();
    for rate in [44_100, 48_000, 96_000] {
        let prepared = PlaybackLoop::from_part(&project, &part, &score).unwrap();
        assert!(prepared.voices.iter().all(|v| v.events.is_some()));
        let mut live = AudioEngine::new(
            rate as f32,
            Arc::new(Mutex::new(prepared.clone())),
            Arc::new(AtomicU64::new(1)),
        )
        .unwrap();
        let mut offline = OfflineRenderer::new(prepared, rate).unwrap();
        let frames = offline.score_frame_count();
        let mut energy = [0.0; 4];
        for _ in 0..frames {
            let (mix, stems) = offline.next_frame().unwrap();
            let live_mix = live.next_frame();
            // Live mixes before master gain; offline scales stems before summing.
            assert!((mix.left - live_mix.left).abs() < 1e-6);
            assert!((mix.right - live_mix.right).abs() < 1e-6);
            let sum: f32 = stems.iter().map(|s| s.left).sum();
            assert!((sum - mix.left).abs() < 0.00001);
            for (e, s) in energy.iter_mut().zip(stems) {
                *e += f64::from(s.left * s.left);
            }
        }
        assert!(energy.iter().all(|e| *e > 1.0));
        let mut tail = 0;
        while let Some((mix, _)) = offline.next_frame() {
            assert!(mix.left.is_finite() && mix.right.is_finite());
            tail += 1;
            assert!(tail < rate * 15);
        }
    }
}

#[test]
#[ignore = "writes an audition project; run explicitly with AHESS_VSCO_DEMO_ROOT"]
fn vsco_write_audition_project() {
    let root = std::path::PathBuf::from(
        std::env::var_os("AHESS_VSCO_DEMO_ROOT")
            .expect("set AHESS_VSCO_DEMO_ROOT to the destination workspace"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let (project, part, score) = fixture();
    let directory = create_project(&root, &project).unwrap();
    score.save(&directory, &part, &project).unwrap();
    let loaded = load_project(&directory).unwrap();
    assert_eq!(loaded.project, project);
    let score = PartScore::load(&directory, &part, project.voices()).unwrap();
    let scores = vec![(part, score)];
    let prepared = PlaybackLoop::from_project_arrangement(
        &project,
        &scores,
        BeatRange::new(1, 24, 24).unwrap(),
    )
    .unwrap();
    let result = build_project_audio(
        &directory,
        &project,
        &scores,
        prepared,
        BuildSampleRate::Hz48000,
    )
    .unwrap();
    assert_eq!(result.file_count, 9);
    println!("VSCO audition project: {}", directory.display());
}

#[test]
#[ignore = "plays three seconds through the actual audio device"]
fn vsco_live_device_smoke_test() {
    let (project, part, score) = fixture();
    let playback =
        Playback::start(PlaybackLoop::from_part(&project, &part, &score).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(playback.current_arrangement_beat() > 1);
}

#[test]
fn vsco_blend_seeds_follow_absolute_score_positions_and_project_seed() {
    let (project, part, score) = fixture();
    let rows = score.resolved_strikes(&part, &project).unwrap();
    let full = PlaybackLoop::from_rows(&project, rows.clone(), 1).unwrap();
    let partial = PlaybackLoop::from_rows(&project, rows[4..].to_vec(), 5).unwrap();
    for (a, b) in full.voices.iter().zip(&partial.voices) {
        assert_eq!(&a.blend_seeds[4..], b.blend_seeds);
        assert_ne!(a.blend_seeds[0].derive(0), a.blend_seeds[0].derive(1));
        assert_ne!(a.blend_seeds[0], a.blend_seeds[1]);
    }
    assert_ne!(full.voices[0].blend_seeds, full.voices[1].blend_seeds);
    let mut changed = project.clone();
    changed.seed = Seed::new(2);
    let reshuffled = PlaybackLoop::from_rows(&changed, rows, 1).unwrap();
    assert_ne!(full.voices[0].blend_seeds, reshuffled.voices[0].blend_seeds);
}

#[test]
#[ignore = "writes a repeated-note blend audition; set AHESS_VSCO_DEMO_ROOT"]
fn vsco_write_blend_variation_audition() {
    let root = std::path::PathBuf::from(std::env::var_os("AHESS_VSCO_DEMO_ROOT").unwrap());
    let (original, _, _) = fixture();
    let part = Part::new("repeated notes", 48);
    let project = Project::new("VSCO blend variation", 500, 0, Seed::new(42))
        .with_pitch_system(original.pitch_system().clone())
        .with_voices(original.voices().to_vec())
        .with_parts(vec![part.clone()]);
    let mut rows = vec![vec![String::new(); 4]; 48];
    for (voice, pitch) in ["a", "d", "c", "b"].into_iter().enumerate() {
        for repeat in 0..6 {
            let event = CellEvent::from_fields(
                project.pitch_system(),
                BeatOffset::from_ticks(0).unwrap(),
                pitch,
                "5/4",
                "C0",
            )
            .unwrap()
            .unwrap();
            // One onset per second, with fixed pitch, volume, and duration.
            rows[voice * 12 + repeat * 2][voice] = crate::score_cell::encode(&[event]);
        }
    }
    let score = PartScore::from_rows(rows);
    let directory = create_project(&root, &project).unwrap();
    score.save(&directory, &part, &project).unwrap();
    let scores = vec![(part, score)];
    let prepared = PlaybackLoop::from_project_arrangement(
        &project,
        &scores,
        BeatRange::new(1, 48, 48).unwrap(),
    )
    .unwrap();
    let result = build_project_audio(
        &directory,
        &project,
        &scores,
        prepared,
        BuildSampleRate::Hz48000,
    )
    .unwrap();
    assert_eq!(result.file_count, 9);
    println!("{}", directory.display());
}

#[test]
#[ignore = "writes the sharp-attack flute and clarinet demo; set AHESS_VSCO_DEMO_ROOT"]
fn vsco_write_sharp_attack_audition() {
    let root = std::path::PathBuf::from(std::env::var_os("AHESS_VSCO_DEMO_ROOT").unwrap());
    let (original, _, _) = fixture();
    let part = Part::new("sharp attacks", 32);
    let sharp = crate::voice::AttackSharpness::new(100).unwrap();
    let project = Project::new("VSCO sharp attacks", 500, 0, Seed::new(42))
        .with_pitch_system(original.pitch_system().clone())
        .with_voices(vec![
            Voice::new(1, "flute", VoiceType::VscoFlute).with_attack_sharpness(sharp),
            Voice::new(2, "clarinet", VoiceType::VscoClarinet),
        ])
        .with_parts(vec![part.clone()]);
    let mut rows = vec![vec![String::new(); 2]; 32];
    for voice in 0..2 {
        for repeat in 0..8 {
            let pitch = if voice == 0 {
                ["d", "d", "d", "d", "c", "b", "c", "d"][repeat]
            } else {
                ["c", "c", "c", "c", "b", "a", "b", "c"][repeat]
            };
            let mut event = CellEvent::from_fields(
                project.pitch_system(),
                BeatOffset::from_ticks(0).unwrap(),
                pitch,
                "1",
                "C0",
            )
            .unwrap()
            .unwrap();
            // Flute inherits its default; clarinet demonstrates individual overrides.
            if voice == 1 {
                event.attack_sharpness = Some(sharp);
            }
            rows[voice * 16 + repeat * 2][voice] = crate::score_cell::encode(&[event]);
        }
    }
    let score = PartScore::from_rows(rows);
    let directory = create_project(&root, &project).unwrap();
    score.save(&directory, &part, &project).unwrap();
    assert_eq!(load_project(&directory).unwrap().project, project);
    let scores = vec![(part, score)];
    let prepared = PlaybackLoop::from_project_arrangement(
        &project,
        &scores,
        BeatRange::new(1, 32, 32).unwrap(),
    )
    .unwrap();
    build_project_audio(
        &directory,
        &project,
        &scores,
        prepared,
        BuildSampleRate::Hz48000,
    )
    .unwrap();
    println!("{}", directory.display());
}

#[test]
fn vsco_attack_defaults_and_overrides_agree_between_live_and_export() {
    let (original, _, _) = fixture();
    let sharp = crate::voice::AttackSharpness::new(100).unwrap();
    let part = Part::new("attack inheritance", 4);
    let project = Project::new("attack inheritance", 250, 0, Seed::new(42))
        .with_pitch_system(original.pitch_system().clone())
        .with_voices(vec![
            Voice::new(1, "flute", VoiceType::VscoFlute).with_attack_sharpness(sharp)
        ])
        .with_parts(vec![part.clone()]);
    let mut inherited = crate::score_cell::parse(project.pitch_system(), "d").unwrap();
    let inherited_text = crate::score_cell::encode(&inherited);
    inherited[0].attack_sharpness = Some(sharp);
    let overridden = crate::score_cell::encode(&inherited);
    let render = |text: String| {
        let score = PartScore::from_rows(vec![
            vec![text],
            vec![String::new()],
            vec![String::new()],
            vec![String::new()],
        ]);
        let prepared = PlaybackLoop::from_part(&project, &part, &score).unwrap();
        let mut live = AudioEngine::new(
            48_000.0,
            Arc::new(Mutex::new(prepared.clone())),
            Arc::new(AtomicU64::new(1)),
        )
        .unwrap();
        let mut offline = OfflineRenderer::new(prepared, 48_000).unwrap();
        (0..48_000)
            .map(|_| {
                let mix = offline.next_frame().unwrap().0;
                let actual = live.next_frame();
                assert!((mix.left - actual.left).abs() < 1e-6);
                mix.left
            })
            .collect::<Vec<_>>()
    };
    let expected = render(overridden);
    assert_eq!(render(inherited_text), expected);
    assert_eq!(render("d".into()), expected);
    inherited[0].attack_sharpness = Some(Default::default());
    assert_ne!(render(crate::score_cell::encode(&inherited)), expected);
}
