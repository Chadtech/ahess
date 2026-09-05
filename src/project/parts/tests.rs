//! Filesystem-level operation tests; these do not require a GPUI session.

use super::*;
use crate::{
    part::{PartScore, ScoreRowRange},
    project::{Voice, VoiceType},
    seed::Seed,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn arrangement_changes_persist_and_prevent_deleting_referenced_parts() {
    let root = temp_root("part-arrangement");
    let mut project = Project::new("test project", 800, 0, Seed::new(12));
    let project_directory = project::create_project(&root, &project).unwrap();
    let part = create_project_part(&project_directory, &mut project, "part-a", 4).unwrap();

    update_project_sequence(
        &project_directory,
        &mut project,
        vec![part.name.clone(), part.name.clone()],
    )
    .unwrap();

    assert_eq!(project.sequence().len(), 2);
    assert_eq!(
        project::load_project(&project_directory)
            .unwrap()
            .project
            .sequence(),
        project.sequence()
    );
    let error = delete_project_part(&project_directory, &mut project, &part.name).unwrap_err();
    let PartChangeError::PartInSequence {
        occurrence_count, ..
    } = error
    else {
        panic!("deleting an arranged part should report its occurrences");
    };
    assert_eq!(occurrence_count, 2);
    assert!(project_directory.join("part-a.csv").is_file());

    update_project_sequence(&project_directory, &mut project, Vec::new()).unwrap();
    delete_project_part(&project_directory, &mut project, &part.name).unwrap();

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicated_parts_copy_the_score_and_persist_project_metadata() {
    let root = temp_root("duplicate-project-part");
    let mut project = Project::new("test project", 800, 0, Seed::new(12))
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let project_directory = project::create_project(&root, &project).unwrap();
    let source = create_configured_project_part(
        &project_directory,
        &mut project,
        "intro",
        2,
        Some(SubdivisionPattern::new([4, 3, 3]).unwrap()),
    )
    .unwrap();
    let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
    score.save(&project_directory, &source, &project).unwrap();

    let duplicated = duplicate_project_part(
        &project_directory,
        &mut project,
        &source.name,
        "intro variation",
    )
    .unwrap();

    assert_eq!(duplicated.length, source.length);
    assert_eq!(
        duplicated.subdivision_pattern(),
        source.subdivision_pattern()
    );
    assert_eq!(
        PartScore::load(&project_directory, &duplicated, project.voices()).unwrap(),
        score
    );
    assert_eq!(
        project::load_project(&project_directory).unwrap().project,
        project
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn selected_parts_append_as_independent_variants_with_repetitions_preserved() {
    let root = temp_root("append-project-variants");
    let mut project = Project::new("test project", 800, 0, Seed::new(12))
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let project_directory = project::create_project(&root, &project).unwrap();
    let d1 = create_configured_project_part(
        &project_directory,
        &mut project,
        "d1",
        2,
        Some(SubdivisionPattern::new([2]).unwrap()),
    )
    .unwrap();
    let d2 = create_project_part(&project_directory, &mut project, "d2", 2).unwrap();
    let d3 = create_project_part(&project_directory, &mut project, "d3", 1).unwrap();
    let d1_score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
    let d2_score = PartScore::from_rows(vec![vec!["E4".to_string()], vec!["F4".to_string()]]);
    d1_score.save(&project_directory, &d1, &project).unwrap();
    d2_score.save(&project_directory, &d2, &project).unwrap();
    update_project_sequence(&project_directory, &mut project, vec![d3.name.clone()]).unwrap();

    let appended = append_project_variants(
        &project_directory,
        &mut project,
        &[
            d1.name.clone(),
            d2.name.clone(),
            d2.name.clone(),
            d3.name.clone(),
        ],
        "v1",
    )
    .unwrap();

    assert_eq!(
        appended.iter().map(PartName::as_str).collect::<Vec<_>>(),
        ["d1 v1", "d2 v1", "d2 v1", "d3 v1"]
    );
    assert_eq!(
        project
            .sequence()
            .iter()
            .map(PartName::as_str)
            .collect::<Vec<_>>(),
        ["d3", "d1 v1", "d2 v1", "d2 v1", "d3 v1"]
    );
    assert_eq!(
        project.parts().len(),
        6,
        "each distinct source is copied once"
    );
    let d1_variant = project.part(&PartName::new("d1 v1")).unwrap();
    let d2_variant = project.part(&PartName::new("d2 v1")).unwrap();
    assert_eq!(d1_variant.subdivision_pattern(), d1.subdivision_pattern());
    assert_eq!(
        PartScore::load(&project_directory, d1_variant, project.voices()).unwrap(),
        d1_score
    );
    assert_eq!(
        PartScore::load(&project_directory, d2_variant, project.voices()).unwrap(),
        d2_score
    );
    assert_eq!(
        project::load_project(&project_directory).unwrap().project,
        project
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn variant_creation_rolls_back_earlier_files_when_a_later_name_collides() {
    let root = temp_root("append-project-variants-rollback");
    let mut project = Project::new("test project", 800, 0, Seed::new(12));
    let project_directory = project::create_project(&root, &project).unwrap();
    let d1 = create_project_part(&project_directory, &mut project, "d1", 2).unwrap();
    let d2 = create_project_part(&project_directory, &mut project, "d2", 2).unwrap();
    create_project_part(&project_directory, &mut project, "d2 v1", 2).unwrap();
    let project_before = project.clone();

    let error = append_project_variants(
        &project_directory,
        &mut project,
        &[d1.name.clone(), d2.name.clone()],
        "v1",
    )
    .unwrap_err();

    let PartChangeError::CreateVariants { .. } = error else {
        panic!("a colliding variant name should fail variant file creation");
    };
    assert_eq!(project, project_before);
    let d1_variant_file = part::csv_file_name(&PartName::new("d1 v1")).unwrap();
    assert!(!project_directory.join(d1_variant_file).exists());
    assert_eq!(
        project::load_project(&project_directory).unwrap().project,
        project_before
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn combined_parts_concatenate_an_explicit_source_list_without_changing_the_arrangement() {
    let root = temp_root("combine-project-parts");
    let mut project = Project::new("test project", 800, 0, Seed::new(12))
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let project_directory = project::create_project(&root, &project).unwrap();
    let pattern = SubdivisionPattern::new([2]).unwrap();
    let intro = create_configured_project_part(
        &project_directory,
        &mut project,
        "intro",
        2,
        Some(pattern.clone()),
    )
    .unwrap();
    let verse = create_configured_project_part(
        &project_directory,
        &mut project,
        "verse",
        2,
        Some(pattern.clone()),
    )
    .unwrap();
    let outro = create_project_part(&project_directory, &mut project, "outro", 1).unwrap();
    PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]])
        .save(&project_directory, &intro, &project)
        .unwrap();
    PartScore::from_rows(vec![vec!["E4".to_string()], vec!["F4".to_string()]])
        .save(&project_directory, &verse, &project)
        .unwrap();
    update_project_sequence(
        &project_directory,
        &mut project,
        vec![
            intro.name.clone(),
            verse.name.clone(),
            verse.name.clone(),
            outro.name.clone(),
        ],
    )
    .unwrap();
    let sequence_before = project.sequence().to_vec();
    let sources = vec![intro.name.clone(), verse.name.clone(), verse.name.clone()];

    let combined = combine_project_parts(
        &project_directory,
        &mut project,
        &sources,
        "intro and verses",
    )
    .unwrap();

    assert_eq!(combined.length, 6);
    assert_eq!(
        combined.subdivision_pattern(),
        Some(&pattern),
        "a common subdivision pattern should be preserved"
    );
    assert_eq!(
        PartScore::load(&project_directory, &combined, project.voices())
            .unwrap()
            .rows(),
        [
            vec!["C4".to_string()],
            vec!["D4".to_string()],
            vec!["E4".to_string()],
            vec!["F4".to_string()],
            vec!["E4".to_string()],
            vec!["F4".to_string()],
        ]
    );
    assert_eq!(project.sequence(), sequence_before);
    assert!(project.part(&intro.name).is_some());
    assert!(project.part(&verse.name).is_some());
    assert_eq!(
        project::load_project(&project_directory).unwrap().project,
        project
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn combining_requires_at_least_two_sources() {
    let root = temp_root("combine-project-parts-invalid-sources");
    let mut project = Project::new("test project", 800, 0, Seed::new(12));
    let project_directory = project::create_project(&root, &project).unwrap();
    let intro = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
    update_project_sequence(&project_directory, &mut project, vec![intro.name.clone()]).unwrap();

    let error = combine_project_parts(
        &project_directory,
        &mut project,
        std::slice::from_ref(&intro.name),
        "not combined",
    )
    .err()
    .unwrap();

    let PartChangeError::CombineNeedsTwoParts = error else {
        panic!("combining one part should require another source");
    };
    assert!(project.part(&"not combined".into()).is_none());
    assert!(!project_directory.join("not-combined.csv").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn selected_rows_export_as_a_new_part_with_the_source_meter() {
    let root = temp_root("export-project-part-rows");
    let mut project = Project::new("test project", 800, 0, Seed::new(12))
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let project_directory = project::create_project(&root, &project).unwrap();
    let source = create_configured_project_part(
        &project_directory,
        &mut project,
        "theme",
        4,
        Some(SubdivisionPattern::new([2, 3]).unwrap()),
    )
    .unwrap();
    let score = PartScore::from_rows(vec![
        vec!["C4".to_string()],
        vec!["D4".to_string()],
        vec!["E4".to_string()],
        vec!["F4".to_string()],
    ]);
    score.save(&project_directory, &source, &project).unwrap();

    let exported = export_project_part_rows(
        &project_directory,
        &mut project,
        &source.name,
        &score,
        ScoreRowRange::new(1, 2, 4).unwrap(),
        "theme middle",
    )
    .unwrap();

    assert_eq!(exported.length, 2);
    assert_eq!(exported.subdivision_pattern(), source.subdivision_pattern());
    assert_eq!(
        PartScore::load(&project_directory, &exported, project.voices())
            .unwrap()
            .rows(),
        [vec!["D4".to_string()], vec!["E4".to_string()]]
    );
    assert_eq!(
        project::load_project(&project_directory).unwrap().project,
        project
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_selected_rows_do_not_leave_an_incomplete_export() {
    let root = temp_root("invalid-export-project-part-rows");
    let mut project = Project::new("test project", 800, 0, Seed::new(12))
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let project_directory = project::create_project(&root, &project).unwrap();
    let source = create_project_part(&project_directory, &mut project, "theme", 2).unwrap();
    let score = PartScore::from_rows(vec![vec!["not-a-note".to_string()], vec!["C4".to_string()]]);

    let error = export_project_part_rows(
        &project_directory,
        &mut project,
        &source.name,
        &score,
        ScoreRowRange::new(0, 0, 2).unwrap(),
        "broken excerpt",
    )
    .unwrap_err();

    let PartChangeError::ExportScore { .. } = error else {
        panic!("a failed score export should preserve its error kind");
    };
    assert_eq!(project.parts().len(), 1);
    assert!(!project_directory.join("broken-excerpt.csv").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn renamed_parts_keep_their_score_and_update_every_arrangement_occurrence() {
    let root = temp_root("rename-project-part");
    let mut project = Project::new("test project", 800, 0, Seed::new(12))
        .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
    let project_directory = project::create_project(&root, &project).unwrap();
    let intro = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
    let verse = create_project_part(&project_directory, &mut project, "verse", 2).unwrap();
    let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
    score.save(&project_directory, &intro, &project).unwrap();
    update_project_sequence(
        &project_directory,
        &mut project,
        vec![intro.name.clone(), verse.name.clone(), intro.name.clone()],
    )
    .unwrap();

    let renamed = rename_project_part(
        &project_directory,
        &mut project,
        &intro.name,
        "opening theme",
    )
    .unwrap();

    assert_eq!(renamed.name.as_str(), "opening theme");
    assert_eq!(
        project
            .sequence()
            .iter()
            .map(PartName::as_str)
            .collect::<Vec<_>>(),
        ["opening theme", "verse", "opening theme"]
    );
    assert!(!project_directory.join("intro.csv").exists());
    assert_eq!(
        PartScore::load(&project_directory, &renamed, project.voices()).unwrap(),
        score
    );
    assert_eq!(
        project::load_project(&project_directory).unwrap().project,
        project
    );

    let error =
        rename_project_part(&project_directory, &mut project, &renamed.name, "verse").unwrap_err();
    let PartChangeError::RenameFile(_) = error else {
        panic!("a conflicting part name should fail while renaming its file");
    };

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_arrangement_saves_restore_the_in_memory_sequence() {
    let part = Part::new("part-a", 4);
    let mut project =
        Project::new("test project", 800, 0, Seed::new(12)).with_parts(vec![part.clone()]);
    let original_sequence = project.sequence().to_vec();

    let error = update_project_sequence(
        Path::new("/a/project/directory/that/does/not/exist"),
        &mut project,
        vec![part.name.clone(), part.name],
    )
    .unwrap_err();

    let ArrangementChangeError::Save(_) = error else {
        panic!("saving to a missing directory should report a save error");
    };
    assert_eq!(project.sequence(), original_sequence);
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
