pub mod new_score;

pub enum Page {
    Landing,
    NewScore(new_score::Model),
}
