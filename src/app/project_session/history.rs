use std::sync::Arc;

use crate::{
    part::{PartName, PartScore, ScoreError},
    project::Project,
};

use super::score::ScoreCellEdit;

const MAX_CHANGES: usize = 100;

#[derive(Clone, Debug)]
pub(super) struct ProjectState {
    pub(super) project: Arc<Project>,
    scores: Vec<ScoreState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScoreState {
    part_name: PartName,
    content: ScoreContent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScoreContent {
    score: Arc<PartScore>,
    saved_score: Arc<PartScore>,
}

impl ProjectState {
    #[cfg(test)]
    pub(super) fn new_from_project(project: Arc<Project>) -> Self {
        Self::new(project, [])
    }

    pub(super) fn new(
        project: Arc<Project>,
        scores: impl IntoIterator<Item = (PartName, Arc<PartScore>, Arc<PartScore>)>,
    ) -> Self {
        Self {
            project,
            scores: scores
                .into_iter()
                .map(|(part_name, score, saved_score)| ScoreState {
                    part_name,
                    content: ScoreContent { score, saved_score },
                })
                .collect(),
        }
    }

    pub(super) fn score(&self, part_name: &PartName) -> Option<&Arc<PartScore>> {
        self.score_state(part_name)
            .map(|entry| &entry.content.score)
    }

    pub(super) fn saved_score(&self, part_name: &PartName) -> Option<&Arc<PartScore>> {
        self.score_state(part_name)
            .map(|entry| &entry.content.saved_score)
    }

    pub(super) fn scores(
        &self,
    ) -> impl Iterator<Item = (&PartName, &Arc<PartScore>, &Arc<PartScore>)> {
        self.scores.iter().map(|entry| {
            (
                &entry.part_name,
                &entry.content.score,
                &entry.content.saved_score,
            )
        })
    }

    fn score_state(&self, part_name: &PartName) -> Option<&ScoreState> {
        self.scores
            .iter()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(part_name))
    }

    fn replacing_score(&self, part_name: &PartName, content: ScoreContent) -> Result<Self, String> {
        let mut state = self.clone();
        state.replace_score(part_name, content)?;
        Ok(state)
    }

    fn replace_score(&mut self, part_name: &PartName, content: ScoreContent) -> Result<(), String> {
        let entry = self
            .scores
            .iter_mut()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(part_name))
            .ok_or_else(|| format!("history has no score for part {:?}", part_name.as_str()))?;
        entry.content = content;
        Ok(())
    }

    fn replacing_cell(
        &self,
        part_name: &PartName,
        edit: &ScoreCellEdit,
        saved_score: Arc<PartScore>,
    ) -> Result<Self, String> {
        let current = self
            .score_state(part_name)
            .ok_or_else(|| format!("history has no score for part {:?}", part_name.as_str()))?;
        let score = apply_score_cell_edit(&current.content.score, part_name, edit)?;
        self.replacing_score(
            part_name,
            ScoreContent {
                score: Arc::new(score),
                saved_score,
            },
        )
    }

    fn applying_project_change(
        &self,
        expected_project: &Project,
        target_project: Arc<Project>,
        target_scores: &[ScoreState],
    ) -> Result<Self, String> {
        if self.project.as_ref() != expected_project {
            return Err("project history no longer matches the current project".to_string());
        }

        let scores = target_project
            .parts()
            .iter()
            .map(|part| {
                let mut score = target_scores
                    .iter()
                    .find(|entry| entry.part_name.eq_ignore_ascii_case(&part.name))
                    .or_else(|| self.score_state(&part.name))
                    .cloned()
                    .ok_or_else(|| {
                        format!("history has no score for part {:?}", part.name.as_str())
                    })?;
                score.part_name = part.name.clone();
                Ok::<ScoreState, String>(score)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            project: target_project,
            scores,
        })
    }
}

impl PartialEq for ProjectState {
    fn eq(&self, other: &Self) -> bool {
        self.project == other.project
            && self.scores.len() == other.scores.len()
            && self.scores.iter().all(|entry| {
                other
                    .score_state(&entry.part_name)
                    .is_some_and(|other| other.content == entry.content)
            })
    }
}

impl Eq for ProjectState {}

#[derive(Clone, Debug)]
enum HistoryChange {
    ScoreCell {
        part_name: PartName,
        edit: ScoreCellEdit,
        before_saved_score: Arc<PartScore>,
        after_saved_score: Arc<PartScore>,
    },
    ScoreRows {
        part_name: PartName,
        before: ScoreContent,
        after: ScoreContent,
    },
    Project {
        before_project: Arc<Project>,
        after_project: Arc<Project>,
        before_scores: Vec<ScoreState>,
        after_scores: Vec<ScoreState>,
    },
}

