use std::fs::File;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use lru::LruCache;
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;
use tracing::{debug, info};
use tract_onnx::prelude::tract_ndarray as ndarray;
use tract_onnx::prelude::{
    DatumType, Framework, InferenceFact, InferenceModelExt, Tensor, TypedModel, TypedRunnableModel,
    tvec,
};

use crate::{Error, Result, Vault};

pub(crate) struct EmbeddingModel {
    model: TypedRunnableModel<TypedModel>,
    tokenizer: Tokenizer,
    max_length: usize,
}

const EMBEDDING_MODEL_CACHE_CAP: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    root: PathBuf,
    model_path: PathBuf,
    tokenizer_path: PathBuf,
    model_url: String,
    tokenizer_url: String,
    max_length: usize,
}

impl CacheKey {
    fn for_vault(vault: &Vault) -> Result<Self> {
        let cfg = vault.config();
        let assets = EmbeddingAssets::for_vault(vault)?;
        Ok(Self {
            root: vault.root().to_path_buf(),
            model_path: assets.model_path,
            tokenizer_path: assets.tokenizer_path,
            model_url: cfg.embedding_model_url.clone(),
            tokenizer_url: cfg.embedding_tokenizer_url.clone(),
            max_length: cfg.embedding_max_length.max(8),
        })
    }
}

fn model_cache() -> &'static Mutex<LruCache<CacheKey, Arc<EmbeddingModel>>> {
    static CACHE: OnceLock<Mutex<LruCache<CacheKey, Arc<EmbeddingModel>>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let cap = NonZeroUsize::new(EMBEDDING_MODEL_CACHE_CAP)
            .unwrap_or_else(|| NonZeroUsize::new(1).expect("non-zero"));
        Mutex::new(LruCache::new(cap))
    })
}

fn cache_get_or_insert_with<V, F>(
    cache: &mut LruCache<CacheKey, V>,
    key: CacheKey,
    loader: F,
) -> Result<(V, bool)>
where
    V: Clone,
    F: FnOnce() -> Result<V>,
{
    if let Some(v) = cache.get(&key) {
        return Ok((v.clone(), true));
    }

    let v = loader()?;
    cache.put(key, v.clone());
    Ok((v, false))
}

impl EmbeddingModel {
    pub(crate) fn load(vault: &Vault) -> Result<Self> {
        let cfg = vault.config();
        let assets = EmbeddingAssets::for_vault(vault)?;
        info!(
            model = assets.model_path.display().to_string(),
            tokenizer = assets.tokenizer_path.display().to_string(),
            max_length = cfg.embedding_max_length,
            "loading embedding model"
        );
        assets.ensure_downloaded(cfg)?;
        debug!("embedding assets ready");

        let tokenizer = Tokenizer::from_file(&assets.tokenizer_path)
            .map_err(|e| Error::Embedding(format!("tokenizer load failed: {e}")))?;

        let max_length = cfg.embedding_max_length.max(8);
        let model = tract_onnx::onnx()
            .model_for_path(&assets.model_path)
            .map_err(|e| Error::Embedding(format!("onnx load failed: {e}")))?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(DatumType::I64, tvec!(1, max_length as i64)),
            )
            .map_err(|e| Error::Embedding(format!("onnx input_ids shape failed: {e}")))?
            .with_input_fact(
                1,
                InferenceFact::dt_shape(DatumType::I64, tvec!(1, max_length as i64)),
            )
            .map_err(|e| Error::Embedding(format!("onnx attention_mask shape failed: {e}")))?
            .with_input_fact(
                2,
                InferenceFact::dt_shape(DatumType::I64, tvec!(1, max_length as i64)),
            )
            .map_err(|e| Error::Embedding(format!("onnx token_type_ids shape failed: {e}")))?
            .into_optimized()
            .map_err(|e| Error::Embedding(format!("onnx optimize failed: {e}")))?
            .into_runnable()
            .map_err(|e| Error::Embedding(format!("onnx runnable failed: {e}")))?;

