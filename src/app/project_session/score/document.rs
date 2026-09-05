//! Shared score content, validation cache, autosave, and document events.

use crate::{
    part::{Part, PartRowEdit, PartRowEditError, PartScore, ScoreError, ScoreRowRange},
    project::Project,
    view::data_grid,
};
use gpui::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(750);
const AUTOSAVE_MAX_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app::project_session) enum SaveState {
    Idle,
    Saving,
    SavingRecovered,
    RecoverySaved,
    Saved,
}

pub struct ScoreDocument {
    project: Project,
    project_directory: PathBuf,
    part: Part,
    score: PartScore,
    parse_issue_cache: ParseIssueCache,
    dirty: bool,
    last_save_error: Option<String>,
    save_state: SaveState,
    pending_autosave_since: Option<Instant>,
    autosave_task: Option<Task<()>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app::project_session) struct ParseIssue {
    pub row: usize,
    pub column: usize,
    pub voice: String,
    pub message: String,
}

struct ParseIssueCache {
    issues: Vec<ParseIssue>,
    invalid_cells: data_grid::InvalidCells,
}

impl ParseIssueCache {
    fn collect(project: &Project, score: &PartScore) -> Self {
        let issues = score
            .rows()
            .iter()
            .enumerate()
            .flat_map(|(row, values)| {
                values
                    .iter()
                    .enumerate()
                    .filter_map(move |(column, value)| {
                        project
                            .pitch_system()
                            .resolve_strike(value)
                            .err()
                            .map(|source| ParseIssue {
                                row,
                                column,
                                voice: project
                                    .voices()
                                    .get(column)
                                    .map(|voice| voice.name.as_str().to_string())
                                    .unwrap_or_else(|| format!("column {}", column + 1)),
                                message: source.to_string(),
                            })
                    })
            })
            .collect::<Vec<_>>();
        let invalid_cells = issues
            .iter()
            .map(|issue| (issue.row, issue.column))
            .collect();
        Self {
            issues,
            invalid_cells,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DocumentEvent {
    CellChanged {
        source_editor: u64,
        edit: ScoreCellEdit,
    },
    Saved,
    RecoverySaved,
    SaveFailed,
    RowsCleared,
    StructureChanged {
        source_editor: u64,
        selected_rows: ScoreRowRange,
    },
    Reset,
    HistoryRestored {
        structure_changed: bool,
    },
    ProjectChanged,
    PartSettingsChanged,
}

impl EventEmitter<DocumentEvent> for ScoreDocument {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app::project_session) struct ScoreCellEdit {
    pub(in crate::app::project_session) row: usize,
    pub(in crate::app::project_session) column: usize,
    pub(in crate::app::project_session) before: String,
    pub(in crate::app::project_session) after: String,
}

impl ScoreCellEdit {
    pub(in crate::app::project_session) fn reversed(&self) -> Self {
        Self {
            row: self.row,
            column: self.column,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

impl ScoreDocument {
    pub fn new(project: Project, project_directory: PathBuf, part: Part, score: PartScore) -> Self {
        let parse_issue_cache = ParseIssueCache::collect(&project, &score);
        Self {
            project,
            project_directory,
            part,
            score,
            parse_issue_cache,
            dirty: false,
            last_save_error: None,
            save_state: SaveState::Idle,
            pending_autosave_since: None,
            autosave_task: None,
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn part(&self) -> &Part {
        &self.part
    }

    pub fn score(&self) -> &PartScore {
        &self.score
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn last_save_error(&self) -> Option<&str> {
        self.last_save_error.as_deref()
    }

    pub(in crate::app::project_session) fn save_state(&self) -> SaveState {
        self.save_state
    }

    pub(in crate::app::project_session) fn autosave_recovered_score(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.dirty = true;
        self.save_state = SaveState::SavingRecovered;
        self.schedule_autosave(cx);
        cx.notify();
    }

    pub fn parse_issues(&self) -> &[ParseIssue] {
        &self.parse_issue_cache.issues
    }

    pub(super) fn invalid_cells(&self) -> &data_grid::InvalidCells {
        &self.parse_issue_cache.invalid_cells
    }

    fn refresh_parse_issues(&mut self) {
        self.parse_issue_cache = ParseIssueCache::collect(&self.project, &self.score);
    }

    fn set_score(&mut self, score: PartScore) {
        self.score = score;
        self.refresh_parse_issues();
    }

    fn set_project(&mut self, project: Project) {
        self.project = project;
        self.refresh_parse_issues();
    }

    fn replace_content(&mut self, project: Project, part: Part, score: PartScore) {
        self.project = project;
        self.part = part;
        self.score = score;
        self.refresh_parse_issues();
    }

    pub fn update_cell(
        &mut self,
        source_editor: u64,
        row: usize,
        column: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let mut rows = self.score.rows().to_vec();
        let Some(cell) = rows.get_mut(row).and_then(|row| row.get_mut(column)) else {
            return;
        };
        if *cell == value {
            return;
        }

        let edit = ScoreCellEdit {
            row,
            column,
            before: cell.clone(),
            after: value,
        };
        *cell = edit.after.clone();
        self.set_score(PartScore::from_rows(rows));
        self.dirty = true;
        self.last_save_error = None;
        self.schedule_autosave(cx);
        cx.emit(DocumentEvent::CellChanged {
            source_editor,
            edit,
        });
        cx.notify();
    }

    pub fn clear_rows(
        &mut self,
        rows: ScoreRowRange,
        cx: &mut Context<Self>,
    ) -> Result<(), PartRowEditError> {
        let score = self
            .score
            .edited_rows(PartRowEdit::Clear(rows), self.project.voices().len())?;
        if score == self.score {
            return Ok(());
        }

        self.set_score(score);
        self.dirty = true;
        self.last_save_error = None;
        self.schedule_autosave(cx);
        cx.emit(DocumentEvent::RowsCleared);
        cx.notify();
        Ok(())
    }

    pub fn apply_saved_structure_change(
        &mut self,
        project: Project,
        part: Part,
        score: PartScore,
        source_editor: u64,
        selected_rows: ScoreRowRange,
        cx: &mut Context<Self>,
    ) {
        self.replace_content(project, part, score);
        self.dirty = false;
        self.last_save_error = None;
        self.save_state = SaveState::Saved;
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::StructureChanged {
            source_editor,
            selected_rows,
        });
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        self.autosave_task.take();
        self.pending_autosave_since = None;
        if let Err(error) =
            self.score
                .save_recovery(&self.project_directory, &self.part, self.project.voices())
        {
            return self.save_failed(error, cx);
        }

        match self
            .score
            .save(&self.project_directory, &self.part, &self.project)
        {
            Ok(()) => self.finish_save(cx),
            Err(error) => {
                self.save_state = SaveState::RecoverySaved;
                self.save_failed(error, cx)
            }
        }
    }

    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let pending_since = *self.pending_autosave_since.get_or_insert(now);
        let remaining =
            AUTOSAVE_MAX_DELAY.saturating_sub(now.saturating_duration_since(pending_since));
        let delay = AUTOSAVE_DEBOUNCE.min(remaining);
        if self.save_state != SaveState::SavingRecovered {
            self.save_state = SaveState::Saving;
        }

        self.autosave_task = Some(cx.spawn(
            async move |document: WeakEntity<ScoreDocument>, cx: &mut AsyncApp| {
                cx.background_executor().timer(delay).await;
                document
                    .update(cx, |document, cx| document.autosave(cx))
                    .ok();
            },
        ));
    }

    fn autosave(&mut self, cx: &mut Context<Self>) {
        self.pending_autosave_since = None;
        if let Err(error) =
            self.score
                .save_recovery(&self.project_directory, &self.part, self.project.voices())
        {
            let _ = self.save_failed(error, cx);
            return;
        }

        match self
            .score
            .save(&self.project_directory, &self.part, &self.project)
        {
            Ok(()) => {
                let _ = self.finish_save(cx);
            }
            Err(ScoreError::InvalidPitch { .. }) => {
                self.last_save_error = None;
                self.save_state = SaveState::RecoverySaved;
                cx.emit(DocumentEvent::RecoverySaved);
                cx.notify();
            }
            Err(error) => {
                let _ = self.save_failed(error, cx);
            }
        }
    }

    fn finish_save(&mut self, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        self.save_state = SaveState::Saved;
        match PartScore::clear_recovery(&self.project_directory, &self.part) {
            Ok(()) => {
                self.dirty = false;
                self.last_save_error = None;
                cx.emit(DocumentEvent::Saved);
                cx.notify();
                Ok(())
            }
            Err(error) => self.save_failed(error, cx),
        }
    }

    fn save_failed(&mut self, error: ScoreError, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        self.last_save_error = Some(error.to_string());
        cx.emit(DocumentEvent::SaveFailed);
        cx.notify();
        Err(error)
    }

    pub fn replace_project_and_score(
        &mut self,
        project: Project,
        part: Part,
        score: PartScore,
        cx: &mut Context<Self>,
    ) {
        self.replace_content(project, part, score);
        self.dirty = false;
        self.last_save_error = None;
        self.save_state = SaveState::Idle;
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::Reset);
        cx.notify();
    }

    pub fn restore_history_content(
        &mut self,
        project: Project,
        part: Part,
        score: PartScore,
        has_recovery: bool,
        cx: &mut Context<Self>,
    ) {
        let structure_changed = self.score.rows().len() != score.rows().len()
            || self
                .score
                .rows()
                .iter()
                .zip(score.rows())
                .any(|(current, restored)| current.len() != restored.len());
        self.replace_content(project, part, score);
        self.dirty = has_recovery;
        self.last_save_error = None;
        self.save_state = if has_recovery {
            SaveState::RecoverySaved
        } else {
            SaveState::Saved
        };
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::HistoryRestored { structure_changed });
        cx.notify();
    }

    pub fn project_settings_changed(&mut self, project: Project, cx: &mut Context<Self>) {
        self.set_project(project);
        self.last_save_error = None;
        cx.emit(DocumentEvent::ProjectChanged);
        cx.notify();
    }

    pub fn part_settings_changed(&mut self, project: Project, part: Part, cx: &mut Context<Self>) {
        self.project = project;
        self.part = part;
        self.refresh_parse_issues();
        self.last_save_error = None;
        cx.emit(DocumentEvent::PartSettingsChanged);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        part::{Part, PartScore, ScoreRowRange},
        pitch_system::{
            ExplicitPitchSystem, FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem,
            PitchSystem,
        },
        project::{Project, Voice, VoiceType},
        seed::Seed,
    };
    use gpui::{div, prelude::*, AppContext, Context, Entity, TestAppContext, Window};
    use std::{collections::BTreeMap, path::PathBuf};
    struct DocumentEventHost {
        document: Entity<ScoreDocument>,
        last_cell_edit: Option<ScoreCellEdit>,
    }

    impl DocumentEventHost {
        fn new(document: Entity<ScoreDocument>, cx: &mut Context<Self>) -> Self {
            cx.subscribe(&document, Self::on_document_event).detach();
            Self {
                document,
                last_cell_edit: None,
            }
        }

        fn on_document_event(
            &mut self,
            _: Entity<ScoreDocument>,
            event: &DocumentEvent,
            _: &mut Context<Self>,
        ) {
            if let DocumentEvent::CellChanged { edit, .. } = event {
                self.last_cell_edit = Some(edit.clone());
            }
        }
    }

    impl Render for DocumentEventHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn cell_changes_emit_the_exact_domain_edit(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec!["C4".to_string()]]);
        let (host, cx) = cx.add_window_view(|_, cx| {
            let document = cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part, score));
            DocumentEventHost::new(document, cx)
        });
        let document = cx.update(|_, cx| host.read(cx).document.clone());

        document.update(cx, |document, cx| {
            document.update_cell(7, 0, 0, "D4".to_string(), cx);
        });

        assert_eq!(
            cx.update(|_, cx| host.read(cx).last_cell_edit.clone()),
            Some(ScoreCellEdit {
                row: 0,
                column: 0,
                before: "C4".to_string(),
                after: "D4".to_string(),
            })
        );
    }

    #[test]
    fn reports_every_invalid_score_cell() {
        let project = Project::new("test project", 20_000, 32, Seed::new(12)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let part = Part::new("part-a", 2);
        let score = PartScore::from_rows(vec![
            vec!["not-a-note".to_string(), "C4".to_string()],
            vec!["128".to_string(), "H4".to_string()],
        ]);
        let document = ScoreDocument::new(project, PathBuf::new(), part, score);

        let issues = document.parse_issues();

        assert_eq!(issues.len(), 3);
        assert_eq!((issues[0].row, issues[0].column), (0, 0));
        assert_eq!(issues[0].voice, "lead");
        assert_eq!((issues[1].row, issues[1].column), (1, 0));
        assert_eq!((issues[2].row, issues[2].column), (1, 1));
        assert_eq!(issues[2].voice, "bass");
    }

    #[test]
    fn score_issues_use_the_projects_explicit_notation() {
        let pitch_system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
            )
            .unwrap(),
        );
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let part = Part::new("part-a", 2);
        let score = PartScore::from_rows(vec![vec!["ember".to_string()], vec!["C4".to_string()]]);
        let document = ScoreDocument::new(project, PathBuf::new(), part, score);

        let issues = document.parse_issues();

        assert_eq!(issues.len(), 1);
        assert_eq!((issues[0].row, issues[0].column), (1, 0));
        assert!(issues[0].message.contains("not defined in \"embers\""));
    }

