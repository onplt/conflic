use crate::model::{ConfigAssertion, SemanticType, VersionSpec};

/// Result of N-ary constraint satisfaction check.
#[derive(Debug)]
pub enum ConstraintResult {
    /// All assertions can be satisfied simultaneously.
    Satisfiable {
        /// A witness value that satisfies all constraints.
        witness: String,
    },
    /// No single value satisfies all assertions; `minimal_conflict` contains
    /// indices (into the input slice) of the smallest unsatisfiable subset.
    Unsatisfiable { minimal_conflict: Vec<usize> },
}

/// Trait for N-ary constraint satisfaction over a set of assertions.
///
/// Unlike the pairwise `Solver` trait, a `ConstraintSolver` considers
/// **all** assertions for a concept simultaneously and reports whether
/// there exists a single value that satisfies every assertion.
pub trait ConstraintSolver: Send + Sync {
    /// Check whether the given assertions can all be satisfied simultaneously.
    fn satisfiable(&self, assertions: &[&ConfigAssertion]) -> ConstraintResult;
}

/// Half-open interval `[lo, hi)` over a linear version space.
/// Versions are encoded as `major * 1_000_000 + minor * 1_000 + patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    lo: u64,
    hi: u64,
}

impl Interval {
    fn intersect(self, other: Interval) -> Option<Interval> {
        let lo = self.lo.max(other.lo);
        let hi = self.hi.min(other.hi);
        if lo < hi {
            Some(Interval { lo, hi })
        } else {
            None
        }
    }

    fn is_empty(self) -> bool {
        self.lo >= self.hi
    }
}

fn encode_version(major: u64, minor: u64, patch: u64) -> u64 {
    major * 1_000_000 + minor * 1_000 + patch
}

fn decode_version(encoded: u64) -> (u64, u64, u64) {
    let major = encoded / 1_000_000;
    let minor = (encoded % 1_000_000) / 1_000;
    let patch = encoded % 1_000;
    (major, minor, patch)
}

/// Convert a `VersionSpec` to a set of `Interval`s.
/// Returns `None` if the version cannot be represented as intervals.
fn version_to_intervals(spec: &VersionSpec) -> Option<Vec<Interval>> {
    match spec {
        VersionSpec::Exact(v) => {
            let enc = encode_version(v.major, v.minor, v.patch);
            Some(vec![Interval {
                lo: enc,
                hi: enc + 1,
            }])
        }
        VersionSpec::Partial { major, minor } => match minor {
            Some(m) => {
                let lo = encode_version(*major, *m, 0);
                let hi = encode_version(*major, *m + 1, 0);
                Some(vec![Interval { lo, hi }])
            }
            None => {
                let lo = encode_version(*major, 0, 0);
                let hi = encode_version(*major + 1, 0, 0);
                Some(vec![Interval { lo, hi }])
            }
        },
        VersionSpec::Range(range) => {
            // Use node-semver to test a grid of versions and approximate intervals.
            // For common ranges like ">=18 <20" or "^20", this is precise enough.
            range_to_intervals(range)
        }
        VersionSpec::DockerTag { version, .. } => {
            let reparsed = crate::model::parse_version(version);
            if matches!(&reparsed, VersionSpec::DockerTag { .. }) {
                None
            } else {
                version_to_intervals(&reparsed)
            }
        }
        VersionSpec::Unparsed(_) => None,
    }
}

/// Approximate a node-semver Range as a set of Intervals by scanning
/// major and minor version pairs.
///
/// Probes every `(major, minor, 0)` point for major 0..=200 and minor
/// 0..=99.  This covers all real-world runtime version schemes including
/// Python (3.x), Go (1.x), and Ruby (3.x) whose versions differ only at
/// the minor level within a single major.
fn range_to_intervals(range: &node_semver::Range) -> Option<Vec<Interval>> {
    let mut intervals = Vec::new();
    let mut in_range = false;
    let mut range_start = 0u64;

    for major in 0..=200u64 {
        for minor in 0..=99u64 {
            let test = node_semver::Version::from((major, minor, 0u64));
            let encoded = encode_version(major, minor, 0);

            if range.satisfies(&test) {
                if !in_range {
                    range_start = encoded;
                    in_range = true;
                }
            } else if in_range {
                intervals.push(Interval {
                    lo: range_start,
                    hi: encoded,
                });
                in_range = false;
            }
        }
    }
    if in_range {
        intervals.push(Interval {
            lo: range_start,
            hi: encode_version(201, 0, 0),
        });
    }

    if intervals.is_empty() {
        None
    } else {
        Some(intervals)
    }
}

