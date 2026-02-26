use std::collections::HashMap;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use crate::error::EnjectError;

const EN_PREFIX: &str = "en://";
const GLOBAL_PREFIX: &str = "en://global/";
const EV_COMPAT_PREFIX: &str = "ev://";
const EV_COMPAT_GLOBAL_PREFIX: &str = "ev://global/";

/// A single parsed line from a `.env` file.
#[derive(Debug, PartialEq)]
pub enum EnvLine {
    /// A blank line or comment — preserved as-is.
    Passthrough(String),
    /// `KEY=plain_value` — passed to subprocess unchanged.
    Plain { key: String, value: String },
    /// `KEY=en://secret_name` — resolved from the local store.
    LocalRef { key: String, secret_name: String },
    /// `KEY=en://global/secret_name` — resolved from the global store.
    GlobalRef { key: String, secret_name: String },
}

/// Parse a `.env` template file into a list of `EnvLine` variants.
/// Returns `Err` on any malformed line.
pub fn parse(content: &str) -> Result<Vec<EnvLine>, EnjectError> {
    content
        .lines()
        .map(|line| parse_line(line).map(|(env_line, _)| env_line))
        .collect()
}

/// Parse a `.env` template file from disk.
/// If the file contains legacy `ev://` references, the user is prompted to migrate
/// in place; a `.env.bak` backup is written before any changes are made.
pub fn parse_file(path: &Path) -> Result<Vec<EnvLine>, EnjectError> {
    let content = std::fs::read_to_string(path)?;
    let content = maybe_migrate_env_file(path, &content)?;
    parse(&content)
}

/// If `content` contains legacy `ev://` references, offer to rewrite the file in place.
/// Writes a `.bak` backup before making any changes.
fn maybe_migrate_env_file(path: &Path, content: &str) -> Result<String, EnjectError> {
    let legacy_count = content.matches("ev://").count();
    if legacy_count == 0 {
        return Ok(content.to_string());
    }

    if !std::io::stdin().is_terminal() {
        println!(
            "Warning: {} contains {} legacy ev:// reference(s). Update to en:// to silence this warning.",
            path.display(),
            legacy_count
        );
        return Ok(content.to_string());
    }

    println!(
        "Warning: {} contains {} legacy ev:// reference(s).",
        path.display(),
        legacy_count
    );
    print!(
        "Update ev:// to en://? A backup will be saved to {}.bak [y/N]: ",
        path.display()
    );
    std::io::stdout().flush().map_err(EnjectError::Io)?;

    let mut answer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(EnjectError::Io)?;

    if !answer.trim().eq_ignore_ascii_case("y") {
        println!("Skipping. Update ev:// to en:// to silence this warning.");
        return Ok(content.to_string());
    }

    // Write backup
    let mut bak_name = path.file_name().unwrap_or_default().to_owned();
    bak_name.push(".bak");
    let backup = path.with_file_name(bak_name);
    std::fs::copy(path, &backup).map_err(EnjectError::Io)?;

    // Rewrite atomically
    let new_content = content.replace("ev://", "en://");
    let mut tmp_name = path.file_name().unwrap_or_default().to_owned();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let mut f = std::fs::File::create(&tmp).map_err(EnjectError::Io)?;
        write!(f, "{}", new_content).map_err(EnjectError::Io)?;
        f.sync_all().map_err(EnjectError::Io)?;
    }
    std::fs::rename(&tmp, path).map_err(EnjectError::Io)?;

    println!(
        "Migrated {} (backup at {}).",
        path.display(),
        backup.display()
    );

    Ok(new_content)
}

