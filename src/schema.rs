use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use tracing::{error, info};

use crate::fields::normalize_field_key;
use crate::{Error, FieldMap, FieldValue, Result, Vault, VaultPath};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaSeverity {
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSource {
    File(PathBuf),
    Inline,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SchemaStatus {
    #[default]
    Disabled,
    Loaded {
        source: SchemaSource,
        version: u32,
    },
    Error {
        source: SchemaSource,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolation {
    pub severity: SchemaSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolationRecord {
    pub path: Option<VaultPath>,
    pub violation: SchemaViolation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaReport {
    pub status: SchemaStatus,
    pub errors: usize,
    pub warnings: usize,
    pub violations: Vec<SchemaViolationRecord>,
}

#[derive(Debug, Clone)]
pub struct SchemaState {
    pub status: SchemaStatus,
    pub schema: Option<Schema>,
}

impl SchemaState {
    pub fn disabled() -> Self {
        Self {
            status: SchemaStatus::Disabled,
            schema: None,
        }
    }

    pub fn load(vault: &Vault) -> Self {
        let path = schema_path_for_vault(vault);
        let source = SchemaSource::File(path.clone());
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                info!(path = %path.display(), "schema not found; validation disabled");
                return Self::disabled();
            }
            Err(err) => {
                error!(path = %path.display(), error = %err, "failed to read schema");
                return Self {
                    status: SchemaStatus::Error {
                        source,
                        error: err.to_string(),
                    },
                    schema: None,
                };
            }
        };

        match Schema::from_toml_str(&text) {
            Ok(schema) => {
                info!(path = %path.display(), version = schema.version, "schema loaded");
                Self {
                    status: SchemaStatus::Loaded {
                        source,
                        version: schema.version,
                    },
                    schema: Some(schema),
                }
            }
            Err(err) => {
                error!(path = %path.display(), error = %err, "failed to parse schema");
                Self {
                    status: SchemaStatus::Error {
                        source,
                        error: err.to_string(),
                    },
                    schema: None,
                }
            }
        }
    }

    pub fn from_schema(schema: Schema) -> Self {
        Self {
            status: SchemaStatus::Loaded {
                source: SchemaSource::Inline,
                version: schema.version,
            },
            schema: Some(schema),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Schema {
    pub version: u32,
    pub node: NodeSchema,
    pub predicates: PredicatesSchema,
    pub vault: VaultSchema,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct NodeSchema {
    pub types: Vec<String>,
    #[serde(rename = "type")]
    pub type_def: NodeTypeSchema,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct NodeTypeSchema {
    #[serde(default)]
    pub docs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PredicatesSchema {
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(flatten)]
    pub defs: BTreeMap<String, PredicateDef>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PredicateDef {
    pub description: String,
    pub domain: Vec<String>,
    pub range: Vec<String>,
    #[serde(default)]
    pub inverse: Option<String>,
    #[serde(default)]
    pub symmetric: bool,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct VaultSchema {
    pub layout: VaultLayout,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct VaultLayout {
    #[serde(default)]
    pub allow_other_dirs: bool,
    #[serde(default)]
    pub dirs: Vec<LayoutDir>,
    #[serde(default)]
    pub rules: Vec<LayoutRule>,
    #[serde(default)]
    pub type_rules: Vec<LayoutTypeRule>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LayoutDir {
    pub path: String,
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LayoutRule {
    pub id: String,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(rename = "match")]
    pub match_kind: LayoutMatch,
    pub pattern: String,
    #[serde(default)]
    pub capture_equal: Vec<[usize; 2]>,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
    #[serde(default)]
    pub allow_extensions: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LayoutTypeRule {
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(rename = "match")]
    #[serde(default)]
    pub match_kind: Option<LayoutMatch>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(rename = "type")]
    pub required_type: String,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMatch {
    Relpath,
}

impl Schema {
    pub fn from_toml_str(input: &str) -> Result<Self> {
        let schema: Schema =
            toml::from_str(input).map_err(|err| Error::SchemaToml(err.to_string()))?;
        schema.validate()?;
        Ok(schema)
    }

    pub fn validate_note(
        &self,
        rel: &VaultPath,
        fields: &FieldMap,
        inline_fields: &[(String, String)],
    ) -> Vec<SchemaViolation> {
        let mut out = Vec::new();

        out.extend(self.validate_node_type(fields));
        out.extend(self.validate_predicates(rel, fields, inline_fields));
        out.extend(self.validate_type_rules(rel, fields));

        out
    }

    pub fn validate_layout_for_path(&self, rel: &VaultPath) -> Vec<SchemaViolation> {
        let rel_str = path_to_rel_string(rel.as_path());
        let rel_lower = rel_str.to_ascii_lowercase();
        let mut out = Vec::new();

        if !self.vault.layout.allow_other_dirs {
            let mut allowed = false;
            for dir in &self.vault.layout.dirs {
                let dir_path = dir.path.trim_matches('/').to_ascii_lowercase();
                if rel_lower == dir_path || rel_lower.starts_with(&format!("{dir_path}/")) {
                    allowed = true;
                    break;
                }
            }
            if !allowed {
                out.push(SchemaViolation {
                    severity: SchemaSeverity::Error,
                    code: "layout_dir_disallowed".to_string(),
                    message: format!("path '{rel_str}' is outside allowed directories"),
                });
            }
        }

        for rule in &self.vault.layout.rules {
            if let Some(dir) = &rule.dir {
                let dir_path = dir.trim_matches('/');
                if rel_str != dir_path && !rel_str.starts_with(&format!("{dir_path}/")) {
                    continue;
                }
            }

            if !rule.allow_extensions.is_empty() {
                let ext = rel
                    .as_path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let ext = ext.to_ascii_lowercase();
                let allowed = rule
                    .allow_extensions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&ext));
                if !allowed {
                    out.push(SchemaViolation {
                        severity: rule.severity.clone(),
                        code: "layout_extension".to_string(),
                        message: format!(
                            "path '{rel_str}' extension '{ext}' is not allowed by rule '{}'",
                            rule.id
                        ),
                    });
                    continue;
                }
            }

            match rule.match_kind {
                LayoutMatch::Relpath => {
                    let re = match Regex::new(&rule.pattern) {
                        Ok(r) => r,
                        Err(err) => {
                            out.push(SchemaViolation {
                                severity: SchemaSeverity::Error,
                                code: "layout_rule_invalid".to_string(),
                                message: format!("rule '{}' has invalid regex: {err}", rule.id),
                            });
                            continue;
                        }
                    };

                    let caps = match re.captures(&rel_str) {
                        Some(c) => c,
                        None => {
                            out.push(SchemaViolation {
                                severity: rule.severity.clone(),
                                code: "layout_rule_mismatch".to_string(),
                                message: format!(
                                    "path '{rel_str}' does not match rule '{}'",
                                    rule.id
                                ),
                            });
                            continue;
                        }
                    };

                    for pair in &rule.capture_equal {
                        let left = caps.get(pair[0]).map(|m| m.as_str());
                        let right = caps.get(pair[1]).map(|m| m.as_str());
                        if left.is_none() || right.is_none() || left != right {
                            out.push(SchemaViolation {
                                severity: rule.severity.clone(),
                                code: "layout_capture_mismatch".to_string(),
                                message: format!(
                                    "path '{rel_str}' does not satisfy capture equality for rule '{}'",
                                    rule.id
                                ),
                            });
                            break;
                        }
                    }
                }
            }
        }

        out
    }

    pub fn validate_vault_layout(&self, vault: &Vault) -> Vec<SchemaViolationRecord> {
        let mut out = Vec::new();
        for dir in &self.vault.layout.dirs {
            if !dir.required {
                continue;
            }
            let rel = Path::new(&dir.path);
            let Ok(rel) = VaultPath::try_from(rel) else {
                out.push(SchemaViolationRecord {
                    path: None,
                    violation: SchemaViolation {
                        severity: SchemaSeverity::Error,
                        code: "layout_dir_invalid".to_string(),
                        message: format!("required dir '{}' is not a valid vault path", dir.path),
                    },
                });
                continue;
            };
            let abs = vault.to_abs(&rel);
            if !abs.is_dir() {
                out.push(SchemaViolationRecord {
                    path: Some(rel),
                    violation: SchemaViolation {
                        severity: SchemaSeverity::Error,
                        code: "layout_dir_missing".to_string(),
                        message: format!("required dir '{}' is missing", dir.path),
                    },
                });
            }
        }
        out
    }

    fn validate(&self) -> Result<()> {
        for rule in &self.vault.layout.rules {
            Regex::new(&rule.pattern).map_err(|err| {
                Error::SchemaToml(format!("rule '{}' invalid regex: {err}", rule.id))
            })?;
        }
        for rule in &self.vault.layout.type_rules {
            let has_dir = rule.dir.as_ref().is_some();
            let has_pattern = rule.pattern.as_ref().is_some();
            if has_dir == has_pattern {
                return Err(Error::SchemaToml(
                    "type_rules must set exactly one of dir or pattern".to_string(),
                ));
            }
            if let Some(pattern) = &rule.pattern {
                if rule.match_kind.is_none() {
                    return Err(Error::SchemaToml(
                        "type_rules with pattern must set match".to_string(),
                    ));
                }
                Regex::new(pattern)
                    .map_err(|err| Error::SchemaToml(format!("type_rule invalid regex: {err}")))?;
            }
        }
        Ok(())
    }

    fn validate_node_type(&self, fields: &FieldMap) -> Vec<SchemaViolation> {
        let mut out = Vec::new();
        let Some(value) = fields.get("type") else {
            return out;
        };

        let allowed: Vec<String> = self
            .node
            .types
            .iter()
            .map(|t| t.to_ascii_lowercase())
            .collect();

        let mut types = Vec::new();
        match value {
            FieldValue::String(s) => {
                if !s.trim().is_empty() {
                    types.push(s.trim().to_ascii_lowercase());
                }
            }
            FieldValue::List(items) => {
                for item in items {
                    if let FieldValue::String(s) = item
                        && !s.trim().is_empty()
                    {
                        types.push(s.trim().to_ascii_lowercase());
                    }
                }
            }
            _ => {
                out.push(SchemaViolation {
                    severity: SchemaSeverity::Warn,
                    code: "node_type_invalid".to_string(),
                    message: "frontmatter 'type' must be a string or list of strings".to_string(),
                });
                return out;
            }
        }

        for t in types {
            if !allowed.iter().any(|a| a == &t) {
                out.push(SchemaViolation {
                    severity: SchemaSeverity::Error,
                    code: "node_type_unknown".to_string(),
                    message: format!("node type '{t}' is not allowed"),
                });
            }
        }

        out
    }

    fn validate_predicates(
        &self,
        rel: &VaultPath,
        fields: &FieldMap,
        inline_fields: &[(String, String)],
    ) -> Vec<SchemaViolation> {
        let mut out = Vec::new();
        let note_type = extract_note_type(fields);

        for (raw_key, raw_value) in inline_fields {
            let Some(key) = normalize_field_key(raw_key) else {
                continue;
            };
            if key == "type" {
                continue;
            }

            let (canonical, def) = if let Some(alias) = self.predicates.aliases.get(&key) {
                (alias.as_str(), self.predicates.defs.get(alias))
            } else {
                (key.as_str(), self.predicates.defs.get(&key))
            };

            let Some(def) = def else {
                if value_looks_like_link(raw_value) {
                    out.push(SchemaViolation {
                        severity: SchemaSeverity::Warn,
                        code: "predicate_unknown".to_string(),
                        message: format!(
                            "predicate '{}' is not defined for path '{}'",
                            key,
                            rel.as_str_lossy()
                        ),
                    });
                }
                continue;
            };

            if let Some(note_type) = &note_type
                && !predicate_domain_allows(&def.domain, note_type)
            {
                out.push(SchemaViolation {
                    severity: def.severity.clone(),
                    code: "predicate_domain".to_string(),
                    message: format!(
                        "predicate '{}' not allowed for node type '{}'",
                        canonical, note_type
                    ),
                });
            }
        }

        out
    }

    fn validate_type_rules(&self, rel: &VaultPath, fields: &FieldMap) -> Vec<SchemaViolation> {
        let rel_str = path_to_rel_string(rel.as_path());
        let note_type = extract_note_type(fields);
        let mut out = Vec::new();

        for rule in &self.vault.layout.type_rules {
            let applies = if let Some(dir) = &rule.dir {
                let dir_path = dir.trim_matches('/');
                rel_str == dir_path || rel_str.starts_with(&format!("{dir_path}/"))
            } else if let Some(pattern) = &rule.pattern {
                match rule.match_kind {
                    Some(LayoutMatch::Relpath) => {
                        Regex::new(pattern).is_ok_and(|re| re.is_match(&rel_str))
                    }
                    None => false,
                }
            } else {
                false
            };

            if !applies {
                continue;
            }

            let expected = rule.required_type.trim();
            if expected.is_empty() {
                continue;
            }

            let matches = note_type
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case(expected))
                .unwrap_or(false);
            if !matches {
                out.push(SchemaViolation {
                    severity: rule.severity.clone(),
                    code: "layout_type_mismatch".to_string(),
                    message: format!("path '{rel_str}' requires type '{}'", rule.required_type),
                });
            }
        }

        out
    }
}

fn schema_path_for_vault(vault: &Vault) -> PathBuf {
    vault.root().join(&vault.config().schema_path)
}

fn default_severity() -> SchemaSeverity {
    SchemaSeverity::Warn
}

fn path_to_rel_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn value_looks_like_link(value: &str) -> bool {
    let s = value.trim();
    if s.contains("[[") && s.contains("]]") {
        return true;
    }
    s.contains("](") && s.contains(')')
}

fn extract_note_type(fields: &FieldMap) -> Option<String> {
    let value = fields.get("type")?;
    match value {
        FieldValue::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_ascii_lowercase())
            }
        }
        FieldValue::List(items) => items.iter().find_map(|item| match item {
            FieldValue::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_ascii_lowercase())
                }
            }
            _ => None,
        }),
        _ => None,
    }
}

fn predicate_domain_allows(domain: &[String], note_type: &str) -> bool {
    if domain.iter().any(|d| d == "*") {
        return true;
    }
    domain.iter().any(|d| d.eq_ignore_ascii_case(note_type))
}
