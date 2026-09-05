use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::transaction::{
    PROJECT_TRANSACTION_DIRECTORY, TRANSACTION_COMMITTING_FILE, TRANSACTION_CREATED_DIRECTORY,
    TRANSACTION_NEW_DIRECTORY, TRANSACTION_OLD_DIRECTORY,
};
use super::{
    add_voice, add_voice_at, add_voice_with_adjustment_at, create_project, delete_voice,
    duplicate_project, edit_part_rows, edit_voice, edit_voice_at, edit_voice_with_adjustment_at,
    list_projects, load_project, project_directory_name, restore_project_state, save_project,
    save_project_with_voice_convolution, CreateProjectError, DuplicateProjectError,
    FrequencyVariance, LoadProjectError, Project, ProjectEntry, Voice, VoiceConvolutionChange,
    VoiceType, VoiceVolumeAdjustment, PROJECT_CONFIG_FILE,
};
use crate::{
    acoustics::{AcousticScene, Point3Meters, RectangularRoom},
    part::{self, Part, PartName, PartRowEdit, PartScore, ScoreRowIndex, ScoreRowRange},
    pitch_system::{
        ExplicitPitchSystem, FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem,
        PitchSystem,
    },
    seed::Seed,
    tuning_system,
    voice_name::VoiceName,
};

const DEFAULT_TUNING_REFERENCE: &str = "tuning_system_id = \"western-twelve-tone\"\n";

#[test]
fn project_stores_the_initial_music_settings() {
    let project = Project::new("test", 4000, 100, Seed::new(19)).with_description("sketch");

    assert_eq!(project.name, "test");
    assert_eq!(project.beat_duration_millis.get(), 4000);
    assert_eq!(project.timing_variance, 100);
    assert_eq!(project.frequency_variance(), FrequencyVariance::default());
    assert!(project.mix_normalization_enabled());
    assert_eq!(project.seed, Seed::new(19));
    assert_eq!(project.description, "sketch");
    assert!(project.voices.is_empty());
    assert!(project.sequence().is_empty());
}

