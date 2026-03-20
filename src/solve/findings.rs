use std::collections::HashMap;
use std::path::Path;

use crate::config::ConflicConfig;
use crate::model::*;

use super::{Compatibility, boolean, port, severity, string, version};

pub(super) fn find_contradictions(
    assertions: &[ConfigAssertion],
    config: &ConflicConfig,
) -> Vec<Finding> {
    let assertion_refs: Vec<&ConfigAssertion> = assertions.iter().collect();
    let mut findings = HashMap::new();
    collect_findings_within(&assertion_refs, config, &mut findings);
    sort_findings(findings)
}

pub(super) fn collect_findings_within(
    assertions: &[&ConfigAssertion],
    config: &ConflicConfig,
    findings: &mut HashMap<FindingDedupKey, Finding>,
) {
    let buckets = build_value_buckets(assertions);
    for index in 0..buckets.len() {
        for right_index in (index + 1)..buckets.len() {
            collect_findings_for_bucket_pair(
                &buckets[index],
                &buckets[right_index],
                config,
                findings,
            );
        }
    }
}

pub(super) fn collect_findings_across(
    left_assertions: &[&ConfigAssertion],
    right_assertions: &[&ConfigAssertion],
    config: &ConflicConfig,
    findings: &mut HashMap<FindingDedupKey, Finding>,
) {
    let left_buckets = build_value_buckets(left_assertions);
    let right_buckets = build_value_buckets(right_assertions);

    for left in &left_buckets {
        for right in &right_buckets {
            collect_findings_for_bucket_pair(left, right, config, findings);
        }
    }
}

pub(super) fn sort_findings(findings: HashMap<FindingDedupKey, Finding>) -> Vec<Finding> {
    let mut findings: Vec<Finding> = findings.into_values().collect();
    findings.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| finding_sort_key(left).cmp(&finding_sort_key(right)))
    });
    findings
}

fn collect_findings_for_bucket_pair(
    left_bucket: &ValueBucket<'_>,
    right_bucket: &ValueBucket<'_>,
    config: &ConflicConfig,
    findings: &mut HashMap<FindingDedupKey, Finding>,
) {
    let Compatibility::Incompatible(explanation) =
        compare_values(&left_bucket.sample.value, &right_bucket.sample.value)
    else {
        return;
    };

    let rule_id = rule_id_for_type(&left_bucket.sample.value);

    for left in &left_bucket.representatives {
        for right in &right_bucket.representatives {
            if normalized_file_key(&left.source.file) == normalized_file_key(&right.source.file) {
                continue;
            }

            let finding = Finding {
                severity: severity::compute_severity(left.authority, right.authority),
                left: (*left).clone(),
                right: (*right).clone(),
                explanation: explanation.clone(),
                rule_id: rule_id.clone(),
            };

            if config.should_ignore_finding(
                &finding.rule_id,
                &finding.left.source.file,
                &finding.right.source.file,
            ) {
                continue;
            }

            let key = finding_dedup_key(&finding, &left_bucket.key, &right_bucket.key);
            match findings.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(finding);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if finding_preferred(&finding, entry.get()) {
                        entry.insert(finding);
                    }
                }
            }
        }
    }
}

fn compare_values(left: &SemanticType, right: &SemanticType) -> Compatibility {
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

fn rule_id_for_type(value: &SemanticType) -> String {
    match value {
        SemanticType::Version(_) => "VER001".into(),
        SemanticType::Port(_) => "PORT001".into(),
        SemanticType::Boolean(_) => "BOOL001".into(),
        SemanticType::StringValue(_) => "STR001".into(),
        _ => "MISC001".into(),
    }
}

#[derive(Debug)]
struct ValueBucket<'a> {
    key: String,
    sample: &'a ConfigAssertion,
    representatives: Vec<&'a ConfigAssertion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FindingDedupKey {
    file_a: String,
    file_b: String,
    value_a: String,
    value_b: String,
    rule_id: String,
}

