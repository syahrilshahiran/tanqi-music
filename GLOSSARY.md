# Tanqi Music

A local-first music player: it keeps the user's tracks, groups them into albums, and analyses their audio and lyrics.

## Language

**Track**:
One playable piece of music, found in a Library folder.
_Avoid_: Song, library track

**Library folder**:
A folder on disk the user has added so its audio files become Tracks.
_Avoid_: Root, music folder, watch folder

**Scan**:
Reading a Library folder to add new or changed Tracks and drop Tracks whose files are gone.
_Avoid_: Import, sync, refresh

**Library store**:
The module that owns every Track, Library folder and Scan result, and the grouping of Tracks into albums. It does not own Favorites or Playlists, which only refer to Tracks.
_Avoid_: Library service, track database

**Audio analysis**:
A Track's measured bass, mid and treble levels, loudness, tempo (BPM) and key, with a Suggested EQ made from them.
_Avoid_: EQ analysis

**Optional features**:
Audio analysis and Lyrics (their lookups, analyses and pages). Each is off until chosen in onboarding and can be switched in Settings; the app is a plain music player without them. An install that existed before they became optional keeps them on.
_Avoid_: Modules, plugins

**Suggested EQ**:
The equalisation an Audio analysis proposes for a Track: only the frequencies where the Track departs from what music usually sounds like, eased by the Suggested EQ strength. It is a hint, not a correction to a target.
_Avoid_: Auto EQ, AI EQ, EQ preset

**Reference**:
How music usually sounds: the typical spectrum, and the normal range around it, that a Track's measured spectrum is compared against. It is measured from real albums, not a perfect or flat curve, so "unusual" means unlike most music, not wrong.
_Avoid_: Target curve, ideal curve

**Suggested EQ strength**:
How much of the Suggested EQ is applied, from none to all of it. One setting for every Track.

**Headphone correction**:
A named, fixed correction for one earphone or headphone, applied to every Track under the Suggested EQ. Unlike the Suggested EQ it does not depend on the Track.
_Avoid_: Headphone profile, headphone preset

**Lyrics analysis**:
A Track's lyrics read by the local AI model into a vibe summary, annotations on line ranges, topics and entities.
_Avoid_: Lyrics breakdown, deep dive

**Topic**:
A theme the local AI model names in a Track's lyrics.

**Entity**:
A named person, place or thing the local AI model finds in a Track's lyrics.

When filtering analysed lyrics by several Topics or Entities, a Track matches if it has any of them.

**Lyrics translation**:
A line-by-line English version of a Track's lyrics, produced by the local AI model.
_Avoid_: Lyrics conversion

**Lyrics analyzer**:
The module that produces and saves Lyrics analyses and Lyrics translations, including the check for a saved one and the rule for lyrics not worth analyzing (none found, empty, instrumental).
_Avoid_: Ollama client, lyrics service

**AI provider**:
The service that reads lyrics for the Lyrics analyzer: Ollama (a local or network model), an OpenAI-compatible API, or Claude. Reached with an address, a model and, for the hosted ones, an API key kept in the system keychain.
_Avoid_: backend, LLM server

**Local reference**:
A `local:`-prefixed path naming a file on this machine; it is both a scanned Track's id and the location of its cover.
_Avoid_: Local id, local path, local URL

## Collections

**Favorite**:
A Track the user has hearted. It stays a Favorite while its file is missing from the library, and returns with the file.
_Avoid_: Like, starred track

**Playlist**:
A user-named, ordered list of Tracks that lists each Track at most once. A Track whose file has left the library stays listed but is not shown or played.
_Avoid_: Mix, collection

**Play queue**:
The ordered list of Tracks currently being played; it may repeat a Track. The app remembers it, and the current Track's position, and brings it back paused the next time it opens. Not a Playlist.
_Avoid_: Now-playing list, up next
