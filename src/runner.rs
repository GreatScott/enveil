use std::collections::HashMap;
use std::process::Command;

use anyhow::{bail, Result};

/// Spawn a subprocess inheriting the parent environment, with `extra_env` layered on top.
/// Blocks until the subprocess exits, then exits the current process with the same code.
pub fn exec(cmd: &[String], extra_env: &HashMap<String, String>) -> Result<()> {
    if cmd.is_empty() {
        bail!("No command provided.");
    }

    let (program, args) = cmd.split_first().unwrap();

    let mut command = Command::new(program);
    command.args(args);

    // Layer .env resolved values on top of the inherited parent environment.
    // std::process::Command inherits the full parent env by default; we just
    // add/override with the resolved secrets.
    for (key, value) in extra_env {
        command.env(key, value);
    }

    let status = command.status()?;

    let code = status.code().unwrap_or(1);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// Helper: run a subprocess and capture its stdout.
    /// Does NOT call std::process::exit — used only for inspection in tests.
    fn run_capture(cmd: &[&str], extra_env: &HashMap<String, String>) -> (i32, String) {
        let program = cmd[0];
        let args = &cmd[1..];

        let mut command = std::process::Command::new(program);
        command.args(args);
        for (k, v) in extra_env {
            command.env(k, v);
        }

        let output = command.output().expect("Failed to run command");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let code = output.status.code().unwrap_or(1);
        (code, stdout)
    }

    #[test]
    fn test_subprocess_receives_injected_env_var() {
        let mut extra = HashMap::new();
        extra.insert(
            "ENVEIL_TEST_VAR".to_string(),
            "hello-from-enject".to_string(),
        );

        let (code, stdout) = run_capture(&["sh", "-c", "echo $ENVEIL_TEST_VAR"], &extra);
        assert_eq!(code, 0);
        assert!(stdout.trim() == "hello-from-enject");
    }

    #[test]
    fn test_subprocess_inherits_path() {
        // PATH must be inherited so basic commands work
        let extra = HashMap::new();
        let (code, _) = run_capture(&["sh", "-c", "which sh"], &extra);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_subprocess_env_var_not_set_without_injection() {
        let extra = HashMap::new();
        // ENVEIL_TEST_UNSET is not in parent env and not injected
        let (_, stdout) = run_capture(&["sh", "-c", "echo ${ENVEIL_TEST_UNSET:-MISSING}"], &extra);
        assert_eq!(stdout.trim(), "MISSING");
    }

    #[test]
    fn test_injected_var_overrides_parent() {
        // Set a var in the test process env, then override it via extra_env
        std::env::set_var("ENVEIL_OVERRIDE_TEST", "original");
        let mut extra = HashMap::new();
        extra.insert("ENVEIL_OVERRIDE_TEST".to_string(), "overridden".to_string());

        let (code, stdout) = run_capture(&["sh", "-c", "echo $ENVEIL_OVERRIDE_TEST"], &extra);
        assert_eq!(code, 0);
        assert_eq!(stdout.trim(), "overridden");
    }
}