fn build_value_buckets<'a>(assertions: &[&'a ConfigAssertion]) -> Vec<ValueBucket<'a>> {
    let mut by_value: HashMap<String, Vec<&'a ConfigAssertion>> = HashMap::new();
    for assertion in assertions {
        by_value
            .entry(value_bucket_key(&assertion.value))
            .or_default()
            .push(*assertion);
    }

    let mut buckets: Vec<ValueBucket<'a>> = by_value
        .into_iter()
        .filter_map(|(key, grouped)| {
            let sample = grouped.first().copied()?;
            let representatives = pick_bucket_representatives(&grouped);
            Some(ValueBucket {
                key,
                sample,
                representatives,
            })
        })
        .collect();
    buckets.sort_by(|left, right| left.key.cmp(&right.key));
    buckets
}

fn pick_bucket_representatives<'a>(assertions: &[&'a ConfigAssertion]) -> Vec<&'a ConfigAssertion> {
    let mut by_file: HashMap<String, &'a ConfigAssertion> = HashMap::new();

    for assertion in assertions {
        let file_key = normalized_file_key(&assertion.source.file);
        match by_file.entry(file_key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(*assertion);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if assertion_preferred(assertion, entry.get()) {
                    entry.insert(*assertion);
                }
            }
        }
    }

    let mut representatives: Vec<&'a ConfigAssertion> = by_file.into_values().collect();
    representatives.sort_by_key(|assertion| assertion_sort_key(assertion));
    representatives
}

fn assertion_preferred(candidate: &ConfigAssertion, existing: &ConfigAssertion) -> bool {
    candidate.authority > existing.authority
        || (candidate.authority == existing.authority
            && assertion_sort_key(candidate) < assertion_sort_key(existing))
}

fn finding_preferred(candidate: &Finding, existing: &Finding) -> bool {
    candidate.severity > existing.severity
        || (candidate.severity == existing.severity
            && finding_sort_key(candidate) < finding_sort_key(existing))
}

fn value_bucket_key(value: &SemanticType) -> String {
    match value {
        SemanticType::Version(version) => format!("version:{}", version),
        SemanticType::Port(port) => format!("port:{}", port),
        SemanticType::Boolean(value) => format!("bool:{}", value),
        SemanticType::Path(path) => {
            format!("path:{}", crate::pathing::normalize_path(path).display())
        }
        SemanticType::StringValue(value) => format!("string:{}", value),
        SemanticType::Number(value) => format!("number:{:016x}", value.to_bits()),
    }
}

fn finding_dedup_key(
    finding: &Finding,
    left_value_key: &str,
    right_value_key: &str,
) -> FindingDedupKey {
    let left_file = normalized_file_key(&finding.left.source.file);
    let right_file = normalized_file_key(&finding.right.source.file);

    if (left_file.as_str(), left_value_key) <= (right_file.as_str(), right_value_key) {
        FindingDedupKey {
            file_a: left_file,
            file_b: right_file,
            value_a: left_value_key.to_string(),
            value_b: right_value_key.to_string(),
            rule_id: finding.rule_id.clone(),
        }
    } else {
        FindingDedupKey {
            file_a: right_file,
            file_b: left_file,
            value_a: right_value_key.to_string(),
            value_b: left_value_key.to_string(),
            rule_id: finding.rule_id.clone(),
        }
    }
}

fn finding_sort_key(finding: &Finding) -> (String, String, usize, usize, String) {
    (
        normalized_file_key(&finding.left.source.file),
        normalized_file_key(&finding.right.source.file),
        finding.left.source.line,
        finding.right.source.line,
        finding.rule_id.clone(),
    )
}

fn assertion_sort_key(assertion: &ConfigAssertion) -> (String, usize, usize, String) {
    (
        normalized_file_key(&assertion.source.file),
        assertion.source.line,
        assertion.source.column,
        assertion.extractor_id.clone(),
    )
}

fn normalized_file_key(path: &Path) -> String {
    crate::pathing::normalize_path(path)
        .to_string_lossy()
        .into_owned()
}
