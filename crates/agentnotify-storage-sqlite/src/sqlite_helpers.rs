use agentnotify_application::StoreError;
use rusqlite::{Connection, Params, Row};

use crate::migrations::storage_error;

pub(crate) fn map_write_error(context: &str, error: rusqlite::Error) -> StoreError {
    if matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::ConstraintViolation)
    ) {
        StoreError::conflict("store_conflict", context)
    } else {
        storage_error(context, error)
    }
}

pub(crate) fn query_optional<T, F, P>(
    connection: &Connection,
    sql: &str,
    params: P,
    mapper: F,
) -> Result<Option<T>, StoreError>
where
    F: FnOnce(&Row<'_>) -> Result<T, StoreError>,
    P: Params,
{
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| storage_error("准备 SQLite 查询失败", error))?;
    let mut rows = statement
        .query(params)
        .map_err(|error| storage_error("执行 SQLite 查询失败", error))?;
    match rows
        .next()
        .map_err(|error| storage_error("读取 SQLite 查询结果失败", error))?
    {
        Some(row) => mapper(row).map(Some),
        None => Ok(None),
    }
}