#[test]
fn structural_row_edits_commit_the_score_and_part_length_together() {
    let root = temp_root("structural-row-edit");
    let mut project = Project::new("rows", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
        1,
        "lead",
        VoiceType::Saw,
    )]);
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    let part = project.part(&PartName::new("intro")).unwrap().clone();
    let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
    score.save(&project_directory, &part, &project).unwrap();

    let (project, part, score) = edit_part_rows(
        &project_directory,
        &project,
        &part.name,
        &score,
        PartRowEdit::InsertAfter(ScoreRowIndex::new(1, 2).unwrap()),
    )
    .unwrap();

    assert_eq!(part.length, 3);
    assert_eq!(score.rows()[2], [String::new()]);
    assert_eq!(load_project(&project_directory).unwrap().project, project);
    assert_eq!(
        PartScore::load(&project_directory, &part, project.voices()).unwrap(),
        score
    );

    let (project, part, score) = edit_part_rows(
        &project_directory,
        &project,
        &part.name,
        &score,
        PartRowEdit::Delete(ScoreRowRange::new(0, 0, 3).unwrap()),
    )
    .unwrap();
    assert_eq!(part.length, 2);
    assert_eq!(score.rows(), &[vec!["D4".to_string()], vec![String::new()]]);
    assert_eq!(load_project(&project_directory).unwrap().project, project);
    assert!(!project_directory
        .join(PROJECT_TRANSACTION_DIRECTORY)
        .exists());
    assert!(!project_directory.join(".intro.csv.recovery").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_directory_name_is_filesystem_safe() {
    assert_eq!(
        project_directory_name("Arc Light Sketch!"),
        Some("arc-light-sketch".to_string())
    );
    assert_eq!(
        project_directory_name("../Score"),
        Some("score".to_string())
    );
    assert_eq!(project_directory_name("!!!"), None);
}

#[test]
fn config_file_contents_are_toml_compatible() {
    let project = Project::new("test \"score\"", 4000, 100, Seed::new(1234))
        .with_frequency_variance(FrequencyVariance::new(0.017).unwrap())
        .with_description("line one\nline two");

    assert_eq!(
        project.config_file_contents(),
        format!(
            "name = \"test \\\"score\\\"\"\ndescription = \"line one\\nline two\"\nbeat_duration_millis = 4000\ntiming_variance = 100\nfrequency_variance = 0.017\nmix_normalization = true\nseed = 1234\nnext_voice_id = 1\nsequence = []\n{DEFAULT_TUNING_REFERENCE}"
        )
    );
}

#[test]
fn mix_normalization_can_be_disabled_and_round_trips() {
    let root = temp_root("mix-normalization");
    let mut project = Project::new("test", 4000, 100, Seed::new(1234));
    project.set_mix_normalization_enabled(false);
    let project_directory = create_project(&root, &project).unwrap();

    let stored = fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();
    let loaded = load_project(&project_directory).unwrap().project;

    assert!(stored.contains("mix_normalization = false"));
    assert!(!loaded.mix_normalization_enabled());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_file_contents_store_voices_in_column_order() {
    let project = Project::new("test", 4000, 100, Seed::new(1234)).with_voices(vec![
        Voice::new(1, "lead", VoiceType::Saw),
        Voice::new(2, "bass", VoiceType::Sin),
    ]);

    assert_eq!(
        project.config_file_contents(),
        format!(
            "name = \"test\"\ndescription = \"\"\nbeat_duration_millis = 4000\ntiming_variance = 100\nfrequency_variance = 0\nmix_normalization = true\nseed = 1234\nnext_voice_id = 3\nsequence = []\n{DEFAULT_TUNING_REFERENCE}\n[[voices]]\nid = 1\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nid = 2\nname = \"bass\"\nvoice_type = \"sin\"\n"
        )
    );
}

#[test]
fn acoustic_scene_and_voice_positions_round_trip_through_project_config() {
    let root = temp_root("acoustic-scene-round-trip");
    let listener = Point3Meters::new(2.5, 2.0, 1.5).unwrap();
    let room = RectangularRoom::new(5.0, 4.0, 3.0, 0.25).unwrap();
    let voice_position = Point3Meters::new(1.0, 3.0, 1.0).unwrap();
    let mut project = Project::new("room", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
        1,
        "lead",
        VoiceType::Saw,
    )
    .with_position(voice_position)]);
    project
        .set_acoustic_scene(AcousticScene::new(listener, Some(room)).unwrap())
        .unwrap();

    let project_directory = create_project(&root, &project).unwrap();
    let config = fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();

    assert!(config.contains(
        "listener = { x = 2.5, y = 2.0, z = 1.5 }\nroom = { width = 5.0, length = 4.0, height = 3.0, reflection_gain = 0.25 }"
    ));
    assert!(config.contains("position = { x = 1.0, y = 3.0, z = 1.0 }"));
    assert_eq!(load_project(&project_directory).unwrap().project, project);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn centered_room_changes_preserve_voice_offsets_from_the_listener() {
    let mut project = Project::new("room", 800, 0, Seed::new(1)).with_voices(vec![
        Voice::new(1, "center", VoiceType::Sin),
        Voice::new(2, "right", VoiceType::Saw)
            .with_position(Point3Meters::new(1.0, 0.0, 0.0).unwrap()),
    ]);
    let room = RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap();

    project.set_centered_room(Some(room)).unwrap();

    assert_eq!(
        project.acoustic_scene().listener(),
        Point3Meters::new(4.0, 5.0, 1.5).unwrap()
    );
    assert_eq!(
        project.voices[0].position(),
        project.acoustic_scene().listener()
    );
    assert_eq!(
        project.voices[1].position(),
        Point3Meters::new(5.0, 5.0, 1.5).unwrap()
    );

    project.set_centered_room(None).unwrap();

    assert_eq!(project.acoustic_scene(), &AcousticScene::default());
    assert_eq!(project.voices[0].position(), Point3Meters::origin());
    assert_eq!(
        project.voices[1].position(),
        Point3Meters::new(1.0, 0.0, 0.0).unwrap()
    );
}

#[test]
fn rejected_room_resize_does_not_partially_move_the_scene_or_voices() {
    let mut project = Project::new("room", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
        1,
        "right",
        VoiceType::Saw,
    )
    .with_position(Point3Meters::new(3.0, 0.0, 0.0).unwrap())]);
    project
        .set_centered_room(Some(RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap()))
        .unwrap();
    let original = project.clone();

    let error = project
        .set_centered_room(Some(RectangularRoom::new(4.0, 10.0, 3.0, 0.25).unwrap()))
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("must be inside the acoustic room"));
    assert_eq!(project, original);
}