/// Intersect two sets of intervals (unions of disjoint intervals).
fn intersect_interval_sets(a: &[Interval], b: &[Interval]) -> Vec<Interval> {
    let mut result = Vec::new();
    let mut ai = 0;
    let mut bi = 0;

    while ai < a.len() && bi < b.len() {
        if let Some(inter) = a[ai].intersect(b[bi])
            && !inter.is_empty()
        {
            result.push(inter);
        }
        // Advance the interval that ends first
        if a[ai].hi <= b[bi].hi {
            ai += 1;
        } else {
            bi += 1;
        }
    }

    result
}

/// Constraint solver for version concepts.
///
/// Converts each assertion's `VersionSpec` into a set of half-open intervals,
/// intersects all intervals, and reports satisfiability in O(n log n).
pub struct VersionConstraintSolver;

/// Check if a version spec involves prerelease versions, which need special
/// semver semantics that interval arithmetic cannot capture.
fn has_prerelease(spec: &VersionSpec) -> bool {
    match spec {
        VersionSpec::Exact(v) => !v.pre.is_empty(),
        _ => false,
    }
}

impl ConstraintSolver for VersionConstraintSolver {
    fn satisfiable(&self, assertions: &[&ConfigAssertion]) -> ConstraintResult {
        // Prerelease versions have special semver semantics that interval
        // arithmetic cannot represent. Fall back to pairwise (return Unsatisfiable
        // with empty conflict to trigger full pairwise).
        let has_prerelease_assertion = assertions
            .iter()
            .any(|a| matches!(&a.value, SemanticType::Version(spec) if has_prerelease(spec)));
        if has_prerelease_assertion {
            return ConstraintResult::Unsatisfiable {
                minimal_conflict: vec![],
            };
        }

        // Convert each assertion to intervals
        let mut assertion_intervals: Vec<(usize, Vec<Interval>)> = Vec::new();

        for (idx, assertion) in assertions.iter().enumerate() {
            if let SemanticType::Version(ref spec) = assertion.value
                && let Some(intervals) = version_to_intervals(spec)
            {
                assertion_intervals.push((idx, intervals));
            }
            // If we can't convert, skip — it won't participate in constraint check
        }

        if assertion_intervals.len() < 2 {
            // Not enough interval-representable assertions to check
            let witness = assertions
                .first()
                .map(|a| a.raw_value.clone())
                .unwrap_or_default();
            return ConstraintResult::Satisfiable { witness };
        }

        // Compute cumulative intersection
        let mut current = assertion_intervals[0].1.clone();
        let mut first_empty_idx = None;

        for (i, (_idx, intervals)) in assertion_intervals.iter().enumerate().skip(1) {
            let next = intersect_interval_sets(&current, intervals);
            if next.is_empty() {
                first_empty_idx = Some(i);
                break;
            }
            current = next;
        }

        if let Some(empty_idx) = first_empty_idx {
            // Find minimal unsatisfiable core by trying pairs
            let minimal = find_minimal_conflict(&assertion_intervals, empty_idx);
            ConstraintResult::Unsatisfiable {
                minimal_conflict: minimal,
            }
        } else {
            // Satisfiable — pick witness from the first interval
            let witness = if let Some(interval) = current.first() {
                let (major, minor, patch) = decode_version(interval.lo);
                format!("{}.{}.{}", major, minor, patch)
            } else {
                assertions
                    .first()
                    .map(|a| a.raw_value.clone())
                    .unwrap_or_default()
            };
            ConstraintResult::Satisfiable { witness }
        }
    }
}

