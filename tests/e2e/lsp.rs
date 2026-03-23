use crate::common::e2e_helpers::*;
use crate::common::TestWorkspace;
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(feature = "lsp")]
use tower_lsp::lsp_types::Url;

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_uses_unsaved_buffer_diagnostics_and_targeted_code_actions() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    let saved_package =
        "{\r\n  // keep this comment\r\n  \"engines\": {\r\n    \"node\": \"20\"\r\n  }\r\n}\r\n";
    let unsaved_package =
        "{\r\n  // keep this comment\r\n  \"engines\": {\r\n    \"node\": \"18\"\r\n  }\r\n}\r\n";
    workspace.write("package.json", saved_package);

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let package_path = workspace.path("package.json");
    let package_uri = Url::from_file_path(&package_path).unwrap().to_string();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    let initialize_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");
    assert!(
        initialize_response.get("result").is_some(),
        "expected initialize response, got {}",
        initialize_response
    );
    assert_eq!(
        initialize_response["result"]["capabilities"]["textDocumentSync"]["change"].as_u64(),
        Some(2),
        "server should advertise incremental text sync"
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": package_uri.clone(),
                        "languageId": "json",
                        "version": 1,
                        "text": saved_package
                    }
                }
            })
            .to_string(),
        );
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {
                        "uri": package_uri.clone(),
                        "version": 2
                    },
                    "contentChanges": [
                        {
                            "text": unsaved_package
                        }
                    ]
                }
            })
            .to_string(),
        );
    }

    let diagnostics_message = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str())
            == Some("textDocument/publishDiagnostics")
            && json
                .get("params")
                .and_then(|params| params.get("uri"))
                .and_then(|uri| uri.as_str())
                .is_some_and(|uri| lsp_uri_matches_path(uri, &package_path))
            && json
                .get("params")
                .and_then(|params| params.get("diagnostics"))
                .and_then(|diagnostics| diagnostics.as_array())
                .is_some_and(|diagnostics| !diagnostics.is_empty())
    })
    .expect("publishDiagnostics for unsaved package.json should exist");

    let diagnostics = diagnostics_message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics should be an array");
    let diagnostic_message = diagnostics[0]["message"]
        .as_str()
        .expect("diagnostic message should be a string");
    assert!(
        diagnostic_message.contains("18"),
        "expected diagnostic to reflect unsaved content, got {}",
        diagnostic_message
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": package_uri.clone() },
                    "range": {
                        "start": { "line": 3, "character": 0 },
                        "end": { "line": 3, "character": 100 }
                    },
                    "context": { "diagnostics": diagnostics }
                }
            })
            .to_string(),
        );
    }

    let code_action_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(2)
    })
    .expect("code action response should exist");

    let actions = code_action_response["result"]
        .as_array()
        .expect("code action result should be an array");
    assert!(!actions.is_empty(), "expected at least one code action");

    let edit = &actions[0]["edit"]["changes"][package_uri.as_str()][0];
    assert_eq!(edit["newText"].as_str(), Some("\"20\""));

    let start = &edit["range"]["start"];
    let end = &edit["range"]["end"];
    assert_eq!(start["line"].as_u64(), Some(3));
    assert_eq!(end["line"].as_u64(), Some(3));
    assert_ne!(start["character"].as_u64(), Some(0));

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#);
    }

    let shutdown_response =
        read_lsp_message_matching(&mut reader, |message| message.contains(r#""id":3"#))
            .expect("shutdown response should exist");
    assert!(
        shutdown_response.contains(r#""id":3"#) && shutdown_response.contains(r#""result":null"#),
        "expected shutdown response, got {}",
        shutdown_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_rejects_documents_outside_workspace_root() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");
    workspace.write("package.json", r#"{"engines":{"node":"20"}}"#);

    let outside_dir = tempfile::tempdir().unwrap();
    let outside_path = outside_dir.path().join("package.json");
    std::fs::write(&outside_path, r#"{"engines":{"node":"18"}}"#).unwrap();

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let outside_uri = Url::from_file_path(&outside_path).unwrap().to_string();
    let outside_text = std::fs::read_to_string(&outside_path).unwrap();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("kind=full"))
    })
    .expect("initial full scan stats should be logged");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": outside_uri.clone(),
                        "languageId": "json",
                        "version": 1,
                        "text": outside_text
                    }
                }
            })
            .to_string(),
        );
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": outside_uri.clone() },
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 100 }
                    },
                    "context": { "diagnostics": [] }
                }
            })
            .to_string(),
        );
    }

    let mut rejection_logged = false;
    let mut outside_diagnostics_seen = false;
    let mut code_action_response = None;

    for _ in 0..48 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        match json.get("method").and_then(|method| method.as_str()) {
            Some("window/logMessage") => {
                let log_message = json["params"]["message"].as_str().unwrap_or_default();
                if log_message.contains("outside workspace root") {
                    rejection_logged = true;
                }
            }
            Some("textDocument/publishDiagnostics") => {
                if json["params"]["uri"].as_str() == Some(outside_uri.as_str()) {
                    outside_diagnostics_seen = true;
                }
            }
            _ => {}
        }

        if json.get("id").and_then(|id| id.as_i64()) == Some(2) {
            code_action_response = Some(json);
        }

        if rejection_logged && code_action_response.is_some() {
            break;
        }
    }

    assert!(
        rejection_logged,
        "outside-workspace documents should be rejected with a warning log"
    );
    assert!(
        !outside_diagnostics_seen,
        "outside-workspace documents must not receive diagnostics"
    );

    let code_action_response = code_action_response.expect("code action response should exist");
    assert!(
        code_action_response["result"].is_null(),
        "outside-workspace code actions should be rejected, got {}",
        code_action_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#);
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(3)
    })
    .expect("shutdown response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_reloads_config_after_config_buffer_change() {
    let workspace = TestWorkspace::new();
    workspace.write(".nvmrc", "18\n");
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    let saved_config = "[conflic]\nseverity = \"warning\"\n";
    let updated_config = "[conflic]\nseverity = \"warning\"\nskip_concepts = [\"node-version\"]\n";
    workspace.write(".conflic.toml", saved_config);

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let nvmrc_path = workspace.path(".nvmrc");
    let nvmrc_uri = Url::from_file_path(&nvmrc_path).unwrap().to_string();
    let config_uri = Url::from_file_path(workspace.path(".conflic.toml"))
        .unwrap()
        .to_string();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
    }

    let mut initial_diagnostics = None;
    let mut initial_full_scan_logged = false;
    let mut observed_initial_messages = Vec::new();

    for _ in 0..64 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        if observed_initial_messages.len() < 12 {
            observed_initial_messages.push(message.clone());
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        match json.get("method").and_then(|method| method.as_str()) {
            Some("textDocument/publishDiagnostics")
                if json["params"]["uri"]
                    .as_str()
                    .is_some_and(|uri| lsp_uri_matches_path(uri, &nvmrc_path))
                    && json["params"]["diagnostics"]
                        .as_array()
                        .is_some_and(|diagnostics| !diagnostics.is_empty()) =>
            {
                initial_diagnostics = Some(json);
            }
            Some("window/logMessage")
                if json["params"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("kind=full")) =>
            {
                initial_full_scan_logged = true;
            }
            _ => {}
        }

        if initial_diagnostics.is_some() && initial_full_scan_logged {
            break;
        }
    }

    let initial_diagnostics = initial_diagnostics.unwrap_or_else(|| {
        panic!(
            "initial diagnostics for .nvmrc should exist; observed messages: {:?}",
            observed_initial_messages
        )
    });

    let initial_message = initial_diagnostics["params"]["diagnostics"][0]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        initial_message.contains("20"),
        "expected initial diagnostics to reflect the Dockerfile conflict, got {}",
        initial_message
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": config_uri.clone(),
                        "languageId": "toml",
                        "version": 1,
                        "text": saved_config
                    }
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("kind=full"))
    })
    .expect("opening the config buffer should trigger a full scan");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {
                        "uri": config_uri.clone(),
                        "version": 2
                    },
                    "contentChanges": [
                        {
                            "text": updated_config
                        }
                    ]
                }
            })
            .to_string(),
        );
    }

    let mut config_reload_scan_logged = false;
    let mut cleared_diagnostics = None;
    let mut observed_config_change_messages = Vec::new();

    for _ in 0..64 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        if observed_config_change_messages.len() < 12 {
            observed_config_change_messages.push(message.clone());
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        if json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage") {
            let log_message = json["params"]["message"].as_str().unwrap_or_default();
            assert!(
                !log_message.contains("Failed to reload conflic config")
                    && !log_message.contains("Failed to refresh conflic config"),
                "config reload should not fail: {}",
                log_message
            );
            if log_message.contains("kind=full") {
                config_reload_scan_logged = true;
            }
        }

        if json.get("method").and_then(|method| method.as_str())
            == Some("textDocument/publishDiagnostics")
            && json["params"]["uri"]
                .as_str()
                .is_some_and(|uri| lsp_uri_matches_path(uri, &nvmrc_path))
            && json["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| diagnostics.is_empty())
        {
            cleared_diagnostics = Some(json);
        }

        if config_reload_scan_logged && cleared_diagnostics.is_some() {
            break;
        }
    }

    assert!(
        config_reload_scan_logged,
        "changing the config buffer should trigger a full scan; observed messages: {:?}",
        observed_config_change_messages
    );

    let cleared_diagnostics = cleared_diagnostics.unwrap_or_else(|| {
        panic!(
            "config change should clear stale node-version diagnostics; observed messages: {:?}",
            observed_config_change_messages
        )
    });

    assert_eq!(
        cleared_diagnostics["params"]["diagnostics"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "reloaded config should suppress node-version diagnostics"
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": nvmrc_uri.clone() },
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 100 }
                    },
                    "context": { "diagnostics": [] }
                }
            })
            .to_string(),
        );
    }

    let code_action_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(2)
    })
    .expect("code action response should exist");
    assert!(
        code_action_response["result"].is_null(),
        "reloaded config should also clear cached code actions, got {}",
        code_action_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#);
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(3)
    })
    .expect("shutdown response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_rapid_typing_uses_incremental_targeted_rescans() {
    let workspace = TestWorkspace::new();
    workspace.write("Dockerfile", "FROM node:20-alpine\n");

    let saved_package = "{\"engines\":{\"node\":\"20\"}}\n";
    workspace.write("package.json", saved_package);

    let root_uri = Url::from_file_path(workspace.root()).unwrap().to_string();
    let package_path = workspace.path("package.json");
    let package_uri = Url::from_file_path(&package_path).unwrap().to_string();
    let global_json_path = workspace.path("global.json");

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .env("CONFLIC_LSP_SCAN_STATS", "1")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": serde_json::Value::Null,
                    "rootUri": root_uri.clone(),
                    "capabilities": {}
                }
            })
            .to_string(),
        );
    }

    let initialize_response = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(1)
    })
    .expect("initialize response should exist");
    assert_eq!(
        initialize_response["result"]["capabilities"]["textDocumentSync"]["change"].as_u64(),
        Some(2),
        "server should advertise incremental text sync"
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
    }

    let initial_full_scan = read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("kind=full"))
    })
    .expect("initial full scan stats should be logged");
    assert!(
        initial_full_scan["params"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("parsed_files=2")),
        "expected initial full scan to include the two discovered files, got {}",
        initial_full_scan
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": package_uri.clone(),
                        "languageId": "json",
                        "version": 1,
                        "text": saved_package
                    }
                }
            })
            .to_string(),
        );
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("method").and_then(|method| method.as_str()) == Some("window/logMessage")
            && json["params"]["message"].as_str().is_some_and(|message| {
                message.contains("kind=incremental")
                    && message.contains("changed_files=1")
                    && message.contains("peer_files=1")
            })
    })
    .expect("didOpen should trigger one targeted incremental scan");

    workspace.write("global.json", "{ invalid json");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        for (version, replacement) in [(2, "18"), (3, "17"), (4, "16")] {
            write_lsp_message(
                stdin,
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didChange",
                    "params": {
                        "textDocument": {
                            "uri": package_uri.clone(),
                            "version": version
                        },
                        "contentChanges": [
                            {
                                "range": {
                                    "start": { "line": 0, "character": 20 },
                                    "end": { "line": 0, "character": 22 }
                                },
                                "text": replacement
                            }
                        ]
                    }
                })
                .to_string(),
            );
        }
    }

    let mut package_diagnostics = None;
    let mut incremental_scan_logs = Vec::new();
    let mut observed_incremental_messages = Vec::new();

    for _ in 0..64 {
        let Some(message) = read_lsp_message(&mut reader) else {
            break;
        };
        if observed_incremental_messages.len() < 12 {
            observed_incremental_messages.push(message.clone());
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) else {
            continue;
        };

        match json.get("method").and_then(|method| method.as_str()) {
            Some("textDocument/publishDiagnostics") => {
                let uri = json["params"]["uri"].as_str().unwrap_or_default();
                let diagnostics = json["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();

                assert!(
                    !lsp_uri_matches_path(uri, &global_json_path),
                    "rapid incremental edits should not trigger diagnostics for untouched global.json: {}",
                    json
                );

                if lsp_uri_matches_path(uri, &package_path) && !diagnostics.is_empty() {
                    let message = diagnostics[0]["message"].as_str().unwrap_or_default();
                    if message.contains("16") {
                        package_diagnostics = Some(json.clone());
                    }
                }
            }
            Some("window/logMessage") => {
                let message = json["params"]["message"].as_str().unwrap_or_default();
                if message.contains("kind=incremental") {
                    incremental_scan_logs.push(message.to_string());
                }
            }
            _ => {}
        }

        if package_diagnostics.is_some() && !incremental_scan_logs.is_empty() {
            break;
        }
    }

    let diagnostics_message = package_diagnostics.unwrap_or_else(|| {
        panic!(
            "latest rapid-typing diagnostics should be published; observed messages: {:?}",
            observed_incremental_messages
        )
    });
    let diagnostics = diagnostics_message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics should be an array");
    let diagnostic_message = diagnostics[0]["message"]
        .as_str()
        .expect("diagnostic message should be a string");
    assert!(
        diagnostic_message.contains("16"),
        "expected diagnostics to reflect the final incremental edit, got {}",
        diagnostic_message
    );

    assert_eq!(
        incremental_scan_logs.len(),
        1,
        "rapid typing should coalesce into one debounced incremental scan, got {:?}",
        incremental_scan_logs
    );
    assert!(
        incremental_scan_logs[0].contains("parsed_files=2")
            && incremental_scan_logs[0].contains("changed_files=1")
            && incremental_scan_logs[0].contains("peer_files=1"),
        "incremental scan should rescan only the changed file plus its single concept peer, got {:?}",
        incremental_scan_logs
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    }

    read_lsp_json_message_matching(&mut reader, |json| {
        json.get("id").and_then(|id| id.as_i64()) == Some(2)
    })
    .expect("shutdown response should exist");

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}

#[cfg(feature = "lsp")]
#[test]
fn test_cli_lsp_smoke_initialize_and_exit() {
    let workspace = TestWorkspace::new();

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_conflic"))
        .arg("--lsp")
        .current_dir(workspace.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lsp process should spawn");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = LspMessageReader::spawn(stdout);

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#,
        );
    }

    let initialize_response =
        read_lsp_message_matching(&mut reader, |message| message.contains(r#""id":1"#))
            .expect("initialize response should exist");
    assert!(
        initialize_response.contains(r#""id":1"#) && initialize_response.contains(r#""result""#),
        "expected initialize response, got {}",
        initialize_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(
            stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        );
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    }

    let shutdown_response =
        read_lsp_message_matching(&mut reader, |message| message.contains(r#""id":2"#))
            .expect("shutdown response should exist");
    assert!(
        shutdown_response.contains(r#""id":2"#) && shutdown_response.contains(r#""result":null"#),
        "expected shutdown response, got {}",
        shutdown_response
    );

    {
        let stdin = child.stdin.as_mut().expect("stdin should be piped");
        write_lsp_message(stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("lsp process should exit cleanly");

    assert!(
        output.status.success(),
        "lsp process exited with {:?}",
        output.status.code()
    );
}
