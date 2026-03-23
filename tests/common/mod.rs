pub mod e2e_helpers;
pub mod integration_helpers;
use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

/// Temporary workspace builder for end-to-end CLI tests.
pub struct TestWorkspace {
    temp_dir: TempDir,
}

impl TestWorkspace {
    pub fn new() -> Self {
        Self {
            temp_dir: tempfile::tempdir().expect("temporary workspace should be created"),
        }
    }

    pub fn root(&self) -> &Path {
        self.temp_dir.path()
    }

    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root().join(relative)
    }

    pub fn write(&self, relative: impl AsRef<Path>, contents: &str) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent directories should be created");
        }
        std::fs::write(path, contents).expect("test fixture file should be written");
    }

    pub fn read(&self, relative: impl AsRef<Path>) -> String {
        std::fs::read_to_string(self.path(relative)).expect("test fixture file should be readable")
    }

    pub fn write_node_workspace(&self, nvmrc: &str, package_json_node: &str, docker_tag: &str) {
        self.write(".nvmrc", &format!("{}\n", nvmrc));
        self.write(
            "package.json",
            &format!(r#"{{"engines":{{"node":"{}"}}}}"#, package_json_node),
        );
        self.write(
            "Dockerfile",
            &format!("FROM node:{}\nWORKDIR /app\n", docker_tag),
        );
    }

    pub fn write_docker_compose_service(&self, service_name: &str, image: &str) {
        self.write(
            "docker-compose.yml",
            &format!("services:\n  {}:\n    image: {}\n", service_name, image),
        );
    }

    pub fn write_port_workspace(&self, env_port: &str, compose_port: &str) {
        self.write(".env", &format!("PORT={}\n", env_port));
        self.write(
            "docker-compose.yml",
            &format!(
                "services:\n  app:\n    image: node:20-alpine\n    ports:\n      - \"{}:{}\"\n",
                compose_port, compose_port
            ),
        );
    }

    pub fn write_python_conflict_workspace(&self, python_file: &str, pyproject: &str) {
        self.write(".python-version", &format!("{}\n", python_file));
        self.write(
            "pyproject.toml",
            &format!(
                "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \"{}\"\n",
                pyproject
            ),
        );
    }

    pub fn write_monorepo_package(&self, package: &str, nvmrc: &str, package_json_node: &str) {
        let root = PathBuf::from("packages").join(package);
        self.write(root.join(".nvmrc"), &format!("{}\n", nvmrc));
        self.write(
            root.join("package.json"),
            &format!(r#"{{"engines":{{"node":"{}"}}}}"#, package_json_node),
        );
    }

    pub fn init_git_repo(&self) {
        self.run_git(&["init"]);
        self.run_git(&["config", "user.email", "codex@example.com"]);
        self.run_git(&["config", "user.name", "Codex"]);
    }

    pub fn git_add_and_commit(&self, message: &str) {
        self.run_git(&["add", "."]);
        self.run_git(&["commit", "-m", message]);
    }

    fn run_git(&self, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .args(args)
            .current_dir(self.root())
            .output()
            .expect("git command should execute");

        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub fn conflic_cmd() -> Command {
    Command::cargo_bin("conflic").expect("conflic test binary should be available")
}

pub fn conflic_cmd_in(dir: &Path) -> Command {
    let mut command = conflic_cmd();
    command.current_dir(dir);
    command
}