#[test]
fn project_loading_rejects_voice_positions_outside_the_room() {
    let root = temp_root("voice-outside-room");
    let project_directory = root.join("projects").join("test");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[acoustic_scene]\nlistener = { x = 2.5, y = 2.0, z = 1.5 }\nroom = { width = 5.0, length = 4.0, height = 3.0, reflection_gain = 0.25 }\n\n[[voices]]\nname = \"lead\"\nvoice_type = \"saw\"\nposition = { x = 6.0, y = 3.0, z = 1.0 }\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(error
        .to_string()
        .contains("voice position (6, 3, 1) must be inside the acoustic room"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_file_contents_store_part_metadata_without_redundant_filenames() {
    let project = Project::new("test", 4000, 100, Seed::new(1234)).with_parts(vec![
        Part::new("intro", 8).with_subdivision_pattern(Some("4, 3, 3".parse().unwrap())),
        Part::new("verse", 16),
    ]);

    assert_eq!(
        project.config_file_contents(),
        format!(
            "name = \"test\"\ndescription = \"\"\nbeat_duration_millis = 4000\ntiming_variance = 100\nfrequency_variance = 0\nmix_normalization = true\nseed = 1234\nnext_voice_id = 1\nsequence = [\"intro\", \"verse\"]\n{DEFAULT_TUNING_REFERENCE}\n[[parts]]\nname = \"intro\"\nlength = 8\nsubdivision_pattern = [4, 3, 3]\n\n[[parts]]\nname = \"verse\"\nlength = 16\n"
        )
    );
}

#[test]
fn subdivision_levels_round_trip_through_project_config() {
    let root = temp_root("subdivision-pattern-round-trip");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 8);
    project.parts[0] = project.parts[0]
        .clone()
        .with_subdivision_pattern(Some("4, 3, 3".parse().unwrap()))
        .with_major_subdivision(Some("16".parse().unwrap()));
    save_project(&project_directory, &project).unwrap();

    assert_eq!(load_project(&project_directory).unwrap().project, project);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_file_contents_preserve_repeated_part_occurrences() {
    let project = Project::new("test", 4000, 100, Seed::new(1234))
        .with_parts(vec![Part::new("part-a", 8), Part::new("part-b", 16)])
        .with_sequence(vec!["part-a".into(), "part-b".into(), "part-b".into()]);

    assert!(project
        .config_file_contents()
        .contains("sequence = [\"part-a\", \"part-b\", \"part-b\"]\n"));
}

#[test]
fn arrangement_occurrences_include_repeated_parts_and_global_beat_spans() {
    let project = Project::new("test", 4000, 100, Seed::new(1234))
        .with_parts(vec![Part::new("intro", 8), Part::new("verse", 16)])
        .with_sequence(vec!["intro".into(), "verse".into(), "verse".into()]);

    let occurrences = project.arrangement_occurrences();

    assert_eq!(occurrences.len(), 3);
    assert_eq!(occurrences[0].index(), 0);
    assert_eq!(occurrences[0].part_name().as_str(), "intro");
    assert_eq!(
        (occurrences[0].first_beat(), occurrences[0].last_beat()),
        (1, 8)
    );
    assert_eq!(occurrences[1].index(), 1);
    assert_eq!(
        (occurrences[1].first_beat(), occurrences[1].last_beat()),
        (9, 24)
    );
    assert_eq!(occurrences[2].index(), 2);
    assert_eq!(occurrences[2].part_name().as_str(), "verse");
    assert_eq!(
        (occurrences[2].first_beat(), occurrences[2].last_beat()),
        (25, 40)
    );
}

#[test]
fn voices_are_found_by_name() {
    let project = Project::new("test", 4000, 100, Seed::new(1234)).with_voices(vec![
        Voice::new(1, "lead", VoiceType::Saw),
        Voice::new(2, "bass", VoiceType::Sin),
        Voice::new(3, "harmony", VoiceType::Saw),
    ]);

    assert_eq!(
        project.voice(&VoiceName::new("BASS")),
        Some(&Voice::new(2, "bass", VoiceType::Sin))
    );
    assert_eq!(project.voice(&VoiceName::new("missing")), None);
}

#[test]
fn create_project_writes_config_under_projects_directory() {
    let root = temp_root("writes-config");
    let project = Project::new("Arc Light Sketch", 4000, 100, Seed::new(1234))
        .with_description("first generated sketch");

    let project_directory = create_project(&root, &project).unwrap();

    assert_eq!(
        project_directory,
        root.join("projects").join("arc-light-sketch")
    );
    assert_eq!(
        fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
        project.config_file_contents()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn create_project_rejects_existing_project_directory() {
    let root = temp_root("existing-project");
    let project = Project::new("Arc Light Sketch", 4000, 100, Seed::new(1234));

    create_project(&root, &project).unwrap();
    let error = create_project(&root, &project).unwrap_err();

    assert!(matches!(error, CreateProjectError::ProjectAlreadyExists(_)));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_project_copies_project_metadata_and_scores_under_a_new_name() {
    let root = temp_root("duplicate-project");
    let mut project = Project::new("Original", 4000, 100, Seed::new(1234))
        .with_description("first version")
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let source_directory = create_project(&root, &project).unwrap();
    add_test_part(&source_directory, &mut project, "intro", 2);
    let score = PartScore::from_rows(vec![vec!["A4".to_string()], vec![String::new()]]);
    score
        .save(&source_directory, &project.parts[0], &project)
        .unwrap();
    let source = load_project(&source_directory).unwrap();

    let duplicated = duplicate_project(&root, &source, "  Original variation  ").unwrap();

    let mut expected_project = project.clone();
    expected_project.name = "Original variation".to_string();
    assert_eq!(duplicated.project, expected_project);
    assert_eq!(
        duplicated.project_directory,
        root.join("projects").join("original-variation")
    );
    assert_eq!(
        PartScore::load(
            &duplicated.project_directory,
            &duplicated.project.parts[0],
            duplicated.project.voices()
        )
        .unwrap(),
        score
    );
    assert_eq!(
        load_project(&duplicated.project_directory).unwrap(),
        duplicated
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_project_does_not_overwrite_an_existing_project() {
    let root = temp_root("duplicate-project-existing");
    let source_directory =
        create_project(&root, &Project::new("Original", 800, 0, Seed::new(1))).unwrap();
    let source = load_project(source_directory).unwrap();
    let existing = Project::new("Existing", 4000, 10, Seed::new(2));
    let existing_directory = create_project(&root, &existing).unwrap();

    let error = duplicate_project(&root, &source, "Existing").unwrap_err();

    assert!(matches!(
        error,
        DuplicateProjectError::Create(CreateProjectError::ProjectAlreadyExists(_))
    ));
    assert_eq!(load_project(existing_directory).unwrap().project, existing);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_project_reports_an_invalid_part_filename_without_creating_a_copy() {
    let root = temp_root("duplicate-project-invalid-part");
    let source = ProjectEntry {
        project: Project::new("Original", 800, 0, Seed::new(1))
            .with_parts(vec![Part::new("!!!", 2)]),
        project_directory: root.join("source"),
    };

    let error = duplicate_project(&root, &source, "Copy").unwrap_err();

    assert!(matches!(
        error,
        DuplicateProjectError::InvalidPartName { name, .. } if name == "!!!"
    ));
    assert!(!root.join("projects").join("copy").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_project_removes_an_incomplete_copy_when_a_score_cannot_be_copied() {
    let root = temp_root("duplicate-project-rollback");
    let mut project = Project::new("Original", 800, 0, Seed::new(1));
    let source_directory = create_project(&root, &project).unwrap();
    add_test_part(&source_directory, &mut project, "intro", 2);
    let source = load_project(&source_directory).unwrap();
    fs::remove_file(source_directory.join("intro.csv")).unwrap();

    let error = duplicate_project(&root, &source, "Copy").unwrap_err();

    assert!(matches!(
        error,
        DuplicateProjectError::CopyPart {
            cleanup_error: None,
            ..
        }
    ));
    assert!(!root.join("projects").join("copy").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_reads_config_from_project_directory() {
    let root = temp_root("load-project");
    let project = Project::new("test \"score\"", 4000, 100, Seed::new(1234))
        .with_description("line one\nline two");
    let project_directory = create_project(&root, &project).unwrap();

    let loaded_project = load_project(&project_directory).unwrap();

    assert_eq!(loaded_project.project, project);
    assert!(loaded_project.project.voices.is_empty());
    assert_eq!(loaded_project.project_directory, project_directory);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_pitch_systems_round_trip_through_config() {
    let root = temp_root("pitch-system-round-trip");
    let periodic = PitchSystem::periodic(
        PeriodicPitchSystem::new(
            "slendro sketch",
            FrequencyHz::new(25.0).unwrap(),
            Interval::ratio(2, 1).unwrap(),
            vec![
                Interval::ratio(1, 1).unwrap(),
                Interval::ratio(8, 7).unwrap(),
                Interval::ratio(21, 16).unwrap(),
                Interval::ratio(32, 21).unwrap(),
                Interval::ratio(7, 4).unwrap(),
            ],
            PeriodicNotation::radler_digits(10).unwrap(),
        )
        .unwrap(),
    );
    let periodic_project =
        Project::new("periodic", 800, 0, Seed::new(1)).with_pitch_system(periodic);
    let periodic_directory = create_project(&root, &periodic_project).unwrap();

    assert_eq!(
        load_project(&periodic_directory).unwrap().project,
        periodic_project
    );
    let periodic_config = fs::read_to_string(periodic_directory.join(PROJECT_CONFIG_FILE)).unwrap();
    assert!(periodic_config.contains("fundamental_hz = 25"));
    assert!(periodic_config.contains("degrees = [\"1/1\", \"8/7\""));
    assert!(periodic_config.contains("kind = \"radler_digits\""));

    let explicit = PitchSystem::explicit(
        ExplicitPitchSystem::new(
            "embers",
            BTreeMap::from([
                ("ember".to_string(), FrequencyHz::new(197.3).unwrap()),
                ("⟟".to_string(), FrequencyHz::new(316.4).unwrap()),
            ]),
        )
        .unwrap(),
    );
    let explicit_project =
        Project::new("explicit", 800, 0, Seed::new(2)).with_pitch_system(explicit);
    let explicit_directory = create_project(&root, &explicit_project).unwrap();

    assert_eq!(
        load_project(&explicit_directory).unwrap().project,
        explicit_project
    );
    let explicit_config = fs::read_to_string(explicit_directory.join(PROJECT_CONFIG_FILE)).unwrap();
    assert!(explicit_config.contains("[pitch_system.pitches]"));
    assert!(explicit_config.contains("\"⟟\" = 316.4"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn projects_store_and_resolve_reusable_tuning_system_references() {
    let root = temp_root("reusable-tuning-reference");
    let tuning = tuning_system::create_tuning_system(
        &root,
        PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
            )
            .unwrap(),
        ),
    )
    .unwrap();
    let project = Project::new("piece", 800, 0, Seed::new(1)).with_tuning_system(&tuning);
    let directory = create_project(&root, &project).unwrap();

    let config = fs::read_to_string(directory.join(PROJECT_CONFIG_FILE)).unwrap();
    assert!(config.contains("tuning_system_id = \"embers\""));
    assert!(!config.contains("[pitch_system]"));
    assert_eq!(load_project(&directory).unwrap().project, project);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_a_missing_tuning_system_reference() {
    let root = temp_root("missing-tuning-reference");
    let project_directory = root.join("projects").join("piece");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"piece\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\ntuning_system_id = \"missing-system\"\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(error
        .to_string()
        .contains("references missing tuning system \"missing-system\""));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_projects_load_with_western_tuning_and_save_its_library_reference() {
    let root = temp_root("legacy-pitch-system");
    let project_directory = root.join("projects").join("legacy");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"legacy\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n",
    )
    .unwrap();

    let project = load_project(&project_directory).unwrap().project;

    assert_eq!(project.beat_duration_millis.get(), 17);
    assert!(
        (project
            .pitch_system()
            .resolve_cell("A4")
            .unwrap()
            .unwrap()
            .as_hz()
            - 440.0)
            .abs()
            < 1e-10
    );
    assert_eq!(project.frequency_variance(), FrequencyVariance::default());
    assert!(project.mix_normalization_enabled());
    save_project(&project_directory, &project).unwrap();
    let saved_config = fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();
    assert!(saved_config.contains("beat_duration_millis = 17"));
    assert!(!saved_config.contains("beat_length ="));
    assert!(saved_config.contains("tuning_system_id = \"western-twelve-tone\""));
    assert!(saved_config.contains("mix_normalization = true"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_frequency_variance_outside_the_fractional_range() {
    let root = temp_root("invalid-frequency-variance");
    let project_directory = root.join("projects").join("test");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nfrequency_variance = 1.0\nseed = 1\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
    assert!(error
        .to_string()
        .contains("frequency variance must be a decimal from 0 up to but not including 1"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_invalid_pitch_system_data() {
    let root = temp_root("invalid-pitch-system");
    let project_directory = root.join("projects").join("test");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[pitch_system]\nkind = \"periodic\"\nname = \"broken\"\nfundamental_hz = 0.0\nperiod = \"2/1\"\ndegrees = [\"1/1\"]\n\n[pitch_system.notation]\nkind = \"radler_digits\"\nplace_value = 10\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
    assert!(error.to_string().contains("frequency must be a positive"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_projects_default_the_sequence_to_each_part_once() {
    let root = temp_root("legacy-part-sequence");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    add_test_part(&project_directory, &mut project, "verse", 2);
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let legacy_config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("sequence = []\n", "");
    fs::write(&config_path, legacy_config).unwrap();

    let loaded = load_project(&project_directory).unwrap().project;

    assert_eq!(
        loaded
            .sequence()
            .iter()
            .map(PartName::as_str)
            .collect::<Vec<_>>(),
        vec!["intro", "verse"]
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_empty_sequence_remains_empty() {
    let root = temp_root("empty-part-sequence");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);

    let loaded = load_project(&project_directory).unwrap().project;

    assert!(loaded.sequence().is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_sequence_references_to_missing_parts() {
    let root = temp_root("missing-sequence-part");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let invalid_config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("sequence = []", "sequence = [\"missing\"]");
    fs::write(&config_path, invalid_config).unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidSequence { .. }));
    assert!(error
        .to_string()
        .contains("sequence references missing part \"missing\""));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sequence_references_use_the_part_name_casing() {
    let root = temp_root("sequence-name-casing");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "Intro", 2);
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("sequence = []", "sequence = [\"INTRO\", \"intro\"]");
    fs::write(&config_path, config).unwrap();

    let loaded = load_project(&project_directory).unwrap().project;

    assert_eq!(
        loaded
            .sequence()
            .iter()
            .map(PartName::as_str)
            .collect::<Vec<_>>(),
        vec!["Intro", "Intro"]
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_voice_names_that_cannot_be_identifiers() {
    let root = temp_root("invalid-voice-identifiers");
    let project_directory = root.join("projects").join("test");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[[voices]]\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nname = \"LEAD\"\nvoice_type = \"sin\"\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
    assert!(error
        .to_string()
        .contains("voice name \"LEAD\" is duplicated"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_voice_configs_receive_stable_ids_when_loaded() {
    let root = temp_root("legacy-voice-ids");
    let project_directory = root.join("projects").join("test");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[[voices]]\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nname = \"bass\"\nvoice_type = \"sin\"\n",
    )
    .unwrap();

    let project = load_project(&project_directory).unwrap().project;

    assert_eq!(project.voices[0].id().value(), 1);
    assert_eq!(project.voices[1].id().value(), 2);
    assert_eq!(project.next_voice_id, 3);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn voice_changes_update_every_part_and_preserve_cells_by_voice_id() {
    let root = temp_root("voice-columns");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    add_test_part(&project_directory, &mut project, "verse", 2);

    project = add_voice(&project_directory, &project, " lead ", VoiceType::Saw).unwrap();
    assert_eq!(
        fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
        "lead\n\"\"\n\"\"\n"
    );
    assert_eq!(project.voices[0].id().value(), 1);

    fs::write(project_directory.join("intro.csv"), "lead\nC4\nD4\n").unwrap();
    fs::write(project_directory.join("verse.csv"), "lead\n\"E,4\"\nF4\n").unwrap();

    project = add_voice(&project_directory, &project, "bass", VoiceType::Sin).unwrap();
    assert_eq!(
        fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
        "lead,bass\nC4,\nD4,\n"
    );
    assert_eq!(
        fs::read_to_string(project_directory.join("verse.csv")).unwrap(),
        "lead,bass\n\"E,4\",\nF4,\n"
    );

    project = edit_voice(
        &project_directory,
        &project,
        &VoiceName::new("LEAD"),
        "melody",
        VoiceType::Sin,
    )
    .unwrap();
    assert_eq!(project.voices[0].id().value(), 1);
    assert_eq!(
        fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
        "melody,bass\nC4,\nD4,\n"
    );

    project = delete_voice(&project_directory, &project, &VoiceName::new("bass")).unwrap();
    assert_eq!(
        fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
        "melody\nC4\nD4\n"
    );
    assert_eq!(
        fs::read_to_string(project_directory.join("verse.csv")).unwrap(),
        "melody\n\"E,4\"\nF4\n"
    );

    project = add_voice(&project_directory, &project, "harmony", VoiceType::Saw).unwrap();
    assert_eq!(project.voices[1].id().value(), 3);
    assert_eq!(
        fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
        "melody,harmony\nC4,\nD4,\n"
    );
    assert_eq!(load_project(&project_directory).unwrap().project, project);
    assert!(!project_directory
        .join(PROJECT_TRANSACTION_DIRECTORY)
        .exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn voice_positions_can_be_added_edited_and_reloaded() {
    let root = temp_root("voice-position-changes");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    project
        .set_centered_room(Some(RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap()))
        .unwrap();
    let project_directory = create_project(&root, &project).unwrap();
    let first_position = Point3Meters::new(2.0, 4.0, 1.0).unwrap();

    project = add_voice_at(
        &project_directory,
        &project,
        "lead",
        VoiceType::Saw,
        first_position,
    )
    .unwrap();
    assert_eq!(project.voices[0].position(), first_position);

    let edited_position = Point3Meters::new(6.0, 7.5, 2.0).unwrap();
    project = edit_voice_at(
        &project_directory,
        &project,
        &VoiceName::new("lead"),
        "lead",
        VoiceType::Saw,
        edited_position,
    )
    .unwrap();

    assert_eq!(project.voices[0].position(), edited_position);
    assert_eq!(load_project(&project_directory).unwrap().project, project);
    assert!(
        fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE))
            .unwrap()
            .contains("position = { x = 6.0, y = 7.5, z = 2.0 }")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn voice_volume_adjustments_can_be_added_edited_and_reloaded() {
    let root = temp_root("voice-volume-adjustments");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let louder = VoiceVolumeAdjustment::new(1.5).unwrap();

    let project = add_voice_with_adjustment_at(
        &project_directory,
        &project,
        "lead",
        VoiceType::Saw,
        Point3Meters::origin(),
        Some(louder),
    )
    .unwrap();
    assert_eq!(project.voices[0].volume_adjustment(), Some(louder));
    assert!(
        fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE))
            .unwrap()
            .contains("volume_adjustment = 1.5")
    );

    let project = edit_voice_with_adjustment_at(
        &project_directory,
        &project,
        &VoiceName::new("lead"),
        "lead",
        VoiceType::Saw,
        Point3Meters::origin(),
        None,
    )
    .unwrap();
    assert_eq!(project.voices[0].volume_adjustment(), None);
    assert!(
        !fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE))
            .unwrap()
            .contains("volume_adjustment")
    );
    assert_eq!(load_project(&project_directory).unwrap().project, project);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_config_rejects_non_positive_voice_volume_adjustments() {
    let root = temp_root("invalid-voice-volume-adjustment");
    let project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
        1,
        "lead",
        VoiceType::Saw,
    )]);
    let project_directory = create_project(&root, &project).unwrap();
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let invalid_config = fs::read_to_string(&config_path).unwrap().replace(
        "voice_type = \"saw\"",
        "voice_type = \"saw\"\nvolume_adjustment = 0.0",
    );
    fs::write(&config_path, invalid_config).unwrap();

    let error = load_project(&project_directory).unwrap_err();
    assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
    assert!(error
        .to_string()
        .contains("voice volume adjustment must be a finite decimal greater than zero"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn voice_position_changes_outside_the_room_leave_files_untouched() {
    let root = temp_root("invalid-voice-position-change");
    let mut project = Project::new("test", 800, 0, Seed::new(1));
    project
        .set_centered_room(Some(RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap()))
        .unwrap();
    let project_directory = create_project(&root, &project).unwrap();
    project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    let saved_config = fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();

    let error = edit_voice_at(
        &project_directory,
        &project,
        &VoiceName::new("lead"),
        "lead",
        VoiceType::Saw,
        Point3Meters::new(-1.0, 5.0, 1.5).unwrap(),
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("must be inside the acoustic room"));
    assert_eq!(
        fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
        saved_config
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_voice_changes_leave_project_files_untouched() {
    let root = temp_root("invalid-voice-change");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();

    assert!(add_voice(&project_directory, &project, " ", VoiceType::Saw).is_err());
    let project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    let saved_config = fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();

    assert!(add_voice(&project_directory, &project, "LEAD", VoiceType::Sin).is_err());
    assert!(edit_voice(
        &project_directory,
        &project,
        &VoiceName::new("lead"),
        " ",
        VoiceType::Sin,
    )
    .is_err());
    assert!(delete_voice(&project_directory, &project, &VoiceName::new("missing"),).is_err());
    assert_eq!(
        fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
        saved_config
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_part_files_with_the_wrong_voice_schema() {
    let root = temp_root("invalid-part-schema");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let mut project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    fs::write(project_directory.join("intro.csv"), "wrong\nC4\nD4\n").unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidPart(_)));
    assert!(error.to_string().contains("voice headers do not match"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rejects_beat_rows_with_the_wrong_column_count() {
    let root = temp_root("invalid-beat-columns");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    let mut project = add_voice(&project_directory, &project, "bass", VoiceType::Sin).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    fs::write(
        project_directory.join("intro.csv"),
        "lead,bass\nC4\nD4,D2\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidPart(_)));
    assert!(error
        .to_string()
        .contains("beat row 1 has 1 columns; expected 2"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rolls_back_an_interrupted_multi_file_commit() {
    let root = temp_root("recover-voice-transaction");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let mut project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    fs::write(project_directory.join("intro.csv"), "lead\nC4\nD4\n").unwrap();
    let original_config = fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();
    let original_part = fs::read(project_directory.join("intro.csv")).unwrap();

    let transaction_directory = project_directory.join(PROJECT_TRANSACTION_DIRECTORY);
    let old_directory = transaction_directory.join(TRANSACTION_OLD_DIRECTORY);
    fs::create_dir_all(&old_directory).unwrap();
    fs::create_dir(transaction_directory.join(TRANSACTION_NEW_DIRECTORY)).unwrap();
    fs::write(old_directory.join(PROJECT_CONFIG_FILE), &original_config).unwrap();
    fs::write(old_directory.join("intro.csv"), &original_part).unwrap();
    fs::write(transaction_directory.join(TRANSACTION_COMMITTING_FILE), "").unwrap();
    fs::write(project_directory.join(PROJECT_CONFIG_FILE), "invalid").unwrap();
    fs::write(project_directory.join("intro.csv"), "invalid").unwrap();

    let recovered = load_project(&project_directory).unwrap().project;

    assert_eq!(recovered, project);
    assert_eq!(
        fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
        original_config
    );
    assert_eq!(
        fs::read(project_directory.join("intro.csv")).unwrap(),
        original_part
    );
    assert!(!transaction_directory.exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn load_project_rolls_back_created_and_deleted_files() {
    let root = temp_root("recover-file-set-transaction");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let mut project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    add_test_part(&project_directory, &mut project, "intro", 2);
    let intro_path = project_directory.join("intro.csv");
    fs::write(&intro_path, "lead\nC4\nD4\n").unwrap();
    let original_part = fs::read(&intro_path).unwrap();

    let transaction_directory = project_directory.join(PROJECT_TRANSACTION_DIRECTORY);
    let old_directory = transaction_directory.join(TRANSACTION_OLD_DIRECTORY);
    let created_directory = transaction_directory.join(TRANSACTION_CREATED_DIRECTORY);
    fs::create_dir_all(&old_directory).unwrap();
    fs::create_dir(&created_directory).unwrap();
    fs::write(old_directory.join("intro.csv"), &original_part).unwrap();
    fs::write(created_directory.join("new-part.csv"), "").unwrap();
    fs::write(transaction_directory.join(TRANSACTION_COMMITTING_FILE), "").unwrap();
    fs::remove_file(&intro_path).unwrap();
    fs::write(project_directory.join("new-part.csv"), "lead\nA4\n").unwrap();

    let recovered = load_project(&project_directory).unwrap().project;

    assert_eq!(recovered, project);
    assert_eq!(fs::read(&intro_path).unwrap(), original_part);
    assert!(!project_directory.join("new-part.csv").exists());
    assert!(!transaction_directory.exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn history_restoration_writes_only_affected_score_files() {
    let root = temp_root("restore-affected-score");
    let project = Project::new("test", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let mut project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
    add_test_part(&project_directory, &mut project, "first", 1);
    add_test_part(&project_directory, &mut project, "second", 1);
    let first = project.part(&PartName::new("first")).unwrap().clone();
    let second = project.part(&PartName::new("second")).unwrap().clone();
    PartScore::from_rows(vec![vec!["C4".to_string()]])
        .save(&project_directory, &first, &project)
        .unwrap();
    PartScore::from_rows(vec![vec!["E4".to_string()]])
        .save(&project_directory, &second, &project)
        .unwrap();
    let restored_first = PartScore::from_rows(vec![vec!["D4".to_string()]]);

    restore_project_state(
        &project_directory,
        &project,
        &project,
        &[(&first.name, &restored_first, &restored_first)],
        false,
        std::slice::from_ref(&first.name),
    )
    .unwrap();

    assert_eq!(
        PartScore::load(&project_directory, &first, project.voices())
            .unwrap()
            .rows()[0][0],
        "D4"
    );
    assert_eq!(
        PartScore::load(&project_directory, &second, project.voices())
            .unwrap()
            .rows()[0][0],
        "E4"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_voice_type_round_trips_through_project_config() {
    let root = temp_root("voice-types-round-trip");
    let voices = VoiceType::ALL
        .into_iter()
        .enumerate()
        .map(|(index, voice_type)| Voice::new(index as u64 + 1, voice_type.label(), voice_type))
        .collect();
    let project = Project::new("voice types", 800, 0, Seed::new(1))
        .with_frequency_variance(FrequencyVariance::new(0.023).unwrap())
        .with_voices(voices);
    let project_directory = create_project(&root, &project).unwrap();

    assert_eq!(load_project(project_directory).unwrap().project, project);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn save_project_replaces_the_config_without_renaming_the_directory() {
    let root = temp_root("save-project");
    let original = Project::new("Original Name", 800, 10, Seed::new(1));
    let project_directory = create_project(&root, &original).unwrap();
    let updated = Project::new("Updated Name", 4000, 100, Seed::new(99))
        .with_description("updated description")
        .with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);

    save_project(&project_directory, &updated).unwrap();

    assert_eq!(
        project_directory,
        root.join("projects").join("original-name")
    );
    assert_eq!(load_project(&project_directory).unwrap().project, updated);
    assert!(!project_directory.join(".project.toml.pending").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn imported_impulse_response_is_project_owned_and_round_trips() {
    let root = temp_root("impulse-response-round-trip");
    let source_path = root.join("small hall.wav");
    fs::write(&source_path, mono_wav_bytes(48_000, &[0, 0, 1, 0])).unwrap();
    let project = Project::new("Room", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();

    let project = save_project_with_voice_convolution(
        &project_directory,
        project,
        VoiceConvolutionChange::Import(source_path),
    )
    .unwrap();

    let spec = project.voice_convolution().unwrap();
    assert_eq!(spec.file_name(), "small hall.wav");
    assert!(project_directory.join(spec.file()).is_file());
    assert_eq!(load_project(&project_directory).unwrap().project, project);
    assert!(
        fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE))
            .unwrap()
            .contains("[voice_convolution]")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_project_copies_its_impulse_response_asset() {
    let root = temp_root("duplicate-impulse-response");
    let source_path = root.join("room.wav");
    fs::write(&source_path, mono_wav_bytes(44_100, &[0, 0])).unwrap();
    let project = Project::new("Original", 800, 0, Seed::new(1));
    let project_directory = create_project(&root, &project).unwrap();
    let project = save_project_with_voice_convolution(
        &project_directory,
        project,
        VoiceConvolutionChange::Import(source_path),
    )
    .unwrap();
    let source = ProjectEntry {
        project,
        project_directory,
    };

    let duplicated = duplicate_project(&root, &source, "Copy").unwrap();

    let spec = duplicated.project.voice_convolution().unwrap();
    assert!(duplicated.project_directory.join(spec.file()).is_file());
    assert_eq!(
        load_project(&duplicated.project_directory).unwrap(),
        duplicated
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_config_rejects_impulse_response_paths_outside_project_assets() {
    let root = temp_root("invalid-impulse-response-path");
    let project_directory = root.join("projects/test");
    fs::create_dir_all(&project_directory).unwrap();
    fs::write(
        project_directory.join(PROJECT_CONFIG_FILE),
        "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[voice_convolution]\nfile = \"../room.wav\"\nname = \"room.wav\"\n",
    )
    .unwrap();

    let error = load_project(&project_directory).unwrap_err();

    assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
    assert!(error
        .to_string()
        .contains("must be stored under assets/impulse-responses"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn list_projects_returns_projects_sorted_by_name() {
    let root = temp_root("list-projects");
    create_project(
        &root,
        &Project::new("Zinc", 4000, 100, Seed::new(1)).with_description("last"),
    )
    .unwrap();
    create_project(
        &root,
        &Project::new("Arc", 4000, 100, Seed::new(2)).with_description("first"),
    )
    .unwrap();

    let projects = list_projects(&root).unwrap();

    assert_eq!(
        projects
            .iter()
            .map(|entry| entry.project.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Arc", "Zinc"]
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn list_projects_allows_a_missing_projects_directory() {
    let root = temp_root("missing-projects-directory");
    fs::remove_dir_all(root.join("projects")).unwrap_or(());

    assert!(list_projects(&root).unwrap().is_empty());

    fs::remove_dir_all(root).unwrap();
}

fn add_test_part(
    project_directory: &std::path::Path,
    project: &mut Project,
    name: &str,
    length: u32,
) {
    let created = part::create_part_file(
        project_directory,
        &project.parts,
        project.voices(),
        name,
        length,
    )
    .unwrap();
    project.add_part(created.commit());
    save_project(project_directory, project).unwrap();
}

fn mono_wav_bytes(sample_rate: u32, data: &[u8]) -> Vec<u8> {
    let channels = 1_u16;
    let bits = 16_u16;
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
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ahess-{test_name}-{}-{unique}", std::process::id()));

    fs::create_dir_all(&root).unwrap();
    root
}
