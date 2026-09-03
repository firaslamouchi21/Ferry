use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecretParseError {
    #[error("line {line}: expected KEY=VALUE, found {content:?}")]
    MalformedEnvLine { line: usize, content: String },
    #[error("line {line}: key is empty")]
    EmptyEnvKey { line: usize },
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("JSON root must be an object, not {found}")]
    JsonRootNotObject { found: &'static str },
    #[error("key {key:?}: nested objects/arrays are not supported for selective secret display — flatten before sending")]
    JsonValueNotFlat { key: String },
    #[error("invalid YAML: {0}")]
    InvalidYaml(String),
    #[error("YAML root must be a mapping, not {found}")]
    YamlRootNotMapping { found: &'static str },
    #[error("YAML mapping keys must be strings, found {found}")]
    YamlKeyNotString { found: &'static str },
    #[error("key {key:?}: nested mappings/sequences are not supported for selective secret display — flatten before sending")]
    YamlValueNotFlat { key: String },
}

pub fn parse_env(bytes: &[u8]) -> Result<Vec<(String, String)>, SecretParseError> {
    let text = String::from_utf8_lossy(bytes);
    let mut pairs = Vec::new();

    for (idx, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);

        let Some((key, value)) = line.split_once('=') else {
            return Err(SecretParseError::MalformedEnvLine {
                line: idx + 1,
                content: raw_line.to_string(),
            });
        };

        let key = key.trim();
        if key.is_empty() {
            return Err(SecretParseError::EmptyEnvKey { line: idx + 1 });
        }

        let value = unquote(value.trim());
        pairs.push((key.to_string(), value));
    }

    Ok(pairs)
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

pub fn parse_json(bytes: &[u8]) -> Result<Vec<(String, String)>, SecretParseError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| SecretParseError::InvalidJson(e.to_string()))?;

    let object = match value {
        serde_json::Value::Object(map) => map,
        other => {
            return Err(SecretParseError::JsonRootNotObject {
                found: json_type_name(&other),
            })
        }
    };

    let mut pairs = Vec::with_capacity(object.len());
    for (key, val) in object {
        let flat = match val {
            serde_json::Value::String(s) => s,
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => String::new(),
            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                return Err(SecretParseError::JsonValueNotFlat { key })
            }
        };
        pairs.push((key, flat));
    }

    Ok(pairs)
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

pub fn parse_yaml(bytes: &[u8]) -> Result<Vec<(String, String)>, SecretParseError> {
    let value: serde_norway::Value =
        serde_norway::from_slice(bytes).map_err(|e| SecretParseError::InvalidYaml(e.to_string()))?;

    let mapping = match value {
        serde_norway::Value::Mapping(m) => m,
        other => {
            return Err(SecretParseError::YamlRootNotMapping {
                found: yaml_type_name(&other),
            })
        }
    };

    let mut pairs = Vec::with_capacity(mapping.len());
    for (key, val) in mapping {
        let key = match key {
            serde_norway::Value::String(s) => s,
            other => {
                return Err(SecretParseError::YamlKeyNotString {
                    found: yaml_type_name(&other),
                })
            }
        };
        let flat = match val {
            serde_norway::Value::String(s) => s,
            serde_norway::Value::Number(n) => n.to_string(),
            serde_norway::Value::Bool(b) => b.to_string(),
            serde_norway::Value::Null => String::new(),
            serde_norway::Value::Mapping(_) | serde_norway::Value::Sequence(_) | serde_norway::Value::Tagged(_) => {
                return Err(SecretParseError::YamlValueNotFlat { key })
            }
        };
        pairs.push((key, flat));
    }

    Ok(pairs)
}

fn yaml_type_name(value: &serde_norway::Value) -> &'static str {
    match value {
        serde_norway::Value::Null => "null",
        serde_norway::Value::Bool(_) => "a boolean",
        serde_norway::Value::Number(_) => "a number",
        serde_norway::Value::String(_) => "a string",
        serde_norway::Value::Sequence(_) => "a sequence",
        serde_norway::Value::Mapping(_) => "a mapping",
        serde_norway::Value::Tagged(_) => "a tagged value",
    }
}

