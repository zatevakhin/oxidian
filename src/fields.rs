use std::collections::BTreeMap;

use crate::Error;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FieldValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    List(Vec<FieldValue>),
    Object(BTreeMap<String, FieldValue>),
}

pub type FieldMap = BTreeMap<String, FieldValue>;

pub fn normalize_field_key(key: &str) -> Option<String> {
    let k = key.trim();
    if k.is_empty() {
        return None;
    }
    Some(k.to_lowercase())
}

pub fn merge_field(map: &mut FieldMap, key: String, value: FieldValue) {
    let Some(existing) = map.get_mut(&key) else {
        map.insert(key, value);
        return;
    };

    match existing {
        FieldValue::List(items) => items.push(value),
        _ => {
            let old = std::mem::replace(existing, FieldValue::Null);
            *existing = FieldValue::List(vec![old, value]);
        }
    }
}

pub fn inline_value_to_field_value(raw: &str) -> FieldValue {
    let s = raw.trim();
    if s.is_empty() {
        return FieldValue::Null;
    }

    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "true" => return FieldValue::Bool(true),
        "false" => return FieldValue::Bool(false),
        "null" | "none" => return FieldValue::Null,
        _ => {}
    }

    if let Ok(n) = s.parse::<f64>() {
        return FieldValue::Number(n);
    }

    FieldValue::String(s.to_string())
}

pub fn yaml_to_field_value(v: &serde_yaml::Value) -> FieldValue {
    match v {
        serde_yaml::Value::Null => FieldValue::Null,
        serde_yaml::Value::Bool(b) => FieldValue::Bool(*b),
        serde_yaml::Value::Number(n) => FieldValue::Number(n.as_f64().unwrap_or(0.0)),
        serde_yaml::Value::String(s) => FieldValue::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            FieldValue::List(seq.iter().map(yaml_to_field_value).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut out = BTreeMap::new();
            for (k, v) in map {
                let Some(k) = k.as_str().and_then(normalize_field_key) else {
                    continue;
                };
                out.insert(k, yaml_to_field_value(v));
            }
            FieldValue::Object(out)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_field_value(&tagged.value),
    }
}

pub fn extract_top_level_frontmatter_fields(fm: &serde_yaml::Value) -> Result<FieldMap, Error> {
    let mut out = FieldMap::new();
    let Some(map) = fm.as_mapping() else {
        return Ok(out);
    };

    for (k, v) in map {
        let Some(key) = k.as_str().and_then(normalize_field_key) else {
            continue;
        };
        merge_field(&mut out, key, yaml_to_field_value(v));
    }

    Ok(out)
}