    #[test]
    fn score_issues_accept_radler_note_duration_volume_strikes() {
        let pitch_system = PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "four tone",
                FrequencyHz::new(16.0).unwrap(),
                Interval::ratio(2, 1).unwrap(),
                vec![
                    Interval::ratio(1, 1).unwrap(),
                    Interval::ratio(5, 4).unwrap(),
                    Interval::ratio(3, 2).unwrap(),
                    Interval::ratio(15, 8).unwrap(),
                ],
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        );
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let part = Part::new("part-a", 1);
        let score = PartScore::from_rows(vec![vec!["310880".to_string()]]);
        let document = ScoreDocument::new(project, PathBuf::new(), part, score);

        assert!(document.parse_issues().is_empty());
    }

    #[gpui::test]
    fn cached_parse_issues_follow_document_mutations(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec!["H4".to_string()], vec![String::new()]]);
        let part_for_document = part.clone();
        let document =
            cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part_for_document, score));

        assert_eq!(cx.update(|cx| document.read(cx).parse_issues().len()), 1);
        assert!(cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });
        assert!(cx.update(|cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "H4".to_string(), cx);
        });
        assert_eq!(cx.update(|cx| document.read(cx).parse_issues().len()), 1);
        assert!(cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document
                .clear_rows(ScoreRowRange::new(0, 0, 2).unwrap(), cx)
                .unwrap();
        });
        assert!(cx.update(|cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "ember".to_string(), cx);
        });
        assert_eq!(cx.update(|cx| document.read(cx).parse_issues().len()), 1);
        assert!(cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        let pitch_system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
            )
            .unwrap(),
        );
        let explicit_project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part]);
        document.update(cx, |document, cx| {
            document.project_settings_changed(explicit_project, cx);
        });
        assert!(cx.update(|cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));
    }
}