/// Returns `(EnvLine, is_legacy)` where `is_legacy` is true if the line used the old `ev://` prefix.
fn parse_line(line: &str) -> Result<(EnvLine, bool), EnjectError> {
    let trimmed = line.trim_end();

    // Blank lines and comments pass through
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok((EnvLine::Passthrough(line.to_string()), false));
    }

    // Must have KEY=VALUE form
    let eq_pos = trimmed.find('=').ok_or_else(|| {
        EnjectError::Config(format!("Malformed .env line (no '=' found): {:?}", trimmed))
    })?;

    let key = trimmed[..eq_pos].trim().to_string();
    if key.is_empty() {
        return Err(EnjectError::Config(format!(
            "Malformed .env line (empty key): {:?}",
            trimmed
        )));
    }

    let value = &trimmed[eq_pos + 1..];

    // Current en:// prefixes
    if let Some(secret_name) = value.strip_prefix(GLOBAL_PREFIX) {
        let secret_name = secret_name.to_string();
        if secret_name.is_empty() {
            return Err(EnjectError::Config(format!(
                "Malformed en:// reference (empty secret name): {:?}",
                trimmed
            )));
        }
        return Ok((EnvLine::GlobalRef { key, secret_name }, false));
    }

    if let Some(secret_name) = value.strip_prefix(EN_PREFIX) {
        let secret_name = secret_name.to_string();
        if secret_name.is_empty() {
            return Err(EnjectError::Config(format!(
                "Malformed en:// reference (empty secret name): {:?}",
                trimmed
            )));
        }
        return Ok((EnvLine::LocalRef { key, secret_name }, false));
    }

    // Legacy ev:// prefixes — accepted for backwards compatibility, but flagged
    if let Some(secret_name) = value.strip_prefix(EV_COMPAT_GLOBAL_PREFIX) {
        let secret_name = secret_name.to_string();
        if secret_name.is_empty() {
            return Err(EnjectError::Config(format!(
                "Malformed ev:// reference (empty secret name): {:?}",
                trimmed
            )));
        }
        return Ok((EnvLine::GlobalRef { key, secret_name }, true));
    }

    if let Some(secret_name) = value.strip_prefix(EV_COMPAT_PREFIX) {
        let secret_name = secret_name.to_string();
        if secret_name.is_empty() {
            return Err(EnjectError::Config(format!(
                "Malformed ev:// reference (empty secret name): {:?}",
                trimmed
            )));
        }
        return Ok((EnvLine::LocalRef { key, secret_name }, true));
    }

    Ok((
        EnvLine::Plain {
            key,
            value: value.to_string(),
        },
        false,
    ))
}

/// Resolve all `en://` references using the provided secret maps.
/// Returns a `HashMap<key, resolved_value>` for all non-comment lines.
/// Hard-errors if any `en://` reference cannot be resolved.
pub fn resolve(
    lines: &[EnvLine],
    local_secrets: &HashMap<String, String>,
    global_secrets: &HashMap<String, String>,
) -> Result<HashMap<String, String>, EnjectError> {
    let mut env = HashMap::new();

    for line in lines {
        match line {
            EnvLine::Passthrough(_) => {}
            EnvLine::Plain { key, value } => {
                env.insert(key.clone(), value.clone());
            }
            EnvLine::LocalRef { key, secret_name } => {
                let val = local_secrets
                    .get(secret_name)
                    .ok_or_else(|| EnjectError::SecretNotFound(secret_name.clone()))?;
                env.insert(key.clone(), val.clone());
            }
            EnvLine::GlobalRef { key, secret_name } => {
                let val = global_secrets.get(secret_name).ok_or_else(|| {
                    EnjectError::SecretNotFound(format!("global/{}", secret_name))
                })?;
                env.insert(key.clone(), val.clone());
            }
        }
    }

    Ok(env)
}

