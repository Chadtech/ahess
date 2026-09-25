use super::*;
use crate::{
    audio_build::{build_project_audio, BuildSampleRate},
    pitch_system::{ExplicitPitchSystem, PitchSystem},
    project::{create_project, load_project, Voice},
    score_cell::{self, BeatOffset, CellEvent},
};

fn fixture() -> (Project, Part, PartScore) {
    let tuning = PitchSystem::explicit(
        ExplicitPitchSystem::new(
            "guitar audition",
            [
                ("a".to_owned(), FrequencyHz::new(129.5).unwrap()),
                ("b".to_owned(), FrequencyHz::new(161.875).unwrap()),
                ("c".to_owned(), FrequencyHz::new(194.25).unwrap()),
                ("d".to_owned(), FrequencyHz::new(259.0).unwrap()),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap(),
    );
    let part = Part::new("plucks and strums", 24);
    let project = Project::new("clean guitar audition", 500, 0, Seed::new(14))
        .with_pitch_system(tuning)
        .with_parts(vec![part.clone()])
        .with_voices(vec![Voice::new(1, "clean guitar", VoiceType::CleanGuitar)]);
    let mut rows = vec![vec![String::new()]; 24];
    for i in 0..8 {
        rows[i][0] = format!(
            "{}@{}",
            ["a", "b", "c", "d"][i % 4],
            if i < 4 { "90" } else { "FF" }
        );
    }
    for i in [8, 10, 12, 14] {
        rows[i][0] = score_cell::encode(
            &["a", "b", "c", "d"]
                .iter()
                .enumerate()
                .map(|(n, pitch)| {
                    CellEvent::from_fields(
                        project.pitch_system(),
                        BeatOffset::from_ticks(n as i32 * 3).unwrap(),
                        pitch,
                        "3/2",
                        "D0",
                    )
                    .unwrap()
                    .unwrap()
                })
                .collect::<Vec<_>>(),
        );
    }
    for i in 16..24 {
        rows[i][0] = score_cell::encode(&[CellEvent::from_fields(
            project.pitch_system(),
            BeatOffset::from_ticks(0).unwrap(),
            "d",
            "1/4",
            if i % 2 == 0 { "FF" } else { "80" },
        )
        .unwrap()
        .unwrap()]);
    }
    (project, part, PartScore::from_rows(rows))
}

#[test]
fn clean_guitar_detailed_score_matches_live_export_and_stems() {
    let (project, part, score) = fixture();
    for rate in [44_100, 48_000, 96_000] {
        let prepared = PlaybackLoop::from_part(&project, &part, &score).unwrap();
        let mut live = AudioEngine::new(
            rate as f32,
            Arc::new(Mutex::new(prepared.clone())),
            Arc::new(AtomicU64::new(1)),
        )
        .unwrap();
        let mut offline = OfflineRenderer::new(prepared, rate).unwrap();
        let frames = offline.score_frame_count();
        for _ in 0..frames {
            let (mix, stems) = offline.next_frame().unwrap();
            let actual = live.next_frame();
            assert!((mix.left - actual.left).abs() < 1e-6);
            assert!((mix.right - actual.right).abs() < 1e-6);
            assert_eq!(mix, stems[0]);
        }
        let mut tail = 0;
        while let Some((mix, _)) = offline.next_frame() {
            assert!(mix.left.is_finite() && mix.right.is_finite());
            tail += 1;
            assert!(tail < rate * 12);
        }
    }
}

#[test]
#[ignore = "writes audition files; set AHESS_GUITAR_DEMO_ROOT"]
fn clean_guitar_write_audition_project() {
    let root = std::path::PathBuf::from(std::env::var_os("AHESS_GUITAR_DEMO_ROOT").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let (project, part, score) = fixture();
    let directory = create_project(&root, &project).unwrap();
    score.save(&directory, &part, &project).unwrap();
    assert_eq!(load_project(&directory).unwrap().project, project);
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
    assert_eq!(result.file_count, 3);
    println!("Guitar audition: {}", directory.display());
}
