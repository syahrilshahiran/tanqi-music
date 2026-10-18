//! Favorites and playlists.

use super::*;

pub fn list_favorite_ids(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT track_id FROM favorites")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

pub fn set_favorite(conn: &Connection, track_id: &str, favorite: bool) -> rusqlite::Result<()> {
    if favorite {
        conn.execute(
            "INSERT OR IGNORE INTO favorites (track_id, added_at) VALUES (?1, ?2)",
            rusqlite::params![track_id, chrono::Utc::now().to_rfc3339()],
        )?;
    } else {
        conn.execute("DELETE FROM favorites WHERE track_id = ?1", [track_id])?;
    }
    Ok(())
}

pub fn list_playlists(conn: &Connection) -> rusqlite::Result<Vec<Playlist>> {
    let mut lists: Vec<Playlist> = conn
        .prepare("SELECT id, name FROM playlists ORDER BY name COLLATE NOCASE, id")?
        .query_map([], |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, track_ids: vec![] }))?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare("SELECT playlist_id, track_id FROM playlist_tracks ORDER BY playlist_id, position")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, track_id) = row?;
        if let Some(list) = lists.iter_mut().find(|l| l.id == id) {
            list.track_ids.push(track_id);
        }
    }
    Ok(lists)
}

pub fn create_playlist(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
        rusqlite::params![name, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_playlist(conn: &Connection, id: i64, name: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE playlists SET name = ?1 WHERE id = ?2", rusqlite::params![name, id])?;
    Ok(())
}

pub fn delete_playlist(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [id])?;
    conn.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
    Ok(())
}

/// Appends the tracks that aren't already in the playlist, in order.
pub fn add_to_playlist(conn: &Connection, id: i64, track_ids: &[String]) -> rusqlite::Result<()> {
    let mut next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    for track_id in track_ids {
        let present: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2)",
            rusqlite::params![id, track_id],
            |r| r.get(0),
        )?;
        if !present {
            conn.execute(
                "INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, next, track_id],
            )?;
            next += 1;
        }
    }
    Ok(())
}

pub fn remove_from_playlist(conn: &Connection, id: i64, position: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND position = ?2", rusqlite::params![id, position])?;
    conn.execute(
        "UPDATE playlist_tracks SET position = position - 1 WHERE playlist_id = ?1 AND position > ?2",
        rusqlite::params![id, position],
    )?;
    Ok(())
}

/// Swaps the entry at `position` with its neighbour (`delta` is -1 or 1).
pub fn move_in_playlist(conn: &Connection, id: i64, position: i64, delta: i64) -> rusqlite::Result<()> {
    let other = position + delta;
    conn.execute(
        "UPDATE playlist_tracks SET position = CASE position WHEN ?2 THEN ?3 ELSE ?2 END
         WHERE playlist_id = ?1 AND position IN (?2, ?3)",
        rusqlite::params![id, position, other],
    )?;
    Ok(())
}
