# Favorites and Playlists refer to Tracks by Local reference, outside the Library store

A Favorite or Playlist entry holds only a Track's Local reference, with no database link to the tracks table and no cascade delete. We considered a linked table that drops entries when a Track is removed, but a Scan can briefly drop and re-add a file (a drive not mounted, a folder being moved), and that would silently erase the user's hearts and playlist entries. The cost is that entries can point at Tracks that are no longer in the library, so the UI hides them instead of removing them.