pub fn select_keys(pairs: &[(String, String)], selected: &[&str]) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter(|(key, _)| selected.contains(&key.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_typical_dotenv_file() {
        let input = b"# a comment\nFOO=bar\nBAZ=\"quoted value\"\nQUUX='single quoted'\n\nexport EXPORTED=yes\n";
        let pairs = parse_env(input).unwrap();
        assert_eq!(
            pairs,
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "quoted value".to_string()),
                ("QUUX".to_string(), "single quoted".to_string()),
                ("EXPORTED".to_string(), "yes".to_string()),
            ]
        );
    }

    #[test]
    fn empty_value_is_allowed() {
        let pairs = parse_env(b"EMPTY=").unwrap();
        assert_eq!(pairs, vec![("EMPTY".to_string(), String::new())]);
    }

    #[test]
    fn a_line_without_an_equals_sign_is_rejected() {
        let result = parse_env(b"FOO=bar\nnot a valid line\n");
        assert_eq!(
            result,
            Err(SecretParseError::MalformedEnvLine {
                line: 2,
                content: "not a valid line".to_string(),
            })
        );
    }

    #[test]
    fn an_empty_key_is_rejected() {
        let result = parse_env(b"=value");
        assert_eq!(result, Err(SecretParseError::EmptyEnvKey { line: 1 }));
    }

    #[test]
    fn parses_a_flat_json_object() {
        let input = br#"{"FOO": "bar", "PORT": 8080, "DEBUG": true, "EMPTY": null}"#;
        let pairs = parse_json(input).unwrap();
        assert_eq!(pairs.len(), 4);
        assert!(pairs.contains(&("FOO".to_string(), "bar".to_string())));
        assert!(pairs.contains(&("PORT".to_string(), "8080".to_string())));
        assert!(pairs.contains(&("DEBUG".to_string(), "true".to_string())));
        assert!(pairs.contains(&("EMPTY".to_string(), String::new())));
    }

    #[test]
    fn a_non_object_json_root_is_rejected() {
        let result = parse_json(b"[1, 2, 3]");
        assert_eq!(result, Err(SecretParseError::JsonRootNotObject { found: "an array" }));
    }

    #[test]
    fn a_nested_json_value_is_rejected_rather_than_guessed_at() {
        let result = parse_json(br#"{"FOO": {"nested": true}}"#);
        assert_eq!(
            result,
            Err(SecretParseError::JsonValueNotFlat { key: "FOO".to_string() })
        );
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(parse_json(b"not json").is_err());
    }

    #[test]
    fn parses_a_flat_yaml_mapping() {
        let input = b"FOO: bar\nPORT: 8080\nDEBUG: true\nEMPTY: null\n";
        let pairs = parse_yaml(input).unwrap();
        assert_eq!(pairs.len(), 4);
        assert!(pairs.contains(&("FOO".to_string(), "bar".to_string())));
        assert!(pairs.contains(&("PORT".to_string(), "8080".to_string())));
        assert!(pairs.contains(&("DEBUG".to_string(), "true".to_string())));
        assert!(pairs.contains(&("EMPTY".to_string(), String::new())));
    }

    #[test]
    fn a_non_mapping_yaml_root_is_rejected() {
        let result = parse_yaml(b"- one\n- two\n");
        assert_eq!(result, Err(SecretParseError::YamlRootNotMapping { found: "a sequence" }));
    }

    #[test]
    fn a_nested_yaml_value_is_rejected_rather_than_guessed_at() {
        let result = parse_yaml(b"FOO:\n  nested: true\n");
        assert_eq!(
            result,
            Err(SecretParseError::YamlValueNotFlat { key: "FOO".to_string() })
        );
    }

    #[test]
    fn malformed_yaml_is_rejected() {
        assert!(parse_yaml(b"foo: [unterminated").is_err());
    }

    #[test]
    fn select_keys_keeps_only_the_chosen_subset_in_original_order() {
        let pairs = vec![
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
            ("C".to_string(), "3".to_string()),
        ];
        let selected = select_keys(&pairs, &["C", "A"]);
        assert_eq!(
            selected,
            vec![("A".to_string(), "1".to_string()), ("C".to_string(), "3".to_string())]
        );
    }
}