        info!(pooling = "mean", "embedding model loaded");
        Ok(Self {
            model,
            tokenizer,
            max_length,
        })
    }

    pub(crate) fn load_cached(vault: &Vault) -> Result<Arc<Self>> {
        let key = CacheKey::for_vault(vault)?;
        let root = key.root.display().to_string();
        let mut cache = model_cache().lock().unwrap_or_else(|e| e.into_inner());
        let (model, hit) = cache_get_or_insert_with(&mut cache, key, || {
            Ok::<Arc<EmbeddingModel>, Error>(Arc::new(EmbeddingModel::load(vault)?))
        })?;
        if hit {
            debug!(root, "embedding model cache hit");
        } else {
            debug!(root, "embedding model cache miss");
        }
        Ok(model)
    }

    pub(crate) fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| Error::Embedding(format!("tokenize failed: {e}")))?;

        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|v| *v as i64).collect();
        let mut mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|v| *v as i64)
            .collect();
        let mut type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|v| *v as i64).collect();
        let original_len = ids.len();

        if ids.len() > self.max_length {
            ids.truncate(self.max_length);
            mask.truncate(self.max_length);
            type_ids.truncate(self.max_length);
        }
        while ids.len() < self.max_length {
            ids.push(0);
            mask.push(0);
            type_ids.push(0);
        }
        debug!(
            original_len,
            used_len = ids.len(),
            max_length = self.max_length,
            "tokenized input"
        );

        let input_ids_arr = ndarray::Array2::from_shape_vec((1, self.max_length), ids)
            .map_err(|e| Error::Embedding(format!("input_ids shape failed: {e}")))?;
        let attention_mask_arr = ndarray::Array2::from_shape_vec((1, self.max_length), mask)
            .map_err(|e| Error::Embedding(format!("attention_mask shape failed: {e}")))?;
        let token_type_ids_arr = ndarray::Array2::from_shape_vec((1, self.max_length), type_ids)
            .map_err(|e| Error::Embedding(format!("token_type_ids shape failed: {e}")))?;

        let input_ids: Tensor = Tensor::from(input_ids_arr);
        let attention_mask: Tensor = Tensor::from(attention_mask_arr.clone());
        let token_type_ids: Tensor = Tensor::from(token_type_ids_arr);

        let outputs = self
            .model
            .run(tvec![
                input_ids.into(),
                attention_mask.clone().into(),
                token_type_ids.into()
            ])
            .map_err(|e| Error::Embedding(format!("onnx run failed: {e}")))?;
        let output = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| Error::Embedding(format!("onnx output type failed: {e}")))?;
        let output = output
            .into_dimensionality::<ndarray::Ix3>()
            .map_err(|e| Error::Embedding(format!("onnx output dims failed: {e}")))?;

        let mask_arr = attention_mask_arr.view();

        let hidden = output.shape()[2];
        debug!(hidden, "embedding output dimension");
        let mut pooled = vec![0f32; hidden];
        let mut count = 0f32;
        for i in 0..self.max_length {
            if mask_arr[[0, i]] == 0 {
                continue;
            }
            for h in 0..hidden {
                pooled[h] += output[[0, i, h]];
            }
            count += 1.0;
        }
        if count == 0.0 {
            return Ok(pooled);
        }
        for v in &mut pooled {
            *v /= count;
        }
        normalize_l2(&mut pooled);
        Ok(pooled)
    }
}

struct EmbeddingAssets {
    model_path: PathBuf,
    tokenizer_path: PathBuf,
}

impl EmbeddingAssets {
    fn for_vault(vault: &Vault) -> Result<Self> {
        let cfg = vault.config();
        let base = if cfg.embedding_cache_dir.is_absolute() {
            cfg.embedding_cache_dir.clone()
        } else {
            vault.root().join(&cfg.embedding_cache_dir)
        };
        let model_path = base.join("all-minilm-l6-v2.onnx");
        let tokenizer_path = base.join("tokenizer.json");
        Ok(Self {
            model_path,
            tokenizer_path,
        })
    }

    fn ensure_downloaded(&self, cfg: &crate::VaultConfig) -> Result<()> {
        ensure_asset(&self.model_path, &cfg.embedding_model_url)?;
        ensure_asset(&self.tokenizer_path, &cfg.embedding_tokenizer_url)?;
        Ok(())
    }
}

fn ensure_asset(path: &Path, url: &str) -> Result<()> {
    if path.exists() {
        debug!(
            path = path.display().to_string(),
            "embedding asset already present"
        );
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }

    info!(
        path = path.display().to_string(),
        url, "downloading embedding asset"
    );
    let start = Instant::now();
    let response = ureq::get(url)
        .call()
        .map_err(|e| Error::Embedding(format!("download failed: {e}")))?;
    let content_length = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut file = File::create(path).map_err(|e| Error::io(path, e))?;
    let bytes = std::io::copy(&mut reader, &mut file).map_err(|e| Error::io(path, e))?;
    file.flush().map_err(|e| Error::io(path, e))?;
    info!(
        path = path.display().to_string(),
        bytes,
        content_length,
        elapsed_ms = start.elapsed().as_millis(),
        "downloaded embedding asset"
    );
    Ok(())
}

pub(crate) fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let out = hasher.finalize();
    hex::encode(out)
}

