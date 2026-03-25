use std::collections::HashMap;
use std::path::Path;

use globset::{Glob, GlobMatcher};

use crate::config::ConflicConfig;
use crate::model::*;
use crate::solve::{Compatibility, registry};

/// Configuration for environment promotion chain validation.
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct PromotionConfig {
    /// Ordered chain of environment names, e.g. ["dev", "staging", "prod"].
    #[serde(default)]
    pub chain: Vec<String>,
    /// File patterns for environment detection.
    #[serde(default)]
    pub pattern: Vec<PromotionPattern>,
}

/// Associates file patterns with an environment stage.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PromotionPattern {
    /// Environment name (must appear in `chain`).
    pub environment: String,
    /// Glob patterns that match files belonging to this environment.
    pub files: Vec<String>,
}

/// Compiled version of promotion patterns for fast matching.
struct CompiledPromotion {
    chain: Vec<String>,
    matchers: Vec<(usize, GlobMatcher)>, // (chain_index, matcher)
}

impl CompiledPromotion {
    fn compile(config: &PromotionConfig) -> Option<Self> {
        if config.chain.len() < 2 || config.pattern.is_empty() {
            return None;
        }

        let chain_index: HashMap<&str, usize> = config
            .chain
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i))
            .collect();

        let mut matchers = Vec::new();
        for pattern in &config.pattern {
            if let Some(&idx) = chain_index.get(pattern.environment.as_str()) {
                for file_glob in &pattern.files {
                    if let Ok(glob) = Glob::new(file_glob) {
                        matchers.push((idx, glob.compile_matcher()));
                    }
                }
            }
        }

        if matchers.is_empty() {
            return None;
        }

        Some(CompiledPromotion {
            chain: config.chain.clone(),
            matchers,
        })
    }

    /// Detect which environment a file belongs to based on its path.
    fn detect_environment(&self, file_path: &Path) -> Option<usize> {
        let path_str = file_path.to_string_lossy();
        // Also try just the filename for patterns like "*.prod.*"
        let filename = file_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut best_match: Option<usize> = None;

        for (chain_idx, matcher) in &self.matchers {
            if matcher.is_match(path_str.as_ref())
                || matcher.is_match(&filename)
            {
                // Prefer the most specific (highest chain index) match
                match best_match {
                    None => best_match = Some(*chain_idx),
                    Some(existing) if *chain_idx > existing => best_match = Some(*chain_idx),
                    _ => {}
                }
            }
        }

        best_match
    }
}

