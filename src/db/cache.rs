//! The response cache and fetched artist pictures.

use super::*;

pub fn list_artist_pictures(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT artist_key, thumbnail_url FROM artist_pictures")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn set_artist_picture(conn: &Connection, artist_key: &str, thumbnail_url: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO artist_pictures (artist_key, thumbnail_url, fetched_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(artist_key) DO UPDATE SET thumbnail_url = excluded.thumbnail_url, fetched_at = excluded.fetched_at",
        params![artist_key, thumbnail_url, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// A cached lookup answer, unless it has expired.
pub fn cache_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM api_cache WHERE key = ?1 AND expires_at > ?2",
        params![key, chrono::Utc::now().timestamp()],
        |r| r.get(0),
    )
    .optional()
}

pub fn cache_set(conn: &Connection, key: &str, value: &str, ttl_secs: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO api_cache (key, value, expires_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, expires_at = excluded.expires_at",
        params![key, value, chrono::Utc::now().timestamp() + ttl_secs],
    )?;
    Ok(())
}

/// Drops expired answers; returns how many.
pub fn cache_prune(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM api_cache WHERE expires_at <= ?1", [chrono::Utc::now().timestamp()])
}

/// (entries, bytes of stored answers) among those still valid.
pub fn cache_stats(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(LENGTH(key) + LENGTH(value)), 0) FROM api_cache WHERE expires_at > ?1",
        [chrono::Utc::now().timestamp()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

pub fn cache_clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM api_cache", [])?;
    Ok(())
}
