//! Saved headphone corrections.

use super::*;

pub fn list_headphone_profiles(conn: &Connection) -> rusqlite::Result<Vec<HeadphoneProfile>> {
    conn.prepare("SELECT id, name, preset FROM headphone_profiles ORDER BY name COLLATE NOCASE, id")?
        .query_map([], |r| Ok(HeadphoneProfile { id: r.get(0)?, name: r.get(1)?, preset: r.get(2)? }))?
        .collect()
}

pub fn save_headphone_profile(conn: &Connection, name: &str, preset: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO headphone_profiles (name, preset, created_at) VALUES (?1, ?2, ?3)",
        params![name, preset, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_headphone_profile(conn: &Connection, id: i64, name: &str, preset: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE headphone_profiles SET name = ?1, preset = ?2 WHERE id = ?3", params![name, preset, id])?;
    Ok(())
}

pub fn delete_headphone_profile(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM headphone_profiles WHERE id = ?1", [id])?;
    Ok(())
}

/// The profile in use, remembered across restarts (None: no correction).
pub fn get_active_headphone(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    let v: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'active_headphone'", [], |r| r.get(0))
        .optional()?;
    Ok(v.and_then(|v| v.parse().ok()))
}

pub fn set_active_headphone(conn: &Connection, id: Option<i64>) -> rusqlite::Result<()> {
    match id {
        Some(id) => conn.execute(
            "INSERT INTO settings (key, value) VALUES ('active_headphone', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [id.to_string()],
        )?,
        None => conn.execute("DELETE FROM settings WHERE key = 'active_headphone'", [])?,
    };
    Ok(())
}
