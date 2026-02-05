use crate::{FieldValue, Tag, TaskStatus, VaultIndex, VaultPath, fields::normalize_field_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    Path,
    Field(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub dir: SortDir,
}

#[derive(Debug, Clone, PartialEq)]
enum Predicate {
    FieldExists { key: String },
    FieldEq { key: String, value: FieldValue },
    FieldContains { key: String, needle: String },
    FieldCmp { key: String, op: CmpOp, rhs: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    path_prefix: Option<String>,
    tag: Option<Tag>,
    predicates: Vec<Predicate>,
    sort: Option<Sort>,
    limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHit {
    pub path: VaultPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskHit {
    pub path: VaultPath,
    pub line: u32,
    pub status: TaskStatus,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskQuery {
    path_prefix: Option<String>,
    status: Option<TaskStatus>,
    contains: Option<String>,
    limit: Option<usize>,
}

impl Query {
    pub fn notes() -> Self {
        Self {
            path_prefix: None,
            tag: None,
            predicates: Vec::new(),
            sort: None,
            limit: None,
        }
    }

    pub fn from_path_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(prefix.into());
        self
    }

    pub fn from_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(Tag(tag
            .into()
            .trim()
            .trim_start_matches('#')
            .to_lowercase()));
        self
    }

    pub fn where_field(self, key: impl AsRef<str>) -> FieldPredicateBuilder {
        FieldPredicateBuilder {
            q: self,
            key: key.as_ref().to_string(),
        }
    }

    pub fn sort_by_path(mut self, dir: SortDir) -> Self {
        self.sort = Some(Sort {
            key: SortKey::Path,
            dir,
        });
        self
    }

    pub fn sort_by_field(mut self, key: impl AsRef<str>, dir: SortDir) -> Self {
        let Some(k) = normalize_field_key(key.as_ref()) else {
            return self;
        };
        self.sort = Some(Sort {
            key: SortKey::Field(k),
            dir,
        });
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub(crate) fn execute(&self, index: &VaultIndex) -> Vec<QueryHit> {
        let mut candidates: Vec<VaultPath> = if let Some(tag) = &self.tag {
            index.files_with_tag(tag).cloned().collect()
        } else {
            index.notes_iter_paths().map(|p| p.clone()).collect()
        };

        if let Some(prefix) = &self.path_prefix {
            candidates.retain(|p| p.as_path().to_string_lossy().starts_with(prefix));
        }

        candidates.retain(|p| {
            let Some(note) = index.note(p) else {
                return false;
            };

            for pred in &self.predicates {
                if !eval_predicate(pred, note) {
                    return false;
                }
            }
            true
        });

        if let Some(sort) = &self.sort {
            sort_candidates(index, &mut candidates, sort);
        }

        if let Some(limit) = self.limit {
            candidates.truncate(limit);
        }

        candidates
            .into_iter()
            .map(|path| QueryHit { path })
            .collect()
    }
}

impl TaskQuery {
    pub fn all() -> Self {
        Self {
            path_prefix: None,
            status: None,
            contains: None,
            limit: None,
        }
    }

    pub fn from_path_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(prefix.into());
        self
    }

    pub fn status(mut self, status: TaskStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn contains_text(mut self, needle: impl Into<String>) -> Self {
        self.contains = Some(needle.into());
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub(crate) fn execute(&self, index: &VaultIndex) -> Vec<TaskHit> {
        let mut out = Vec::new();
        let needle = self.contains.as_deref();

        for (path, note) in index.notes_iter() {
            if let Some(prefix) = &self.path_prefix {
                if !path.as_path().to_string_lossy().starts_with(prefix) {
                    continue;
                }
            }

            for t in &note.tasks {
                if let Some(st) = self.status {
                    if t.status != st {
                        continue;
                    }
                }
                if let Some(n) = needle {
                    if !t.text.contains(n) {
                        continue;
                    }
                }

                out.push(TaskHit {
                    path: t.path.clone(),
                    line: t.line,
                    status: t.status,
                    text: t.text.clone(),
                });
            }
        }

        out.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
        if let Some(limit) = self.limit {
            out.truncate(limit);
        }
        out
    }
}

pub struct FieldPredicateBuilder {
    q: Query,
    key: String,
}

impl FieldPredicateBuilder {
    fn norm_key(&self) -> Option<String> {
        normalize_field_key(&self.key)
    }

    pub fn exists(mut self) -> Query {
        let Some(k) = self.norm_key() else {
            return self.q;
        };
        self.q.predicates.push(Predicate::FieldExists { key: k });
        self.q
    }

    pub fn eq<V: Into<FieldValue>>(mut self, v: V) -> Query {
        let Some(k) = self.norm_key() else {
            return self.q;
        };
        self.q.predicates.push(Predicate::FieldEq {
            key: k,
            value: v.into(),
        });
        self.q
    }

    pub fn contains(mut self, needle: impl Into<String>) -> Query {
        let Some(k) = self.norm_key() else {
            return self.q;
        };
        self.q.predicates.push(Predicate::FieldContains {
            key: k,
            needle: needle.into(),
        });
        self.q
    }

    pub fn gt(self, rhs: f64) -> Query {
        self.cmp(CmpOp::Gt, rhs)
    }

    pub fn gte(self, rhs: f64) -> Query {
        self.cmp(CmpOp::Gte, rhs)
    }

    pub fn lt(self, rhs: f64) -> Query {
        self.cmp(CmpOp::Lt, rhs)
    }

    pub fn lte(self, rhs: f64) -> Query {
        self.cmp(CmpOp::Lte, rhs)
    }

    fn cmp(mut self, op: CmpOp, rhs: f64) -> Query {
        let Some(k) = self.norm_key() else {
            return self.q;
        };
        self.q
            .predicates
            .push(Predicate::FieldCmp { key: k, op, rhs });
        self.q
    }
}

fn eval_predicate(pred: &Predicate, note: &crate::NoteMeta) -> bool {
    let fields = &note.fields;
    match pred {
        Predicate::FieldExists { key } => fields.contains_key(key),
        Predicate::FieldEq { key, value } => match fields.get(key) {
            None => false,
            Some(v) => field_eq(v, value),
        },
        Predicate::FieldContains { key, needle } => match fields.get(key) {
            None => false,
            Some(v) => field_contains(v, needle),
        },
        Predicate::FieldCmp { key, op, rhs } => match fields.get(key) {
            None => false,
            Some(FieldValue::Number(n)) => cmp_num(*n, *rhs, *op),
            Some(FieldValue::List(items)) => items.iter().any(|it| match it {
                FieldValue::Number(n) => cmp_num(*n, *rhs, *op),
                _ => false,
            }),
            _ => false,
        },
    }
}

fn field_eq(a: &FieldValue, b: &FieldValue) -> bool {
    match a {
        FieldValue::List(items) => items.iter().any(|it| it == b),
        _ => a == b,
    }
}

fn field_contains(v: &FieldValue, needle: &str) -> bool {
    match v {
        FieldValue::String(s) => s.contains(needle),
        FieldValue::List(items) => items.iter().any(|it| match it {
            FieldValue::String(s) => s.contains(needle),
            _ => false,
        }),
        _ => false,
    }
}

fn cmp_num(left: f64, right: f64, op: CmpOp) -> bool {
    match op {
        CmpOp::Gt => left > right,
        CmpOp::Gte => left >= right,
        CmpOp::Lt => left < right,
        CmpOp::Lte => left <= right,
    }
}

fn sort_candidates(index: &VaultIndex, paths: &mut [VaultPath], sort: &Sort) {
    match &sort.key {
        SortKey::Path => match sort.dir {
            SortDir::Asc => paths.sort(),
            SortDir::Desc => paths.sort_by(|a, b| b.cmp(a)),
        },
        SortKey::Field(key) => {
            paths.sort_by(|a, b| {
                let ak = index
                    .note(a)
                    .and_then(|n| sort_value_for_field(&n.fields, key));
                let bk = index
                    .note(b)
                    .and_then(|n| sort_value_for_field(&n.fields, key));

                // Always keep missing values last, regardless of direction.
                match (ak, bk) {
                    (None, None) => a.cmp(b),
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (Some(ak), Some(bk)) => match sort.dir {
                        SortDir::Asc => ak.cmp(&bk).then_with(|| a.cmp(b)),
                        SortDir::Desc => bk.cmp(&ak).then_with(|| a.cmp(b)),
                    },
                }
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SortValue {
    Number(i64),
    String(String),
}

fn sort_value_for_field(fields: &crate::FieldMap, key: &str) -> Option<SortValue> {
    match fields.get(key) {
        Some(FieldValue::Number(n)) => Some(SortValue::Number((n * 1_000_000.0) as i64)),
        Some(FieldValue::String(s)) => Some(SortValue::String(s.clone())),
        Some(FieldValue::Bool(b)) => Some(SortValue::Number(if *b { 1 } else { 0 })),
        Some(FieldValue::List(items)) => items.iter().find_map(|it| match it {
            FieldValue::Number(n) => Some(SortValue::Number((n * 1_000_000.0) as i64)),
            FieldValue::String(s) => Some(SortValue::String(s.clone())),
            FieldValue::Bool(b) => Some(SortValue::Number(if *b { 1 } else { 0 })),
            _ => None,
        }),
        _ => None,
    }
}

impl From<&str> for FieldValue {
    fn from(value: &str) -> Self {
        FieldValue::String(value.to_string())
    }
}

impl From<String> for FieldValue {
    fn from(value: String) -> Self {
        FieldValue::String(value)
    }
}

impl From<bool> for FieldValue {
    fn from(value: bool) -> Self {
        FieldValue::Bool(value)
    }
}

impl From<f64> for FieldValue {
    fn from(value: f64) -> Self {
        FieldValue::Number(value)
    }
}

impl From<i64> for FieldValue {
    fn from(value: i64) -> Self {
        FieldValue::Number(value as f64)
    }
}
