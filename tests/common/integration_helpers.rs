use std::path::PathBuf;
use std::process::Command;

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn concept_result<'a>(
    result: &'a conflic::model::ScanResult,
    concept_id: &str,
) -> &'a conflic::model::ConceptResult {
    result
        .concept_results
        .iter()
        .find(|result| result.concept.id == concept_id)
        .unwrap_or_else(|| panic!("missing concept result for {}", concept_id))
}

pub fn find_concept<'a>(
    result: &'a conflic::model::ScanResult,
    concept_id: &str,
) -> Option<&'a conflic::model::ConceptResult> {
    result
        .concept_results
        .iter()
        .find(|result| result.concept.id == concept_id)
}

/// Initialize a git repo with a single "initial" commit containing all files.
pub fn init_git_repo(repo: &std::path::Path) {
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
}