pub(super) struct HistoryTarget {
    pub(super) state: ProjectState,
    pub(super) project_changed: bool,
    pub(super) affected_parts: Vec<PartName>,
}

impl HistoryChange {
    fn apply_backward(&self, state: &ProjectState) -> Result<ProjectState, String> {
        match self {
            Self::ScoreCell {
                part_name,
                edit,
                before_saved_score,
                ..
            } => state.replacing_cell(part_name, &edit.reversed(), before_saved_score.clone()),
            Self::ScoreRows {
                part_name,
                before,
                after,
            } => {
                Self::require_score(state, part_name, after)?;
                state.replacing_score(part_name, before.clone())
            }
            Self::Project {
                before_project,
                after_project,
                before_scores,
                ..
            } => {
                state.applying_project_change(after_project, before_project.clone(), before_scores)
            }
        }
    }

    fn apply_forward(&self, state: &ProjectState) -> Result<ProjectState, String> {
        match self {
            Self::ScoreCell {
                part_name,
                edit,
                after_saved_score,
                ..
            } => state.replacing_cell(part_name, edit, after_saved_score.clone()),
            Self::ScoreRows {
                part_name,
                before,
                after,
            } => {
                Self::require_score(state, part_name, before)?;
                state.replacing_score(part_name, after.clone())
            }
            Self::Project {
                before_project,
                after_project,
                after_scores,
                ..
            } => state.applying_project_change(before_project, after_project.clone(), after_scores),
        }
    }

    fn require_score(
        state: &ProjectState,
        part_name: &PartName,
        expected: &ScoreContent,
    ) -> Result<(), String> {
        let current = state
            .score_state(part_name)
            .ok_or_else(|| format!("history has no score for part {:?}", part_name.as_str()))?;
        if &current.content == expected {
            Ok(())
        } else {
            Err(format!(
                "score history no longer matches part {:?}",
                part_name.as_str()
            ))
        }
    }

    fn target(&self, state: &ProjectState, forward: bool) -> Result<HistoryTarget, String> {
        let state = if forward {
            self.apply_forward(state)?
        } else {
            self.apply_backward(state)?
        };
        let (project_changed, affected_parts) = match self {
            Self::ScoreCell { part_name, .. } | Self::ScoreRows { part_name, .. } => {
                (false, vec![part_name.clone()])
            }
            Self::Project {
                before_project,
                after_project,
                before_scores,
                after_scores,
            } => {
                let mut affected_parts = Vec::new();
                if score_file_schema_changed(before_project, after_project) {
                    for part in before_project.parts().iter().chain(after_project.parts()) {
                        push_unique_name(&mut affected_parts, &part.name);
                    }
                } else {
                    for score in before_scores.iter().chain(after_scores) {
                        push_unique_name(&mut affected_parts, &score.part_name);
                    }
                }
                (before_project != after_project, affected_parts)
            }
        };
        Ok(HistoryTarget {
            state,
            project_changed,
            affected_parts,
        })
    }
}

#[derive(Debug)]
pub(super) struct ProjectHistory {
    current: ProjectState,
    undo_changes: Vec<HistoryChange>,
    redo_changes: Vec<HistoryChange>,
    merge_with_current: bool,
}

impl ProjectHistory {
    pub(super) fn new(initial: ProjectState) -> Self {
        Self {
            current: initial,
            undo_changes: Vec::new(),
            redo_changes: Vec::new(),
            merge_with_current: false,
        }
    }

    pub(super) fn reset(&mut self, initial: ProjectState) {
        self.current = initial;
        self.undo_changes.clear();
        self.redo_changes.clear();
        self.merge_with_current = false;
    }

    pub(super) fn current(&self) -> &ProjectState {
        &self.current
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo_changes.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo_changes.is_empty()
    }

