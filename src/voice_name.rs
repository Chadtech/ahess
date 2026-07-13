use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct VoiceName(String);

impl VoiceName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn eq_ignore_ascii_case(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl From<&str> for VoiceName {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for VoiceName {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}
