use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use tracing::{error, info};

use crate::fields::normalize_field_key;
use crate::{Error, FieldMap, FieldValue, Result, Tag, Vault, VaultPath};

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
    pub scope_id: Option<String>,
    pub rule_id: Option<String>,
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
    #[serde(default)]
    pub inverse: Option<String>,
    #[serde(default)]
    pub symmetric: bool,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct VaultSchema {
    #[serde(default = "default_scope_resolution")]
    pub scope_resolution: ScopeResolution,
    #[serde(default = "default_unscoped")]
    pub unscoped: UnmatchedBehavior,
    #[serde(default)]
    pub deny: Vec<LayoutRule>,
    #[serde(default)]
    pub scopes: Vec<VaultScope>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct VaultScope {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_unmatched")]
    pub unmatched_files: UnmatchedBehavior,
    #[serde(default)]
    pub allow: Vec<LayoutRule>,
    #[serde(default)]
    pub deny: Vec<LayoutRule>,
    #[serde(default)]
    pub inherit_allow: bool,
    #[serde(default)]
    pub inherit_deny: bool,
    #[serde(default)]
    pub inherit_notes: bool,
    #[serde(default)]
    pub kinds: Vec<ScopeKind>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub notes: Option<ScopeNotes>,
    #[serde(default)]
    pub orphan_attachments: Option<SchemaSeverity>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ScopeNotes {
    #[serde(default)]
    pub r#type: Option<ScopeNoteType>,
    #[serde(default)]
    pub require_any: Option<ScopeRequireAny>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ScopeRequireAny {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ScopeNoteType {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LayoutRule {
    pub id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default = "default_severity")]
    pub severity: SchemaSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UnmatchedBehavior {
    Allow,
    Warn,
    Error,
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeResolution {
    MostSpecific,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    Note,
    Attachment,
    Other,
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
        tags: &BTreeSet<Tag>,
    ) -> Vec<SchemaViolation> {
        let mut out = Vec::new();

        out.extend(self.validate_node_type(fields));
        out.extend(self.validate_predicates(rel, fields, inline_fields));
        out.extend(self.validate_scope_note_type(rel, fields));
        out.extend(self.validate_scope_require_any(rel, fields, tags));

        out
    }

    pub fn validate_layout_for_path(&self, vault: &Vault, rel: &VaultPath) -> Vec<SchemaViolation> {
        let rel_str = path_to_rel_string(rel.as_path());
        let mut out = Vec::new();

        for rule in &self.vault.deny {
            if rule_matches(rule, &rel_str) {
                out.push(layout_rule_violation("layout_denied", &rel_str, rule, None));
            }
        }

        let selection = self.scope_selection(&rel_str);
        let Some(selection) = selection else {
            if let Some(severity) = self.vault.unscoped.as_severity() {
                out.push(SchemaViolation {
                    severity,
                    code: "layout_unscoped".to_string(),
                    message: format!("path '{rel_str}' is outside configured scopes"),
                    scope_id: None,
                    rule_id: None,
                });
            }
            return out;
        };

        let scope = selection.scope;
        let scope_id = Some(scope.id.clone());
        let rel_within = strip_scope_prefix(&rel_str, &scope.path);

        let deny_rules = selection.collect_deny();
        for rule in deny_rules {
            if rule_matches(rule, &rel_within) {
                out.push(layout_rule_violation(
                    "layout_denied",
                    &rel_str,
                    rule,
                    Some(scope.id.clone()),
                ));
            }
        }

        if !scope.allows_kind(vault, rel.as_path()) {
            if let Some(severity) = scope.unmatched_files.as_severity() {
                out.push(SchemaViolation {
                    severity,
                    code: "layout_unmatched".to_string(),
                    message: format!("path '{rel_str}' is not allowed by scope '{}'", scope.id),
                    scope_id,
                    rule_id: None,
                });
            }
            return out;
        }

        if !scope.allows_extension(rel.as_path()) {
            if let Some(severity) = scope.unmatched_files.as_severity() {
                out.push(SchemaViolation {
                    severity,
                    code: "layout_unmatched".to_string(),
                    message: format!("path '{rel_str}' is not allowed by scope '{}'", scope.id),
                    scope_id,
                    rule_id: None,
                });
            }
            return out;
        }

        let allow_rules = selection.collect_allow();
        if !allow_rules.is_empty() {
            let allowed = allow_rules
                .iter()
                .any(|rule| rule_matches(rule, &rel_within));
            if !allowed {
                if let Some(severity) = scope.unmatched_files.as_severity() {
                    out.push(SchemaViolation {
                        severity,
                        code: "layout_unmatched".to_string(),
                        message: format!("path '{rel_str}' is not allowed by scope '{}'", scope.id),
                        scope_id: Some(scope.id.clone()),
                        rule_id: None,
                    });
                }
            }
        }

        out
    }

    pub fn validate_vault_layout(&self, vault: &Vault) -> Vec<SchemaViolationRecord> {
        let mut out = Vec::new();
        for scope in &self.vault.scopes {
            if !scope.required {
                continue;
            }
            let rel = Path::new(&scope.path);
            let Ok(rel) = VaultPath::try_from(rel) else {
                out.push(SchemaViolationRecord {
                    path: None,
                    violation: SchemaViolation {
                        severity: SchemaSeverity::Error,
                        code: "layout_dir_invalid".to_string(),
                        message: format!("required scope '{}' has invalid path", scope.path),
                        scope_id: Some(scope.id.clone()),
                        rule_id: None,
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
                        message: format!("required scope '{}' is missing", scope.path),
                        scope_id: Some(scope.id.clone()),
                        rule_id: None,
                    },
                });
            }
        }
        out
    }

    fn validate(&self) -> Result<()> {
        let mut scope_ids = HashSet::new();
        for scope in &self.vault.scopes {
            if scope.id.trim().is_empty() {
                return Err(Error::SchemaToml("scope id must not be empty".to_string()));
            }
            if !scope_ids.insert(scope.id.to_string()) {
                return Err(Error::SchemaToml(format!(
                    "scope id '{}' is duplicated",
                    scope.id
                )));
            }
            if scope.path.trim().is_empty() {
                return Err(Error::SchemaToml(format!(
                    "scope '{}' has empty path",
                    scope.id
                )));
            }
            validate_rules(&scope.allow, "allow")?;
            validate_rules(&scope.deny, "deny")?;
        }
        validate_rules(&self.vault.deny, "deny")?;
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
                    scope_id: None,
                    rule_id: None,
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
                    scope_id: None,
                    rule_id: None,
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
                        scope_id: None,
                        rule_id: None,
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
                    scope_id: None,
                    rule_id: None,
                });
            }
        }

        out
    }

    fn validate_scope_note_type(&self, rel: &VaultPath, fields: &FieldMap) -> Vec<SchemaViolation> {
        let rel_str = path_to_rel_string(rel.as_path());
        let Some(selection) = self.scope_selection(&rel_str) else {
            return Vec::new();
        };

        let scope = selection.scope;
        if !scope.allows_kind_note() {
            return Vec::new();
        }

        let notes = selection.notes();
        let Some(notes) = notes else {
            return Vec::new();
        };
        let Some(note_type_rule) = notes.r#type.clone() else {
            return Vec::new();
        };

        let note_type = extract_note_type(fields);
        if note_type_rule.required && note_type.is_none() {
            return vec![SchemaViolation {
                severity: note_type_rule.severity.clone(),
                code: "note_type_missing".to_string(),
                message: format!("path '{rel_str}' requires a note type"),
                scope_id: Some(scope.id.clone()),
                rule_id: None,
            }];
        }

        if !note_type_rule.allowed.is_empty() {
            if note_type.is_none() {
                return Vec::new();
            }
            let matches = note_type
                .as_deref()
                .map(|t| {
                    note_type_rule
                        .allowed
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(t))
                })
                .unwrap_or(false);
            if !matches {
                return vec![SchemaViolation {
                    severity: note_type_rule.severity,
                    code: "note_type_mismatch".to_string(),
                    message: format!(
                        "path '{rel_str}' requires note type {:?}",
                        note_type_rule.allowed
                    ),
                    scope_id: Some(scope.id.clone()),
                    rule_id: None,
                }];
            }
        }

        Vec::new()
    }

    fn validate_scope_require_any(
        &self,
        rel: &VaultPath,
        fields: &FieldMap,
        tags: &BTreeSet<Tag>,
    ) -> Vec<SchemaViolation> {
        let rel_str = path_to_rel_string(rel.as_path());
        let Some(selection) = self.scope_selection(&rel_str) else {
            return Vec::new();
        };

        let scope = selection.scope;
        if !scope.allows_kind_note() {
            return Vec::new();
        }

        let notes = selection.notes();
        let Some(notes) = notes else {
            return Vec::new();
        };
        let Some(require_any) = notes.require_any.clone() else {
            return Vec::new();
        };

        if require_any.tags.is_empty() && require_any.types.is_empty() {
            return Vec::new();
        }

        let note_type = extract_note_type(fields);
        let mut matches = false;

        if !require_any.types.is_empty() {
            if let Some(note_type) = note_type.as_deref() {
                matches = require_any
                    .types
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(note_type));
            }
        }

        if !matches && !require_any.tags.is_empty() {
            let allowed: HashSet<String> = require_any
                .tags
                .iter()
                .filter_map(|t| normalize_tag_name(t))
                .collect();
            matches = tags.iter().any(|tag| allowed.contains(&tag.0));
        }

        if !matches {
            return vec![SchemaViolation {
                severity: require_any.severity,
                code: "note_require_any_missing".to_string(),
                message: format!(
                    "path '{rel_str}' requires one of tags {:?} or types {:?}",
                    require_any.tags, require_any.types
                ),
                scope_id: Some(scope.id.clone()),
                rule_id: None,
            }];
        }

        Vec::new()
    }

    pub(crate) fn scope_for_path<'a>(&'a self, rel: &VaultPath) -> Option<&'a VaultScope> {
        let rel_str = path_to_rel_string(rel.as_path());
        self.scope_selection(&rel_str).map(|s| s.scope)
    }

    fn scope_selection<'a>(&'a self, rel_str: &str) -> Option<ScopeSelection<'a>> {
        let mut matches = Vec::new();
        for scope in &self.vault.scopes {
            if scope_matches(rel_str, &scope.path) {
                matches.push(scope);
            }
        }

        let Some(scope) = matches
            .iter()
            .max_by_key(|s| normalized_path(&s.path).len())
            .copied()
        else {
            return None;
        };

        let mut ancestors = matches
            .into_iter()
            .filter(|s| s.id != scope.id)
            .collect::<Vec<_>>();
        ancestors.sort_by_key(|s| normalized_path(&s.path).len());

        Some(ScopeSelection { scope, ancestors })
    }
}