    pub(super) fn record_score_cell(
        &mut self,
        part_name: PartName,
        edit: ScoreCellEdit,
    ) -> Result<bool, String> {
        if edit.before == edit.after {
            return Ok(false);
        }
        let before_state = self
            .current
            .score_state(&part_name)
            .ok_or_else(|| format!("history has no score for part {:?}", part_name.as_str()))?;
        let before_saved_score = before_state.content.saved_score.clone();
        let after_score = apply_score_cell_edit(&before_state.content.score, &part_name, &edit)?;
        let after_score = Arc::new(after_score);
        let after_saved_score = self.saved_score_for(&part_name, &after_score)?;
        let after_content = ScoreContent {
            score: after_score,
            saved_score: after_saved_score.clone(),
        };

        self.redo_changes.clear();
        let merges = self.merge_with_current
            && self.undo_changes.last().is_some_and(|change| {
                matches!(
                    change,
                    HistoryChange::ScoreCell {
                        part_name: previous_part,
                        edit: previous_edit,
                        ..
                    } if previous_part.eq_ignore_ascii_case(&part_name)
                        && previous_edit.row == edit.row
                        && previous_edit.column == edit.column
                )
            });
        if merges {
            let HistoryChange::ScoreCell {
                edit: previous_edit,
                after_saved_score: previous_saved,
                ..
            } = self
                .undo_changes
                .last_mut()
                .expect("a matching cell change must exist")
            else {
                unreachable!("the matching history change must be a score cell")
            };
            previous_edit.after = edit.after;
            *previous_saved = after_saved_score;
            if previous_edit.before == previous_edit.after {
                self.undo_changes.pop();
                self.merge_with_current = false;
            }
        } else {
            self.push_undo(HistoryChange::ScoreCell {
                part_name: part_name.clone(),
                edit,
                before_saved_score,
                after_saved_score,
            });
            self.merge_with_current = true;
        }
        self.current.replace_score(&part_name, after_content)?;
        Ok(true)
    }

    pub(super) fn record_score_rows(
        &mut self,
        part_name: PartName,
        after_score: PartScore,
    ) -> Result<bool, String> {
        let before = self
            .current
            .score_state(&part_name)
            .ok_or_else(|| format!("history has no score for part {:?}", part_name.as_str()))?
            .content
            .clone();
        let after_score = Arc::new(after_score);
        let after = ScoreContent {
            saved_score: self.saved_score_for(&part_name, &after_score)?,
            score: after_score,
        };
        if before == after {
            return Ok(false);
        }

        self.redo_changes.clear();
        self.push_undo(HistoryChange::ScoreRows {
            part_name: part_name.clone(),
            before,
            after: after.clone(),
        });
        self.current.replace_score(&part_name, after)?;
        self.merge_with_current = false;
        Ok(true)
    }

    pub(super) fn record_project(&mut self, after: ProjectState) -> bool {
        let before_scores = changed_scores(&self.current, &after);
        let after_scores = changed_scores(&after, &self.current);
        if self.current.project == after.project
            && before_scores.is_empty()
            && after_scores.is_empty()
        {
            return false;
        }

        self.redo_changes.clear();
        self.push_undo(HistoryChange::Project {
            before_project: self.current.project.clone(),
            after_project: after.project.clone(),
            before_scores,
            after_scores,
        });
        self.current = after;
        self.merge_with_current = false;
        true
    }

    pub(super) fn undo_target(&self) -> Result<Option<HistoryTarget>, String> {
        self.undo_changes
            .last()
            .map(|change| change.target(&self.current, false))
            .transpose()
    }

    pub(super) fn redo_target(&self) -> Result<Option<HistoryTarget>, String> {
        self.redo_changes
            .last()
            .map(|change| change.target(&self.current, true))
            .transpose()
    }

    pub(super) fn commit_undo(&mut self, target: ProjectState) {
        let change = self
            .undo_changes
            .pop()
            .expect("undo must have a preceding change");
        self.redo_changes.push(change);
        self.current = target;
        self.merge_with_current = false;
    }

    pub(super) fn commit_redo(&mut self, target: ProjectState) {
        let change = self
            .redo_changes
            .pop()
            .expect("redo must have a following change");
        self.undo_changes.push(change);
        self.current = target;
        self.merge_with_current = false;
    }

    fn saved_score_for(
        &self,
        part_name: &PartName,
        score: &Arc<PartScore>,
    ) -> Result<Arc<PartScore>, String> {
        let part = self
            .current
            .project
            .part(part_name)
            .ok_or_else(|| format!("history has no part {:?}", part_name.as_str()))?;
        match score.resolved_strikes(part, &self.current.project) {
            Ok(_) => Ok(score.clone()),
            Err(ScoreError::InvalidPitch { .. }) => {
                let current = self.current.score_state(part_name).ok_or_else(|| {
                    format!("history has no score for part {:?}", part_name.as_str())
                })?;
                Ok(current.content.saved_score.clone())
            }
            Err(error) => Err(format!(
                "couldn't record score history for part {:?}: {error}",
                part_name.as_str()
            )),
        }
    }

