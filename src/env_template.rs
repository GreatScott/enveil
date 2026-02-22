use std::collections::HashMap;
use std::path::Path;

use crate::error::EnveilError;

const EV_PREFIX: &str = "ev://";
const GLOBAL_PREFIX: &str = "ev://global/";

/// A single parsed line from a `.env` file.
#[derive(Debug, PartialEq)]
pub enum EnvLine {
    /// A blank line or comment — preserved as-is.
    Passthrough(String),
    /// `KEY=plain_value` — passed to subprocess unchanged.
    Plain { key: String, value: String },
    /// `KEY=ev://secret_name` — resolved from the local store.
    LocalRef { key: String, secret_name: String },
    /// `KEY=ev://global/secret_name` — resolved from the global store.
    GlobalRef { key: String, secret_name: String },
}

/// Parse a `.env` template file into a list of `EnvLine` variants.
/// Returns `Err` on any malformed line.
pub fn parse(content: &str) -> Result<Vec<EnvLine>, EnveilError> {
    content.lines().map(parse_line).collect()
}

/// Parse a `.env` template file from disk.
pub fn parse_file(path: &Path) -> Result<Vec<EnvLine>, EnveilError> {
    let content = std::fs::read_to_string(path)?;
    parse(&content)
}

fn parse_line(line: &str) -> Result<EnvLine, EnveilError> {
    let trimmed = line.trim_end();

    // Blank lines and comments pass through
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(EnvLine::Passthrough(line.to_string()));
    }

    // Must have KEY=VALUE form
    let eq_pos = trimmed.find('=').ok_or_else(|| {
        EnveilError::Config(format!("Malformed .env line (no '=' found): {:?}", trimmed))
    })?;

    let key = trimmed[..eq_pos].trim().to_string();
    if key.is_empty() {
        return Err(EnveilError::Config(format!(
            "Malformed .env line (empty key): {:?}",
            trimmed
        )));
    }

    let value = &trimmed[eq_pos + 1..];

    if let Some(secret_name) = value.strip_prefix(GLOBAL_PREFIX) {
        let secret_name = secret_name.to_string();
        if secret_name.is_empty() {
            return Err(EnveilError::Config(format!(
                "Malformed ev:// reference (empty secret name): {:?}",
                trimmed
            )));
        }
        return Ok(EnvLine::GlobalRef { key, secret_name });
    }

    if let Some(secret_name) = value.strip_prefix(EV_PREFIX) {
        let secret_name = secret_name.to_string();
        if secret_name.is_empty() {
            return Err(EnveilError::Config(format!(
                "Malformed ev:// reference (empty secret name): {:?}",
                trimmed
            )));
        }
        return Ok(EnvLine::LocalRef { key, secret_name });
    }

    Ok(EnvLine::Plain {
        key,
        value: value.to_string(),
    })
}

/// Resolve all `ev://` references using the provided secret maps.
/// Returns a `HashMap<key, resolved_value>` for all non-comment lines.
/// Hard-errors if any `ev://` reference cannot be resolved.
pub fn resolve(
    lines: &[EnvLine],
    local_secrets: &HashMap<String, String>,
    global_secrets: &HashMap<String, String>,
) -> Result<HashMap<String, String>, EnveilError> {
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
                    .ok_or_else(|| EnveilError::SecretNotFound(secret_name.clone()))?;
                env.insert(key.clone(), val.clone());
            }
            EnvLine::GlobalRef { key, secret_name } => {
                let val = global_secrets.get(secret_name).ok_or_else(|| {
                    EnveilError::SecretNotFound(format!("global/{}", secret_name))
                })?;
                env.insert(key.clone(), val.clone());
            }
        }
    }

    Ok(env)
}

/// Rewrite a parsed env template, replacing `KEY=plain_value` lines with `KEY=ev://key_name`
/// for any key that appears in `to_templatize`. Used by `enveil import`.
pub fn templatize(lines: &[EnvLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            EnvLine::Passthrough(s) => s.clone(),
            EnvLine::Plain { key, value: _ } => format!("{}=ev://{}", key, key.to_lowercase()),
            EnvLine::LocalRef { key, secret_name } => format!("{}=ev://{}", key, secret_name),
            EnvLine::GlobalRef { key, secret_name } => {
                format!("{}=ev://global/{}", key, secret_name)
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
    fn test_global_ref_parsed_correctly() {
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
        let result = parse("KEY=ev://");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_local_ref() {
        let lines = parse("DB=ev://database_url").unwrap();
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
        let lines = parse("DB=ev://missing_secret").unwrap();
        let result = resolve(&lines, &HashMap::new(), &HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_global_ref() {
        let lines = parse("KEY=ev://global/shared").unwrap();
        let global = make_local(&[("shared", "global-value")]);
        let resolved = resolve(&lines, &HashMap::new(), &global).unwrap();
        assert_eq!(resolved["KEY"], "global-value");
    }

    #[test]
    fn test_mixed_content() {
        let content = "# comment\nPORT=8080\nDB=ev://db_url\n";
        let lines = parse(content).unwrap();
        let local = make_local(&[("db_url", "postgres://localhost")]);
        let resolved = resolve(&lines, &local, &HashMap::new()).unwrap();

        assert_eq!(resolved["PORT"], "8080");
        assert_eq!(resolved["DB"], "postgres://localhost");
        assert!(!resolved.contains_key("# comment"));
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