/// Find the minimal unsatisfiable conflict set.
/// Returns original indices into the assertions slice.
fn find_minimal_conflict(
    assertion_intervals: &[(usize, Vec<Interval>)],
    known_empty_idx: usize,
) -> Vec<usize> {
    // Try each pair involving the assertion that caused emptiness
    for i in 0..known_empty_idx {
        let intersection = intersect_interval_sets(
            &assertion_intervals[i].1,
            &assertion_intervals[known_empty_idx].1,
        );
        if intersection.is_empty() {
            return vec![
                assertion_intervals[i].0,
                assertion_intervals[known_empty_idx].0,
            ];
        }
    }

    // Fallback: return first two and the one that caused emptiness
    let mut result: Vec<usize> = assertion_intervals[..=known_empty_idx]
        .iter()
        .map(|(idx, _)| *idx)
        .collect();
    result.truncate(3);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::model::semantic_type::{SemanticType, parse_version};
    use std::path::PathBuf;

    fn make_version_assertion(raw: &str) -> ConfigAssertion {
        let parsed = parse_version(raw);
        ConfigAssertion {
            concept: SemanticConcept {
                id: "node-version".into(),
                display_name: "Node.js Version".into(),
                category: ConceptCategory::RuntimeVersion,
            },
            value: SemanticType::Version(parsed),
            raw_value: raw.into(),
            source: SourceLocation {
                file: PathBuf::from("test.json"),
                line: 1,
                column: 0,
                key_path: "".into(),
            },
            span: None,
            authority: Authority::Declared,
            extractor_id: "test".into(),
            is_matrix: false,
        }
    }

    #[test]
    fn test_satisfiable_exact_versions_same() {
        let a1 = make_version_assertion("20.11.0");
        let a2 = make_version_assertion("20.11.0");
        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2];

        let solver = VersionConstraintSolver;
        match solver.satisfiable(&refs) {
            ConstraintResult::Satisfiable { .. } => {}
            ConstraintResult::Unsatisfiable { .. } => panic!("Should be satisfiable"),
        }
    }

    #[test]
    fn test_unsatisfiable_exact_versions_differ() {
        let a1 = make_version_assertion("18.0.0");
        let a2 = make_version_assertion("20.0.0");
        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2];

        let solver = VersionConstraintSolver;
        match solver.satisfiable(&refs) {
            ConstraintResult::Unsatisfiable {
                minimal_conflict, ..
            } => {
                assert!(minimal_conflict.len() >= 2);
            }
            ConstraintResult::Satisfiable { .. } => panic!("Should be unsatisfiable"),
        }
    }

    #[test]
    fn test_satisfiable_exact_within_range() {
        let a1 = make_version_assertion("20.0.0");
        let a2 = make_version_assertion(">=18");
        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2];

        let solver = VersionConstraintSolver;
        match solver.satisfiable(&refs) {
            ConstraintResult::Satisfiable { .. } => {}
            ConstraintResult::Unsatisfiable { .. } => panic!("Should be satisfiable"),
        }
    }

    #[test]
    fn test_unsatisfiable_range_and_exact_outside() {
        let a1 = make_version_assertion("22.0.0");
        let a2 = make_version_assertion(">=18");
        let a3 = make_version_assertion("20.0.0");
        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2, &a3];

        let solver = VersionConstraintSolver;
        match solver.satisfiable(&refs) {
            ConstraintResult::Unsatisfiable {
                minimal_conflict, ..
            } => {
                assert!(minimal_conflict.len() >= 2);
                // Should identify 22.0.0 vs 20.0.0 as the conflict
            }
            ConstraintResult::Satisfiable { .. } => panic!("Should be unsatisfiable"),
        }
    }

    #[test]
    fn test_satisfiable_partial_versions() {
        let a1 = make_version_assertion("20");
        let a2 = make_version_assertion("20.11.0");
        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2];

        let solver = VersionConstraintSolver;
        match solver.satisfiable(&refs) {
            ConstraintResult::Satisfiable { .. } => {}
            ConstraintResult::Unsatisfiable { .. } => panic!("Should be satisfiable"),
        }
    }

    #[test]
    fn test_interval_intersect() {
        let a = Interval { lo: 10, hi: 20 };
        let b = Interval { lo: 15, hi: 25 };
        assert_eq!(a.intersect(b), Some(Interval { lo: 15, hi: 20 }));

        let c = Interval { lo: 20, hi: 30 };
        assert_eq!(a.intersect(c), None);
    }

    #[test]
    fn test_intersect_interval_sets() {
        let a = vec![Interval { lo: 0, hi: 10 }, Interval { lo: 20, hi: 30 }];
        let b = vec![Interval { lo: 5, hi: 25 }];
        let result = intersect_interval_sets(&a, &b);
        assert_eq!(
            result,
            vec![Interval { lo: 5, hi: 10 }, Interval { lo: 20, hi: 25 }]
        );
    }

    // ── Bug proof: minor-version-only ranges produce false Satisfiable ──

    #[test]
    fn test_range_to_intervals_minor_version_range_returns_some() {
        // ">=3.10.0 <3.11.0" is a real-world Python version range.
        // range_to_intervals only tests major.0.0 points, so 3.0.0 fails >=3.10.0
        // and 4.0.0 fails <3.11.0 — the function returns None (empty).
        let range = node_semver::Range::parse(">=3.10.0 <3.11.0").unwrap();
        let intervals = range_to_intervals(&range);

        // BUG: This assertion FAILS — intervals is None because the scan
        // only tests major.0.0 points, missing the entire 3.10.x band.
        assert!(
            intervals.is_some(),
            "range_to_intervals must produce intervals for minor-version ranges like >=3.10.0 <3.11.0"
        );
    }

    #[test]
    fn test_unsatisfiable_python_minor_version_contradiction() {
        // Python 3.10 vs Python 3.12: clearly incompatible versions.
        // "3.10" parses as Range(>=3.10.0 <3.11.0),
        // "3.12" parses as Range(>=3.12.0 <3.13.0).
        let a1 = make_version_assertion("3.10");
        let a2 = make_version_assertion("3.12");

        // Verify they both parse as Range (not Partial)
        assert!(
            matches!(&a1.value, SemanticType::Version(VersionSpec::Range(_))),
            "3.10 should parse as Range, got {:?}",
            a1.value
        );
        assert!(
            matches!(&a2.value, SemanticType::Version(VersionSpec::Range(_))),
            "3.12 should parse as Range, got {:?}",
            a2.value
        );

        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2];
        let solver = VersionConstraintSolver;

        // BUG: The constraint solver returns Satisfiable here because
        // range_to_intervals returns None for both ranges (only tests major.0.0),
        // leaving assertion_intervals.len() < 2, which triggers the early return.
        // The pairwise solver correctly identifies this as Incompatible.
        match solver.satisfiable(&refs) {
            ConstraintResult::Unsatisfiable { .. } => {
                // This is the CORRECT outcome — 3.10 and 3.12 are incompatible.
            }
            ConstraintResult::Satisfiable { .. } => {
                panic!(
                    "BUG: VersionConstraintSolver falsely reports Python 3.10 vs 3.12 as \
                     Satisfiable because range_to_intervals only scans major.0.0 points \
                     and misses minor-version ranges entirely. This causes the fast-path \
                     to skip pairwise comparison, silently dropping the contradiction."
                );
            }
        }
    }

    #[test]
    fn test_unsatisfiable_go_minor_version_contradiction() {
        // Go 1.21 vs Go 1.22 — same bug as Python, all Go versions share major=1.
        let a1 = make_version_assertion("1.21");
        let a2 = make_version_assertion("1.22");
        let refs: Vec<&ConfigAssertion> = vec![&a1, &a2];
        let solver = VersionConstraintSolver;

        match solver.satisfiable(&refs) {
            ConstraintResult::Unsatisfiable { .. } => {}
            ConstraintResult::Satisfiable { .. } => {
                panic!(
                    "BUG: Go 1.21 vs 1.22 falsely reported as Satisfiable — \
                     same range_to_intervals major-only scan defect"
                );
            }
        }
    }

    /// End-to-end proof: the full comparison pipeline WITH the constraint
    /// solver registry (as built by the real pipeline) silently drops
    /// Python version contradictions.
    #[test]
    fn test_full_pipeline_drops_python_contradiction() {
        use crate::config::ConflicConfig;
        use crate::solve::SolverRegistry;
        use std::path::Path;

        fn make_python_assertion(file: &str, raw: &str) -> ConfigAssertion {
            let parsed = parse_version(raw);
            ConfigAssertion {
                concept: SemanticConcept {
                    id: "python-version".into(),
                    display_name: "Python Version".into(),
                    category: ConceptCategory::RuntimeVersion,
                },
                value: SemanticType::Version(parsed),
                raw_value: raw.into(),
                source: SourceLocation {
                    file: PathBuf::from(file),
                    line: 1,
                    column: 0,
                    key_path: "".into(),
                },
                span: None,
                authority: Authority::Declared,
                extractor_id: "test".into(),
                is_matrix: false,
            }
        }

        let assertions = vec![
            make_python_assertion(".python-version", "3.10"),
            make_python_assertion("Dockerfile", "3.12"),
        ];
        let config = ConflicConfig::default();

        // Build the solver registry exactly as the real pipeline does
        // (registers VersionConstraintSolver for python-version).
        let mut solvers = SolverRegistry::new();
        solvers.register_constraint(
            "python-version".to_string(),
            Box::new(VersionConstraintSolver),
        );

        let results = crate::solve::compare_assertions_with_solvers(
            Path::new("."),
            assertions,
            &config,
            &solvers,
        );

        let python_result = results
            .iter()
            .find(|r| r.concept.id == "python-version")
            .expect("python-version concept should be in results");

        // BUG: This assertion FAILS — findings is empty because the
        // constraint solver fast-path returns Satisfiable, skipping
        // the pairwise comparison that would correctly detect this.
        assert!(
            !python_result.findings.is_empty(),
            "BUG: Python 3.10 vs 3.12 contradiction silently dropped by constraint solver fast-path. \
             The pairwise solver (versions_compatible) correctly identifies these as Incompatible, \
             but the constraint solver's range_to_intervals returns None for minor-version ranges, \
             causing it to falsely return Satisfiable and skip pairwise entirely."
        );
    }
}