struct ScopeSelection<'a> {
    scope: &'a VaultScope,
    ancestors: Vec<&'a VaultScope>,
}

impl<'a> ScopeSelection<'a> {
    fn collect_allow(&self) -> Vec<&'a LayoutRule> {
        let mut out = self.scope.allow.iter().collect::<Vec<_>>();
        if self.scope.inherit_allow {
            for ancestor in &self.ancestors {
                out.extend(ancestor.allow.iter());
            }
        }
        out
    }

    fn collect_deny(&self) -> Vec<&'a LayoutRule> {
        let mut out = self.scope.deny.iter().collect::<Vec<_>>();
        if self.scope.inherit_deny {
            for ancestor in &self.ancestors {
                out.extend(ancestor.deny.iter());
            }
        }
        out
    }

    fn notes(&self) -> Option<ScopeNotes> {
        if self.scope.notes.is_some() {
            return self.scope.notes.clone();
        }
        if !self.scope.inherit_notes {
            return None;
        }
        for ancestor in self.ancestors.iter().rev() {
            if ancestor.notes.is_some() {
                return ancestor.notes.clone();
            }
        }
        None
    }
}

impl VaultScope {
    fn allows_kind(&self, vault: &Vault, rel: &Path) -> bool {
        if self.kinds.is_empty() {
            return true;
        }
        let kind = scope_kind_for_path(vault, rel);
        self.kinds.iter().any(|k| k == &kind)
    }

