use super::Compatibility;

/// Trait for comparing two raw string values for semantic compatibility.
///
/// Built-in solvers wrap the existing type-specific comparison functions
/// (version, port, boolean, string). Custom solvers can be registered
/// via `.conflic.toml` to provide domain-specific comparison logic.
pub trait Solver: Send + Sync {
    /// Unique identifier for this solver (e.g. "semver", "port", "exact-string").
    fn id(&self) -> &str;

    /// Compare two raw values and return their compatibility.
    fn compatible(&self, left: &str, right: &str) -> Compatibility;

    /// The rule ID prefix used for findings produced by this solver.
    fn rule_id(&self) -> &str;
}
