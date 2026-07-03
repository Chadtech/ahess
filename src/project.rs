use crate::seed::Seed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub beat_length: u32,
    pub timing_variance: u32,
    pub seed: Seed,
}

impl Project {
    pub fn new(
        name: impl Into<String>,
        beat_length: u32,
        timing_variance: u32,
        seed: Seed,
    ) -> Self {
        Self {
            name: name.into(),
            beat_length,
            timing_variance,
            seed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Project;
    use crate::seed::Seed;

    #[test]
    fn project_stores_the_initial_music_settings() {
        let project = Project::new("test", 4000, 100, Seed::new(19));

        assert_eq!(project.name, "test");
        assert_eq!(project.beat_length, 4000);
        assert_eq!(project.timing_variance, 100);
        assert_eq!(project.seed, Seed::new(19));
    }
}