    fn allows_kind_note(&self) -> bool {
        if self.kinds.is_empty() {
            return true;
        }
        self.kinds.iter().any(|k| k == &ScopeKind::Note)
    }

    fn allows_extension(&self, rel: &Path) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        let ext = rel
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))
    }
}

fn validate_rules(rules: &[LayoutRule], label: &str) -> Result<()> {
    for rule in rules {
        if rule.id.trim().is_empty() {
            return Err(Error::SchemaToml(format!(
                "{label} rule id must not be empty"
            )));
        }
        let mut count = 0;
        if rule.glob.as_ref().is_some_and(|s| !s.trim().is_empty()) {
            count += 1;
        }
        if rule.regex.as_ref().is_some_and(|s| !s.trim().is_empty()) {
            count += 1;
        }
        if rule.template.as_ref().is_some_and(|s| !s.trim().is_empty()) {
            count += 1;
        }
        if count != 1 {
            return Err(Error::SchemaToml(format!(
                "{label} rule '{}' must set exactly one of glob, regex, template",
                rule.id
            )));
        }
        if let Some(pattern) = &rule.regex {
            Regex::new(pattern).map_err(|err| {
                Error::SchemaToml(format!("{label} rule '{}' invalid regex: {err}", rule.id))
            })?;
        }
        if let Some(template) = &rule.template {
            compile_template(template).map_err(|err| {
                Error::SchemaToml(format!(
                    "{label} rule '{}' invalid template: {err}",
                    rule.id
                ))
            })?;
        }
    }
    Ok(())
}