pub(crate) fn clean_markdown_for_embedding(text: &str) -> String {
    let body = strip_frontmatter(text);
    let mut out = String::new();
    let mut in_fenced = false;

    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fenced = !in_fenced;
            continue;
        }
        if in_fenced {
            continue;
        }

        let mut line_out = String::new();
        let mut i = 0usize;
        let bytes = line.as_bytes();
        while i < bytes.len() {
            if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
                if let Some((inner, end)) = scan_until(line, i + 2, "]]") {
                    let text = extract_link_label(inner);
                    line_out.push_str(&text);
                    line_out.push(' ');
                    i = end + 2;
                    continue;
                }
            }
            if bytes[i] == b'[' {
                if let Some((label, end_label)) = scan_until(line, i + 1, "]") {
                    if end_label + 1 < line.len() && line.as_bytes()[end_label + 1] == b'(' {
                        if let Some((_url, end_url)) = scan_until(line, end_label + 2, ")") {
                            line_out.push_str(label);
                            line_out.push(' ');
                            i = end_url + 1;
                            continue;
                        }
                    }
                }
            }

            let c = line[i..].chars().next().unwrap_or(' ');
            if c.is_ascii_alphanumeric() || c.is_whitespace() {
                line_out.push(c);
            } else {
                line_out.push(' ');
            }
            i += c.len_utf8();
        }

        out.push_str(&line_out);
        out.push(' ');
    }

    normalize_whitespace(&out)
}

fn strip_frontmatter(text: &str) -> &str {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let bytes = rest.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let line_end = match bytes[idx..].iter().position(|b| *b == b'\n') {
            Some(off) => idx + off + 1,
            None => bytes.len(),
        };
        let line = &rest[idx..line_end];
        let line_trim = line.trim_end_matches(['\r', '\n']);
        if line_trim == "---" {
            return &rest[line_end..];
        }
        idx = line_end;
    }
    text
}

fn scan_until<'a>(s: &'a str, start: usize, delim: &str) -> Option<(&'a str, usize)> {
    s[start..]
        .find(delim)
        .map(|off| (&s[start..start + off], start + off))
}

fn extract_link_label(inner: &str) -> String {
    let mut text = inner.trim();
    if let Some((left, right)) = text.split_once('|') {
        text = right.trim().if_empty_then(left.trim());
    }
    if let Some((left, _)) = text.split_once('#') {
        text = left.trim();
    }
    if let Some((left, _)) = text.split_once('^') {
        text = left.trim();
    }
    text.to_string()
}

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

fn normalize_l2(v: &mut [f32]) {
    let mut sum = 0f32;
    for x in v.iter() {
        sum += x * x;
    }
    if sum == 0.0 {
        return;
    }
    let norm = sum.sqrt();
    for x in v.iter_mut() {
        *x /= norm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn key(root: &str, suffix: &str) -> CacheKey {
        CacheKey {
            root: PathBuf::from(root),
            model_path: PathBuf::from(format!("{root}/model{suffix}.onnx")),
            tokenizer_path: PathBuf::from(format!("{root}/tokenizer{suffix}.json")),
            model_url: format!("https://example.com/model{suffix}.onnx"),
            tokenizer_url: format!("https://example.com/tokenizer{suffix}.json"),
            max_length: 128,
        }
    }

    #[test]
    fn cache_get_or_insert_runs_loader_once() {
        let mut cache = LruCache::new(NonZeroUsize::new(2).expect("non-zero"));
        let key = key("/vault", "a");
        let calls = AtomicUsize::new(0);

        let (v1, hit1) = cache_get_or_insert_with(&mut cache, key.clone(), || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<usize, Error>(42usize)
        })
        .expect("first insert");
        assert!(!hit1);
        assert_eq!(v1, 42);

        let (v2, hit2) = cache_get_or_insert_with(&mut cache, key, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<usize, Error>(99usize)
        })
        .expect("second insert");
        assert!(hit2);
        assert_eq!(v2, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_eviction_respects_lru_capacity() {
        let mut cache = LruCache::new(NonZeroUsize::new(1).expect("non-zero"));
        let key_a = key("/vault", "a");
        let key_b = key("/vault", "b");

        let _ = cache_get_or_insert_with(&mut cache, key_a.clone(), || Ok::<usize, Error>(1usize))
            .expect("insert a");
        let _ = cache_get_or_insert_with(&mut cache, key_b.clone(), || Ok::<usize, Error>(2usize))
            .expect("insert b");

        assert!(!cache.contains(&key_a));
        assert!(cache.contains(&key_b));
    }
}

trait IfEmptyThen {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl IfEmptyThen for str {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}