/// Evaluate promotion chain rules and produce findings for cross-environment
/// contradictions that violate the promotion direction.
pub fn evaluate_promotions(
    concept_results: &mut [ConceptResult],
    config: &ConflicConfig,
) {
    let promotion_config = match &config.promotion {
        Some(pc) if !pc.chain.is_empty() => pc,
        _ => return,
    };

    let compiled = match CompiledPromotion::compile(promotion_config) {
        Some(c) => c,
        None => return,
    };

    for cr in concept_results.iter_mut() {
        let mut env_assertions: HashMap<usize, Vec<&ConfigAssertion>> = HashMap::new();

        for assertion in &cr.assertions {
            if let Some(env_idx) = compiled.detect_environment(&assertion.source.file) {
                env_assertions.entry(env_idx).or_default().push(assertion);
            }
        }

        if env_assertions.len() < 2 {
            continue;
        }

        // Compare each adjacent pair in the chain
        let mut env_indices: Vec<usize> = env_assertions.keys().copied().collect();
        env_indices.sort();

        for i in 0..env_indices.len() {
            for j in (i + 1)..env_indices.len() {
                let lower_env = env_indices[i];
                let higher_env = env_indices[j];

                let lower_assertions = &env_assertions[&lower_env];
                let higher_assertions = &env_assertions[&higher_env];

                // Compare best assertion from each environment
                let lower_best = lower_assertions
                    .iter()
                    .max_by_key(|a| a.authority)
                    .unwrap();
                let higher_best = higher_assertions
                    .iter()
                    .max_by_key(|a| a.authority)
                    .unwrap();

                // Check compatibility
                let compat =
                    registry::compare_values_default(&lower_best.value, &higher_best.value);

                if let Compatibility::Incompatible(explanation) = compat {
                    let lower_env_name = &compiled.chain[lower_env];
                    let higher_env_name = &compiled.chain[higher_env];

                    let promo_explanation = format!(
                        "{} ({}) differs from {} ({}): {}",
                        lower_env_name,
                        lower_best.raw_value,
                        higher_env_name,
                        higher_best.raw_value,
                        explanation,
                    );

                    cr.findings.push(Finding {
                        severity: Severity::Warning,
                        left: (*lower_best).clone(),
                        right: (*higher_best).clone(),
                        explanation: promo_explanation,
                        rule_id: "PROMO001".into(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConflicConfig;
    use crate::model::assertion::{Authority, ConfigAssertion, SourceLocation};
    use crate::model::concept::{ConceptCategory, SemanticConcept};
    use crate::model::semantic_type::{SemanticType, parse_version};
    use std::path::PathBuf;

    fn make_assertion(file: &str, raw: &str) -> ConfigAssertion {
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

    fn make_promotion_config() -> PromotionConfig {
        PromotionConfig {
            chain: vec!["dev".into(), "staging".into(), "prod".into()],
            pattern: vec![
                PromotionPattern {
                    environment: "dev".into(),
                    files: vec!["*.dev.*".into(), "*.dev".into()],
                },
                PromotionPattern {
                    environment: "staging".into(),
                    files: vec!["*.staging.*".into(), "*.staging".into()],
                },
                PromotionPattern {
                    environment: "prod".into(),
                    files: vec!["*.prod.*".into(), "*.prod".into()],
                },
            ],
        }
    }

    #[test]
    fn test_promotion_detects_cross_env_contradiction() {
        let a_dev = make_assertion(".env.dev", "18");
        let a_prod = make_assertion(".env.prod", "20");

        let mut results = vec![ConceptResult {
            concept: a_dev.concept.clone(),
            assertions: vec![a_dev, a_prod],
            findings: vec![],
        }];

        let mut config = ConflicConfig::default();
        config.promotion = Some(make_promotion_config());

        evaluate_promotions(&mut results, &config);

        assert!(
            !results[0].findings.is_empty(),
            "Should detect cross-environment version difference"
        );
        assert_eq!(results[0].findings[0].rule_id, "PROMO001");
    }

    #[test]
    fn test_promotion_no_finding_when_same_value() {
        let a_dev = make_assertion(".env.dev", "20");
        let a_prod = make_assertion(".env.prod", "20");

        let mut results = vec![ConceptResult {
            concept: a_dev.concept.clone(),
            assertions: vec![a_dev, a_prod],
            findings: vec![],
        }];

        let mut config = ConflicConfig::default();
        config.promotion = Some(make_promotion_config());

        evaluate_promotions(&mut results, &config);

        assert!(
            results[0].findings.is_empty(),
            "Same value across environments should not produce findings"
        );
    }

    #[test]
    fn test_promotion_skipped_when_no_config() {
        let a = make_assertion(".env.dev", "18");
        let mut results = vec![ConceptResult {
            concept: a.concept.clone(),
            assertions: vec![a],
            findings: vec![],
        }];

        let config = ConflicConfig::default();
        evaluate_promotions(&mut results, &config);

        assert!(results[0].findings.is_empty());
    }

    #[test]
    fn test_compiled_promotion_detects_environment() {
        let pc = make_promotion_config();
        let compiled = CompiledPromotion::compile(&pc).unwrap();

        assert_eq!(
            compiled.detect_environment(Path::new(".env.dev")),
            Some(0)
        );
        assert_eq!(
            compiled.detect_environment(Path::new(".env.staging")),
            Some(1)
        );
        assert_eq!(
            compiled.detect_environment(Path::new(".env.prod")),
            Some(2)
        );
        assert_eq!(
            compiled.detect_environment(Path::new(".nvmrc")),
            None
        );
    }
}