fn scope_matches(rel_str: &str, scope_path: &str) -> bool {
    let scope_path = normalized_path(scope_path);
    if scope_path.is_empty() {
        return false;
    }
    rel_str == scope_path || rel_str.starts_with(&format!("{scope_path}/"))
}

fn strip_scope_prefix(rel_str: &str, scope_path: &str) -> String {
    let scope_path = normalized_path(scope_path);
    if rel_str == scope_path {
        return String::new();
    }
    rel_str
        .strip_prefix(&format!("{scope_path}/"))
        .unwrap_or(rel_str)
        .to_string()
}

fn normalized_path(path: &str) -> String {
    path.trim_matches('/').to_string()
}

fn layout_rule_violation(
    code: &str,
    rel_str: &str,
    rule: &LayoutRule,
    scope_id: Option<String>,
) -> SchemaViolation {
    SchemaViolation {
        severity: rule.severity.clone(),
        code: code.to_string(),
        message: format!("path '{rel_str}' matched rule '{}'", rule.id),
        scope_id,
        rule_id: Some(rule.id.clone()),
    }
}

fn rule_matches(rule: &LayoutRule, rel_str: &str) -> bool {
    if let Some(glob) = &rule.glob {
        return glob_matches(glob, rel_str);
    }
    if let Some(pattern) = &rule.regex {
        return Regex::new(pattern).is_ok_and(|re| re.is_match(rel_str));
    }
    if let Some(template) = &rule.template {
        return template_matches(template, rel_str);
    }
    false
}

fn glob_matches(pattern: &str, rel_str: &str) -> bool {
    let Ok(regex) = glob_to_regex(pattern) else {
        return false;
    };
    regex.is_match(rel_str)
}

