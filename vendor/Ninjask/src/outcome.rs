//! Weighted successor states for search and regret minimization.
use crate::Battle;

/// Whether to enumerate all damage rolls or approximate them by endpoints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EnumerationMode {
    #[default]
    Precise,
    /// All other chance events remain exact; damage endpoints each get 1/2.
    AlmostPrecise,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub probability: f64,
    pub battle: Battle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumerationError(pub String);

impl std::fmt::Display for EnumerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for EnumerationError {}