    fn push_undo(&mut self, change: HistoryChange) {
        self.undo_changes.push(change);
        if self.undo_changes.len() > MAX_CHANGES {
            self.undo_changes.remove(0);
        }
    }
}

fn changed_scores(source: &ProjectState, comparison: &ProjectState) -> Vec<ScoreState> {
    source
        .scores
        .iter()
        .filter(|entry| {
            comparison
                .score_state(&entry.part_name)
                .is_none_or(|other| other.content != entry.content)
        })
        .cloned()
        .collect()
}

fn score_file_schema_changed(before: &Project, after: &Project) -> bool {
    before.voices().len() != after.voices().len()
        || before
            .voices()
            .iter()
            .zip(after.voices())
            .any(|(before, after)| before.id() != after.id() || before.name != after.name)
}

fn push_unique_name(names: &mut Vec<PartName>, name: &PartName) {
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(name))
    {
        names.push(name.clone());
    }
}

fn apply_score_cell_edit(
    score: &PartScore,
    part_name: &PartName,
    edit: &ScoreCellEdit,
) -> Result<PartScore, String> {
    let mut rows = score.rows().to_vec();
    let cell = rows
        .get_mut(edit.row)
        .and_then(|row| row.get_mut(edit.column))
        .ok_or_else(|| {
            format!(
                "history cell {}:{} is outside part {:?}",
                edit.row + 1,
                edit.column + 1,
                part_name.as_str()
            )
        })?;
    if cell != &edit.before {
        return Err(format!(
            "history expected cell {}:{} in part {:?} to contain {:?}, but found {cell:?}",
            edit.row + 1,
            edit.column + 1,
            part_name.as_str(),
            edit.before
        ));
    }
    *cell = edit.after.clone();
    Ok(PartScore::from_rows(rows))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{HistoryChange, ProjectHistory, ProjectState, MAX_CHANGES};
    use crate::app::project_session::score::ScoreCellEdit;
    use crate::{
        part::{Part, PartScore},
        project::{Project, Voice, VoiceType},
        seed::Seed,
    };

    fn state(name: &str) -> ProjectState {
        let mut project = Project::new(name, 800, 0, Seed::new(1));
        project.description = name.to_string();
        ProjectState::new_from_project(Arc::new(project))
    }

    fn score_state(value: &str) -> ProjectState {
        named_score_state("intro", value)
    }

    fn cell_edit(before: &str, after: &str) -> ScoreCellEdit {
        ScoreCellEdit {
            row: 0,
            column: 0,
            before: before.to_string(),
            after: after.to_string(),
        }
    }

    fn named_score_state(part_name: &str, value: &str) -> ProjectState {
        let part = Part::new(part_name, 1);
        let project = Project::new("score", 800, 0, Seed::new(1))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = Arc::new(PartScore::from_rows(vec![vec![value.to_string()]]));
        ProjectState::new(Arc::new(project), [(part.name, score.clone(), score)])
    }

    fn two_score_state(first: &str, second: &str) -> ProjectState {
        two_score_state_with_voice("lead", first, second)
    }

    fn two_score_state_with_voice(voice_name: &str, first: &str, second: &str) -> ProjectState {
        let first_part = Part::new("first", 1);
        let second_part = Part::new("second", 1);
        let project = Project::new("score", 800, 0, Seed::new(1))
            .with_voices(vec![Voice::new(1, voice_name, VoiceType::Saw)])
            .with_parts(vec![first_part.clone(), second_part.clone()]);
        let first_score = Arc::new(PartScore::from_rows(vec![vec![first.to_string()]]));
        let second_score = Arc::new(PartScore::from_rows(vec![vec![second.to_string()]]));
        ProjectState::new(
            Arc::new(project),
            [
                (first_part.name, first_score.clone(), first_score),
                (second_part.name, second_score.clone(), second_score),
            ],
        )
    }

    #[test]
    fn records_undo_and_redo_in_order() {
        let mut history = ProjectHistory::new(state("zero"));
        history.record_project(state("one"));
        history.record_project(state("two"));

        let target = history.undo_target().unwrap().unwrap();
        assert_eq!(target.state.project.description, "one");
        history.commit_undo(target.state);
        assert_eq!(
            history
                .undo_target()
                .unwrap()
                .unwrap()
                .state
                .project
                .description,
            "zero"
        );
        assert_eq!(
            history
                .redo_target()
                .unwrap()
                .unwrap()
                .state
                .project
                .description,
            "two"
        );
    }

    #[test]
    fn a_new_edit_after_undo_discards_redo() {
        let mut history = ProjectHistory::new(state("zero"));
        history.record_project(state("one"));
        history.record_project(state("two"));
        let target = history.undo_target().unwrap().unwrap();
        history.commit_undo(target.state);

        history.record_project(state("replacement"));

        assert!(!history.can_redo());
        assert_eq!(
            history
                .undo_target()
                .unwrap()
                .unwrap()
                .state
                .project
                .description,
            "one"
        );
    }

    #[test]
    fn consecutive_changes_to_one_cell_are_one_delta() {
        let mut history = ProjectHistory::new(score_state(""));
        history
            .record_score_cell("intro".into(), cell_edit("", "C"))
            .unwrap();
        history
            .record_score_cell("intro".into(), cell_edit("C", "C4"))
            .unwrap();

        assert_eq!(history.undo_changes.len(), 1);
        assert!(matches!(
            &history.undo_changes[0],
            HistoryChange::ScoreCell {
                edit,
                ..
            } if edit.before.is_empty() && edit.after == "C4"
        ));
        let target = history.undo_target().unwrap().unwrap();
        assert!(!target.project_changed);
        assert_eq!(target.affected_parts, ["intro".into()]);
        assert_eq!(
            target.state.score(&"intro".into()).unwrap().rows()[0][0],
            ""
        );
    }

    #[test]
    fn broad_changes_retain_only_affected_scores() {
        let mut history = ProjectHistory::new(two_score_state("C4", "E4"));

        history.record_project(two_score_state("D4", "E4"));

        let HistoryChange::Project {
            before_scores,
            after_scores,
            ..
        } = &history.undo_changes[0]
        else {
            panic!("a broad change should record a project delta");
        };
        assert_eq!(before_scores.len(), 1);
        assert_eq!(after_scores.len(), 1);
        assert_eq!(before_scores[0].part_name.as_str(), "first");
        assert_eq!(after_scores[0].part_name.as_str(), "first");
    }

    #[test]
    fn voice_header_changes_rewrite_scores_without_retaining_score_copies() {
        let mut history = ProjectHistory::new(two_score_state_with_voice("lead", "C4", "E4"));

        history.record_project(two_score_state_with_voice("melody", "C4", "E4"));

        let HistoryChange::Project {
            before_scores,
            after_scores,
            ..
        } = &history.undo_changes[0]
        else {
            panic!("a voice rename should record a project delta");
        };
        assert!(before_scores.is_empty());
        assert!(after_scores.is_empty());
        let target = history.undo_target().unwrap().unwrap();
        assert!(target.project_changed);
        assert_eq!(target.affected_parts, ["first".into(), "second".into()]);
    }

    #[test]
    fn stacked_renames_restore_the_name_expected_by_older_cell_changes() {
        let mut history = ProjectHistory::new(named_score_state("intro", ""));
        history
            .record_score_cell("intro".into(), cell_edit("", "C4"))
            .unwrap();
        history.record_project(named_score_state("opening", "C4"));

        let before_rename = history.undo_target().unwrap().unwrap();
        assert_eq!(
            before_rename.state.project.parts()[0].name.as_str(),
            "intro"
        );
        history.commit_undo(before_rename.state);
        let before_cell = history.undo_target().unwrap().unwrap();
        assert_eq!(before_cell.state.project.parts()[0].name.as_str(), "intro");
        assert_eq!(
            before_cell.state.score(&"intro".into()).unwrap().rows()[0][0],
            ""
        );

        history.commit_undo(before_cell.state);
        let after_cell = history.redo_target().unwrap().unwrap();
        history.commit_redo(after_cell.state);
        let after_rename = history.redo_target().unwrap().unwrap();
        assert_eq!(
            after_rename.state.project.parts()[0].name.as_str(),
            "opening"
        );
        assert_eq!(
            after_rename.state.score(&"opening".into()).unwrap().rows()[0][0],
            "C4"
        );
    }

    #[test]
    fn bounds_retained_changes() {
        let mut history = ProjectHistory::new(state("0"));
        for index in 1..=MAX_CHANGES + 5 {
            history.record_project(state(&index.to_string()));
        }

        let mut undo_count = 0;
        while let Some(target) = history.undo_target().unwrap() {
            history.commit_undo(target.state);
            undo_count += 1;
        }
        assert_eq!(undo_count, MAX_CHANGES);
    }
}
