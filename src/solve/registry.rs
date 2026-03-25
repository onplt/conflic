use std::collections::HashMap;

use super::constraint::ConstraintSolver;
use super::solver_trait::Solver;
use super::{Compatibility, boolean, port, string, version};
use crate::model::SemanticType;

/// Registry mapping concept IDs to custom solvers.
///
/// When a concept has a registered solver, its raw string values are compared
/// using that solver instead of the default type-dispatched comparison.
#[derive(Default)]
pub struct SolverRegistry {
    /// Maps concept ID → pairwise solver.
    concept_solvers: HashMap<String, Box<dyn Solver>>,
    /// Maps concept ID → N-ary constraint solver (opt-in upgrade).
    constraint_solvers: HashMap<String, Box<dyn ConstraintSolver>>,
}

impl SolverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pairwise solver for a specific concept.
    pub fn register(&mut self, concept_id: String, solver: Box<dyn Solver>) {
        self.concept_solvers.insert(concept_id, solver);
    }

    /// Register an N-ary constraint solver for a specific concept.
    pub fn register_constraint(&mut self, concept_id: String, solver: Box<dyn ConstraintSolver>) {
        self.constraint_solvers.insert(concept_id, solver);
    }

    /// Look up the pairwise solver for a concept, if one is registered.
    pub fn get(&self, concept_id: &str) -> Option<&dyn Solver> {
        self.concept_solvers.get(concept_id).map(|s| s.as_ref())
    }

    /// Look up the constraint solver for a concept, if one is registered.
    pub fn get_constraint(&self, concept_id: &str) -> Option<&dyn ConstraintSolver> {
        self.constraint_solvers.get(concept_id).map(|s| s.as_ref())
    }
}

// ── Built-in solver wrappers ──────────────────────────────────────────

/// Solver that compares values as semver versions/ranges.
pub struct SemverSolver;

impl Solver for SemverSolver {
    fn id(&self) -> &str {
        "semver"
    }

    fn compatible(&self, left: &str, right: &str) -> Compatibility {
        let vl = crate::model::parse_version(left);
        let vr = crate::model::parse_version(right);
        version::versions_compatible(&vl, &vr)
    }

    fn rule_id(&self) -> &str {
        "VER001"
    }
}

/// Solver that compares values as port specifications.
pub struct PortSolver;

impl Solver for PortSolver {
    fn id(&self) -> &str {
        "port"
    }

    fn compatible(&self, left: &str, right: &str) -> Compatibility {
        let pl = crate::model::parse_port(left);
        let pr = crate::model::parse_port(right);
        match (pl, pr) {
            (Some(l), Some(r)) => port::ports_compatible(&l, &r),
            _ => Compatibility::Unknown,
        }
    }

    fn rule_id(&self) -> &str {
        "PORT001"
    }
}

/// Solver that compares values as booleans.
pub struct BooleanSolver;

impl Solver for BooleanSolver {
    fn id(&self) -> &str {
        "boolean"
    }

    fn compatible(&self, left: &str, right: &str) -> Compatibility {
        let bl = crate::model::normalize_boolean(left);
        let br = crate::model::normalize_boolean(right);
        match (bl, br) {
            (Some(l), Some(r)) => boolean::booleans_compatible(l, r),
            _ => Compatibility::Unknown,
        }
    }

    fn rule_id(&self) -> &str {
        "BOOL001"
    }
}

/// Solver that compares values as exact strings.
pub struct ExactStringSolver;

impl Solver for ExactStringSolver {
    fn id(&self) -> &str {
        "exact-string"
    }

    fn compatible(&self, left: &str, right: &str) -> Compatibility {
        string::strings_compatible(left, right)
    }

    fn rule_id(&self) -> &str {
        "STR001"
    }
}

/// Compare two `SemanticType` values using the default type-dispatched logic.
/// This is the existing comparison path, extracted here for reuse.
pub fn compare_values_default(left: &SemanticType, right: &SemanticType) -> Compatibility {
    match (left, right) {
        (SemanticType::Version(left), SemanticType::Version(right)) => {
            version::versions_compatible(left, right)
        }
        (SemanticType::Port(left), SemanticType::Port(right)) => {
            port::ports_compatible(left, right)
        }
        (SemanticType::Boolean(left), SemanticType::Boolean(right)) => {
            boolean::booleans_compatible(*left, *right)
        }
        (SemanticType::StringValue(left), SemanticType::StringValue(right)) => {
            string::strings_compatible(left, right)
        }
        _ => Compatibility::Unknown,
    }
}

/// Return the default rule ID for a `SemanticType`.
pub fn rule_id_for_type(value: &SemanticType) -> String {
    match value {
        SemanticType::Version(_) => "VER001".into(),
        SemanticType::Port(_) => "PORT001".into(),
        SemanticType::Boolean(_) => "BOOL001".into(),
        SemanticType::StringValue(_) => "STR001".into(),
        _ => "MISC001".into(),
    }
}

/// Resolve a solver name from config to a boxed Solver.
pub fn solver_from_name(name: &str) -> Option<Box<dyn Solver>> {
    match name {
        "semver" | "version" => Some(Box::new(SemverSolver)),
        "port" => Some(Box::new(PortSolver)),
        "boolean" | "bool" => Some(Box::new(BooleanSolver)),
        "exact-string" | "string" => Some(Box::new(ExactStringSolver)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_solver_compatible() {
        let solver = SemverSolver;
        assert!(matches!(
            solver.compatible("20.11.0", "20.11.0"),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_semver_solver_incompatible() {
        let solver = SemverSolver;
        assert!(matches!(
            solver.compatible("20.11.0", "22.0.0"),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_semver_solver_range() {
        let solver = SemverSolver;
        assert!(matches!(
            solver.compatible("20.0.0", ">=18.0.0 <21.0.0"),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_port_solver_compatible() {
        let solver = PortSolver;
        assert!(matches!(
            solver.compatible("8080", "8080"),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_port_solver_incompatible() {
        let solver = PortSolver;
        assert!(matches!(
            solver.compatible("8080", "3000"),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_boolean_solver_compatible() {
        let solver = BooleanSolver;
        assert!(matches!(
            solver.compatible("true", "yes"),
            Compatibility::Compatible
        ));
    }

    #[test]
    fn test_boolean_solver_incompatible() {
        let solver = BooleanSolver;
        assert!(matches!(
            solver.compatible("true", "false"),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_exact_string_solver() {
        let solver = ExactStringSolver;
        assert!(matches!(
            solver.compatible("hello", "hello"),
            Compatibility::Compatible
        ));
        assert!(matches!(
            solver.compatible("hello", "world"),
            Compatibility::Incompatible(_)
        ));
    }

    #[test]
    fn test_registry_lookup() {
        let mut registry = SolverRegistry::new();
        registry.register("my-concept".into(), Box::new(SemverSolver));
        assert!(registry.get("my-concept").is_some());
        assert!(registry.get("other").is_none());
    }

    #[test]
    fn test_solver_from_name() {
        assert!(solver_from_name("semver").is_some());
        assert!(solver_from_name("version").is_some());
        assert!(solver_from_name("port").is_some());
        assert!(solver_from_name("boolean").is_some());
        assert!(solver_from_name("bool").is_some());
        assert!(solver_from_name("exact-string").is_some());
        assert!(solver_from_name("string").is_some());
        assert!(solver_from_name("unknown-solver").is_none());
    }
}
