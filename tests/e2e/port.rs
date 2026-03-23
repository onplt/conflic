use crate::common::{TestWorkspace, conflic_cmd_in};
use predicates::prelude::*;

#[test]
fn test_cli_env_inline_comments_and_quotes_do_not_hide_port_conflicts() {
    let workspace = TestWorkspace::new();
    workspace.write(
        ".env",
        "PORT=8080 # app port\nAPP_PORT=\"8080\" # quoted app port\nSERVER_PORT='8080' # single quoted\nHOST=\"localhost # keep hash\"\n",
    );
    workspace.write("Dockerfile", "EXPOSE 3000\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Application Port"))
        .stdout(predicate::str::contains("(PORT)"))
        .stdout(predicate::str::contains("(APP_PORT)"))
        .stdout(predicate::str::contains("(SERVER_PORT)"))
        .stdout(predicate::str::contains("8080"))
        .stdout(predicate::str::contains("3000"));
}

#[test]
fn test_cli_port_ranges_treat_inside_and_boundary_values_as_compatible() {
    let inside_range = TestWorkspace::new();
    inside_range.write(".env", "PORT=3001\n");
    inside_range.write("Dockerfile", "EXPOSE 3000-3005\n");

    conflic_cmd_in(inside_range.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Application Port").not());

    let boundary_value = TestWorkspace::new();
    boundary_value.write(".env", "PORT=3000\n");
    boundary_value.write("Dockerfile", "EXPOSE 3000-3005\n");

    conflic_cmd_in(boundary_value.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .success()
        .stdout(predicate::str::contains("Application Port").not());
}

#[test]
fn test_cli_port_ranges_still_report_values_outside_the_range() {
    let workspace = TestWorkspace::new();
    workspace.write(".env", "PORT=3006\n");
    workspace.write("Dockerfile", "EXPOSE 3000-3005\n");

    conflic_cmd_in(workspace.root())
        .arg(".")
        .arg("--no-color")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Application Port"))
        .stdout(predicate::str::contains("3006"))
        .stdout(predicate::str::contains("3000-3005"));
}
