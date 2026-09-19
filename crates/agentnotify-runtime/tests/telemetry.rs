use std::{fs::File, io::Write};

use agentnotify_runtime::{RedactingWriter, redact_sensitive};

#[test]
fn sensitive_values_never_reach_the_log_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runtime.log");
    let mut writer = RedactingWriter::new(File::create(&path).unwrap());
    writer
        .write_all(
            br#"{"token":"token-secret","authorization":"Bearer auth-secret","context_token":"context-secret","message":"body-secret"}
"#,
        )
        .unwrap();
    writer.flush().unwrap();
    drop(writer);

    let contents = std::fs::read_to_string(path).unwrap();
    assert!(!contents.contains("token-secret"));
    assert!(!contents.contains("auth-secret"));
    assert!(!contents.contains("context-secret"));
    assert!(!contents.contains("body-secret"));
    assert!(contents.contains("[REDACTED]"));
}

#[test]
fn tracing_style_fields_are_redacted() {
    let redacted = redact_sensitive("token=abc body=\"private text\" safe=value");
    assert_eq!(
        redacted,
        "token=\"[REDACTED]\" body=\"[REDACTED]\" safe=value"
    );
}