fn glob_to_regex(pattern: &str) -> std::result::Result<Regex, regex::Error> {
    let mut regex = String::from("^");
    let segments: Vec<&str> = pattern.split('/').collect();
    let mut prev_globstar = false;
    for (idx, segment) in segments.iter().enumerate() {
        if *segment == "**" {
            if idx > 0 && !prev_globstar {
                regex.push('/');
            }
            if idx == segments.len() - 1 {
                regex.push_str(".*");
            } else {
                regex.push_str("(?:[^/]+/)*");
            }
            prev_globstar = true;
            continue;
        }

        if idx > 0 && !prev_globstar {
            regex.push('/');
        }
        prev_globstar = false;

        let mut chars = segment.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '*' => regex.push_str("[^/]*"),
                '?' => regex.push_str("[^/]"),
                '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                    regex.push('\\');
                    regex.push(ch);
                }
                _ => regex.push(ch),
            }
        }
    }
    regex.push('$');
    Regex::new(&regex)
}

fn template_matches(template: &str, rel_str: &str) -> bool {
    let Ok(compiled) = compile_template(template) else {
        return false;
    };
    let Some(caps) = compiled.regex.captures(rel_str) else {
        return false;
    };
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for (idx, name) in compiled.vars.iter().enumerate() {
        let Some(m) = caps.get(idx + 1) else {
            return false;
        };
        if let Some(existing) = seen.get(name.as_str()) {
            if existing != &m.as_str() {
                return false;
            }
        } else {
            seen.insert(name.as_str(), m.as_str());
        }
    }
    true
}

struct TemplatePattern {
    regex: Regex,
    vars: Vec<String>,
}

fn compile_template(template: &str) -> std::result::Result<TemplatePattern, String> {
    let mut regex = String::from("^");
    let mut vars = Vec::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut token = String::new();
            while let Some(next) = chars.next() {
                if next == '}' {
                    break;
                }
                token.push(next);
            }
            if token.is_empty() || !token.contains(':') {
                return Err("template tokens must be {name:format}".to_string());
            }
            let mut parts = token.splitn(2, ':');
            let name = parts.next().unwrap().trim();
            let fmt = parts.next().unwrap().trim();
            if name.is_empty() || fmt.is_empty() {
                return Err("template tokens must be {name:format}".to_string());
            }
            let re = match fmt {
                "yyyy" => "\\d{4}",
                "mm" | "dd" | "ww" => "\\d{2}",
                "slug" => "[a-z0-9][a-z0-9-]*",
                _ => return Err(format!("unknown template format '{fmt}'")),
            };
            regex.push('(');
            regex.push_str(re);
            regex.push(')');
            vars.push(name.to_string());
            continue;
        }

        match ch {
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }

    regex.push('$');
    let regex = Regex::new(&regex).map_err(|err| err.to_string())?;
    Ok(TemplatePattern { regex, vars })
}

fn schema_path_for_vault(vault: &Vault) -> PathBuf {
    vault.root().join(&vault.config().schema_path)
}

fn default_severity() -> SchemaSeverity {
    SchemaSeverity::Warn
}

fn default_scope_resolution() -> ScopeResolution {
    ScopeResolution::MostSpecific
}

fn default_unscoped() -> UnmatchedBehavior {
    UnmatchedBehavior::Allow
}

fn default_unmatched() -> UnmatchedBehavior {
    UnmatchedBehavior::Warn
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

fn normalize_tag_name(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix('#') {
        s = rest;
    }
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.trim_matches('/');
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.to_lowercase())
}

fn scope_kind_for_path(vault: &Vault, rel: &Path) -> ScopeKind {
    let ext = rel.extension().and_then(|s| s.to_str()).unwrap_or("");
    if vault
        .config()
        .note_extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(ext))
    {
        return ScopeKind::Note;
    }
    if vault
        .config()
        .attachment_extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(ext))
    {
        return ScopeKind::Attachment;
    }
    ScopeKind::Other
}

impl UnmatchedBehavior {
    fn as_severity(&self) -> Option<SchemaSeverity> {
        match self {
            UnmatchedBehavior::Allow | UnmatchedBehavior::Ignore => None,
            UnmatchedBehavior::Warn => Some(SchemaSeverity::Warn),
            UnmatchedBehavior::Error => Some(SchemaSeverity::Error),
        }
    }
}
