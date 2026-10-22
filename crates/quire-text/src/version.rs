//! Document versions.

/// Monotonic per [`Document`](crate::Document). It bumps on every applied text change and never on
/// selection-only transactions. Every byte offset the crate hands out is valid only together with
/// the version it was computed at.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Version(pub u64);

impl Version {
    /// The version of a freshly created document.
    pub const INITIAL: Version = Version(0);

    /// The next version.
    #[must_use]
    pub fn next(self) -> Version {
        Version(self.0 + 1)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}