/// Rewrite a parsed env template, replacing `KEY=plain_value` lines with `KEY=en://key_name`
/// for any key that appears in `to_templatize`. Used by `enject import`.
pub fn templatize(lines: &[EnvLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            EnvLine::Passthrough(s) => s.clone(),
            EnvLine::Plain { key, value: _ } => format!("{}=en://{}", key, key),
            EnvLine::LocalRef { key, secret_name } => format!("{}=en://{}", key, secret_name),
            EnvLine::GlobalRef { key, secret_name } => {
                format!("{}=en://global/{}", key, secret_name)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_local(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_plain_value_passthrough() {
        let lines = parse("PORT=3000").unwrap();
        assert_eq!(
            lines[0],
            EnvLine::Plain {
                key: "PORT".into(),
                value: "3000".into()
            }
        );
    }

    #[test]
    fn test_ev_ref_parsed_correctly() {
        let lines = parse("DATABASE_URL=en://database_url").unwrap();
        assert_eq!(
            lines[0],
            EnvLine::LocalRef {
                key: "DATABASE_URL".into(),
                secret_name: "database_url".into()
            }
        );
    }

    #[test]
    fn test_global_ref_parsed_correctly() {
        let lines = parse("API_KEY=en://global/shared_key").unwrap();
        assert_eq!(
            lines[0],
            EnvLine::GlobalRef {
                key: "API_KEY".into(),
                secret_name: "shared_key".into()
            }
        );
    }

    #[test]
    fn test_comment_is_passthrough() {
        let lines = parse("# this is a comment").unwrap();
        assert_eq!(lines[0], EnvLine::Passthrough("# this is a comment".into()));
    }

    #[test]
    fn test_blank_line_is_passthrough() {
        // A single newline produces one blank line
        let lines = parse("\n").unwrap();
        assert_eq!(lines[0], EnvLine::Passthrough("".into()));
    }

    #[test]
    fn test_empty_content_produces_no_lines() {
        let lines = parse("").unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn test_malformed_line_no_equals_returns_err() {
        let result = parse("MISSING_EQUALS");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_key_returns_err() {
        let result = parse("=value");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_ev_ref_returns_err() {
        let result = parse("KEY=en://");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_local_ref() {
        let lines = parse("DB=en://database_url").unwrap();
        let local = make_local(&[("database_url", "postgres://localhost/db")]);
        let resolved = resolve(&lines, &local, &HashMap::new()).unwrap();
        assert_eq!(resolved["DB"], "postgres://localhost/db");
    }

    #[test]
    fn test_resolve_plain_value() {
        let lines = parse("PORT=3000").unwrap();
        let resolved = resolve(&lines, &HashMap::new(), &HashMap::new()).unwrap();
        assert_eq!(resolved["PORT"], "3000");
    }

    #[test]
    fn test_unknown_ev_ref_returns_err() {
        let lines = parse("DB=en://missing_secret").unwrap();
        let result = resolve(&lines, &HashMap::new(), &HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_global_ref() {
        let lines = parse("KEY=en://global/shared").unwrap();
        let global = make_local(&[("shared", "global-value")]);
        let resolved = resolve(&lines, &HashMap::new(), &global).unwrap();
        assert_eq!(resolved["KEY"], "global-value");
    }

    #[test]
    fn test_mixed_content() {
        let content = "# comment\nPORT=8080\nDB=en://db_url\n";
        let lines = parse(content).unwrap();
        let local = make_local(&[("db_url", "postgres://localhost")]);
        let resolved = resolve(&lines, &local, &HashMap::new()).unwrap();

        assert_eq!(resolved["PORT"], "8080");
        assert_eq!(resolved["DB"], "postgres://localhost");
        assert!(!resolved.contains_key("# comment"));
    }

    #[test]
    fn test_legacy_ev_ref_parsed_correctly() {
        let lines = parse("DATABASE_URL=ev://database_url").unwrap();
        assert_eq!(
            lines[0],
            EnvLine::LocalRef {
                key: "DATABASE_URL".into(),
                secret_name: "database_url".into()
            }
        );
    }

    #[test]
    fn test_legacy_global_ev_ref_parsed_correctly() {
        let lines = parse("API_KEY=ev://global/shared_key").unwrap();
        assert_eq!(
            lines[0],
            EnvLine::GlobalRef {
                key: "API_KEY".into(),
                secret_name: "shared_key".into()
            }
        );
    }

    #[test]
    fn test_empty_legacy_ev_ref_returns_err() {
        let result = parse("KEY=ev://");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_legacy_ev_ref() {
        let lines = parse("DB=ev://database_url").unwrap();
        let local = make_local(&[("database_url", "postgres://localhost/db")]);
        let resolved = resolve(&lines, &local, &HashMap::new()).unwrap();
        assert_eq!(resolved["DB"], "postgres://localhost/db");
    }

    #[test]
    fn test_value_with_equals_sign() {
        // Values that contain '=' must be preserved correctly
        let lines = parse("URL=http://host?foo=bar").unwrap();
        assert_eq!(
            lines[0],
            EnvLine::Plain {
                key: "URL".into(),
                value: "http://host?foo=bar".into()
            }
        );
    }
}
