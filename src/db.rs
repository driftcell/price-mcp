use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use worker::d1::{D1Database, D1PreparedStatement, D1Result};
use worker::wasm_bindgen::JsValue;

/// A small SQLx-like compatibility layer over the Workers D1 binding.
///
/// D1 returns complete result sets rather than streaming rows. JSON columns are
/// stored as TEXT and decoded through serde instead of SQLx's SQLite types.
pub struct Query {
    sql: String,
    bindings: Vec<JsValue>,
    error: Option<String>,
}

pub fn query(sql: &str) -> Query {
    Query {
        sql: sql.to_owned(),
        bindings: Vec::new(),
        error: None,
    }
}

impl Query {
    pub fn bind<T: Serialize>(mut self, value: T) -> Self {
        if self.error.is_none() {
            match to_js_value(&value) {
                Ok(value) => self.bindings.push(value),
                Err(error) => self.error = Some(error),
            }
        }
        self
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    fn prepare(&self, db: &D1Database) -> Result<D1PreparedStatement> {
        if let Some(error) = &self.error {
            bail!("failed to encode D1 binding: {error}");
        }
        db.prepare(&self.sql)
            .bind(&self.bindings)
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub async fn fetch_all<E: ReadExecutor>(self, executor: &E) -> Result<Vec<DbRow>> {
        let result = self
            .prepare(executor.database())?
            .all()
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let rows = result
            .results::<HashMap<String, Value>>()
            .map_err(|error| anyhow!(error.to_string()))?;
        Ok(rows.into_iter().map(DbRow).collect())
    }

    pub async fn fetch_optional<E: ReadExecutor>(self, executor: &E) -> Result<Option<DbRow>> {
        Ok(self.fetch_all(executor).await?.into_iter().next())
    }

    pub async fn fetch_one<E: ReadExecutor>(self, executor: &E) -> Result<DbRow> {
        self.fetch_optional(executor)
            .await?
            .context("query returned no rows")
    }

    /// Queue a write in a D1 batch. No statement is sent until `commit`.
    pub async fn execute(self, batch: &mut WriteBatch) -> Result<PendingWrite> {
        let statement = self.prepare(&batch.db)?;
        let index = batch.statements.len();
        batch.statements.push(statement);
        Ok(PendingWrite { index })
    }
}

pub struct QueryBuilder {
    query: Query,
}

impl QueryBuilder {
    pub fn new(sql: &str) -> Self {
        Self { query: query(sql) }
    }

    pub fn push(&mut self, sql: &str) -> &mut Self {
        self.query.sql.push_str(sql);
        self
    }

    pub fn push_bind<T: Serialize>(&mut self, value: T) -> &mut Self {
        self.query.sql.push('?');
        if self.query.error.is_none() {
            match to_js_value(&value) {
                Ok(value) => self.query.bindings.push(value),
                Err(error) => self.query.error = Some(error),
            }
        }
        self
    }

    pub fn build(self) -> Query {
        self.query
    }
}

fn to_js_value<T: Serialize + ?Sized>(value: &T) -> std::result::Result<JsValue, String> {
    let serializer =
        worker::d1::serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true);
    value
        .serialize(&serializer)
        .map_err(|error| error.to_string())
}

#[derive(Debug)]
pub struct DbRow(HashMap<String, Value>);

/// Keeps the existing row-mapping code readable without SQLx's `Row` trait.
pub trait Row {
    fn get<T, I>(&self, name: I) -> T
    where
        T: DeserializeOwned,
        I: AsRef<str>;
}

impl Row for DbRow {
    fn get<T, I>(&self, name: I) -> T
    where
        T: DeserializeOwned,
        I: AsRef<str>,
    {
        let name = name.as_ref();
        let value = self
            .0
            .get(name)
            .unwrap_or_else(|| panic!("D1 row has no column '{name}'"));
        serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("failed to decode D1 column '{name}': {error}"))
    }
}

pub trait ReadExecutor {
    fn database(&self) -> &D1Database;
}

impl ReadExecutor for D1Database {
    fn database(&self) -> &D1Database {
        self
    }
}

impl ReadExecutor for Arc<D1Database> {
    fn database(&self) -> &D1Database {
        self.as_ref()
    }
}

/// A set of writes committed atomically by one D1 `batch()` call.
///
/// Reads used to prepare the batch run immediately against D1. They do not see
/// queued writes and are not part of an interactive transaction snapshot.
pub struct WriteBatch {
    db: Arc<D1Database>,
    statements: Vec<D1PreparedStatement>,
}

impl WriteBatch {
    pub fn new(db: Arc<D1Database>) -> Self {
        Self {
            db,
            statements: Vec::new(),
        }
    }

    pub async fn commit(self) -> Result<Vec<D1Result>> {
        if self.statements.is_empty() {
            return Ok(Vec::new());
        }
        let results = self
            .db
            .batch(self.statements)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        for result in &results {
            if !result.success() {
                bail!(
                    result
                        .error()
                        .unwrap_or_else(|| "D1 batch failed".to_owned())
                );
            }
        }
        Ok(results)
    }
}

impl ReadExecutor for WriteBatch {
    fn database(&self) -> &D1Database {
        self.db.as_ref()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PendingWrite {
    index: usize,
}

impl PendingWrite {
    pub fn last_insert_rowid(self) -> PendingId {
        PendingId { index: self.index }
    }

    pub fn rows_affected(self, results: &[D1Result]) -> Result<usize> {
        results
            .get(self.index)
            .context("D1 batch result is missing")?
            .meta()
            .map_err(|error| anyhow!(error.to_string()))?
            .and_then(|meta| meta.changes)
            .context("D1 result has no changes metadata")
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PendingId {
    index: usize,
}

impl PendingId {
    pub fn resolve(self, results: &[D1Result]) -> Result<i64> {
        results
            .get(self.index)
            .context("D1 batch result is missing")?
            .meta()
            .map_err(|error| anyhow!(error.to_string()))?
            .and_then(|meta| meta.last_row_id)
            .context("D1 insert result has no last_row_id metadata")
    }
}

pub fn resolve_ids(ids: Vec<PendingId>, results: &[D1Result]) -> Result<Vec<i64>> {
    ids.into_iter().map(|id| id.resolve(results)).collect()
}
