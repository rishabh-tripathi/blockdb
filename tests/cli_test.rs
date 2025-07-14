use std::process::Command;
use tempfile::TempDir;
use serde_json::Value;
use base64::Engine;

/// CLI integration tests
/// Tests the command-line interface including auth commands

#[test]
fn test_cli_basic_operations() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    // Test basic put operation
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "put", "test_key", "test_value"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Successfully stored"));

    // Test basic get operation
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "get", "test_key"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("test_value"));

    // Test stats operation
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "stats"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("BlockDB Statistics"));

    // Test verify operation
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "verify"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("verified successfully"));
}

#[test]
fn test_cli_auth_operations() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    // Test user creation
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--", 
            "--data-dir", &data_dir,
            "auth", "create-user", "testuser", "testpass123",
            "--permissions", "Read", "Write"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("User 'testuser' created successfully"));

    // Test user login
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "auth", "login", "testuser", "testpass123"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Login successful"));
    assert!(stdout.contains("Session ID"));

    // Test list users
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "auth", "list-users"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("testuser"));

    // Test permission granting
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "auth", "grant-permission", "testuser", "Delete"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Permission 'Delete' granted"));

    // Test permission revoking
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "auth", "revoke-permission", "testuser", "Write"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Permission 'Write' revoked"));
}

#[test]
fn test_cli_collection_operations() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    // Test collection creation
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "collection", "create", "test_collection"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Collection 'test_collection' created"));

    // Test collection list
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "collection", "list"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("test_collection"));

    // Extract collection ID from the output for subsequent operations
    let collection_id = extract_collection_id(&stdout);

    // Test putting data into collection
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "collection", "put", &collection_id, "coll_key", "coll_value"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Successfully stored document"));

    // Test getting data from collection
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "collection", "get", &collection_id, "coll_key"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("coll_value"));

    // Test collection stats
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "collection", "stats", &collection_id
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Document count"));

    // Test collection verify
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "collection", "verify", &collection_id
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("verified successfully"));
}

#[test]
fn test_cli_flush_operations() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    // Put some test data
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "put", "flush_key", "flush_value"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());

    // Verify data exists
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "get", "flush_key"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("flush_value"));

    // Test flush with force flag
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "flush", "--force"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Database flushed successfully"));

    // Verify data is gone
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "get", "flush_key"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Key not found"));
}

#[test]
fn test_cli_base64_encoding() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    let binary_data = vec![0, 1, 2, 3, 255, 128, 64];
    let encoded_key = base64::engine::general_purpose::STANDARD.encode("binary_key");
    let encoded_value = base64::engine::general_purpose::STANDARD.encode(&binary_data);

    // Test put with base64 encoding
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "put", &encoded_key, &encoded_value, "--base64"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());

    // Test get with base64 encoding
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "get", &encoded_key, "--base64"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&encoded_value));
}

#[test]
fn test_cli_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    // Test duplicate key error
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "put", "dup_key", "value1"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());

    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "put", "dup_key", "value2"])
        .output()
        .expect("Failed to execute CLI command");

    // Should fail due to duplicate key
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("already exists") || stderr.contains("DuplicateKey"));

    // Test getting non-existent key
    let output = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "get", "nonexistent"])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Key not found"));

    // Test invalid auth operation
    let output = Command::new("cargo")
        .args(&[
            "run", "--bin", "blockdb-cli", "--",
            "--data-dir", &data_dir,
            "auth", "login", "nonexistent_user", "wrong_pass"
        ])
        .output()
        .expect("Failed to execute CLI command");

    assert!(output.status.success()); // CLI doesn't exit with error, but shows error message
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Login failed"));
}

#[test]
fn test_cli_interactive_mode() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().to_string_lossy();

    // Test interactive mode with scripted input
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "blockdb-cli", "--", "--data-dir", &data_dir, "interactive"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start CLI in interactive mode");

    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    use std::io::Write;
    
    // Send commands to interactive mode
    writeln!(stdin, "put interactive_key interactive_value").unwrap();
    writeln!(stdin, "get interactive_key").unwrap();
    writeln!(stdin, "stats").unwrap();
    writeln!(stdin, "quit").unwrap();

    let output = child.wait_with_output().expect("Failed to read stdout");
    
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("BlockDB Interactive Mode"));
    assert!(stdout.contains("interactive_value"));
    assert!(stdout.contains("BlockDB Statistics"));
    assert!(stdout.contains("Goodbye"));
}

// Helper function to extract collection ID from CLI output
fn extract_collection_id(output: &str) -> String {
    // Look for pattern like "Collection 'name' created with ID: col_xxxxx"
    let lines: Vec<&str> = output.lines().collect();
    for line in lines {
        if line.contains("created with ID:") || line.contains("(col_") {
            // Extract collection ID (col_xxxxx format)
            if let Some(start) = line.find("col_") {
                let id_part = &line[start..];
                if let Some(end) = id_part.find(')').or_else(|| id_part.find(' ')) {
                    return id_part[..end].to_string();
                }
                // If no delimiter found, take the rest of the string
                return id_part.split_whitespace().next().unwrap_or("col_unknown").to_string();
            }
        }
    }
    "col_unknown".to_string() // Fallback
}