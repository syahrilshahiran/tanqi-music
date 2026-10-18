## Tanqi 0.1.0

The first public release: a local-first music player for Linux and Android that measures each track, suggests an EQ, and follows the lyrics. No account, no tracking.

### Downloads

| File | For |
|---|---|
| `tanqi-music_0.1.0_amd64.deb` | Debian, Ubuntu and friends (64-bit) |
| `tanqi-music_0.1.0_x86_64.AppImage` | Any 64-bit Linux |
| `tanqi-music-0.1.0-debug-arm64.apk` | Android 7+ phones (64-bit ARM) |

**Linux:** the packages don't install their dependencies for you. You need WebKitGTK 4.1 and GTK 3 (on Debian or Ubuntu: `libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3`).

```bash
sudo apt install ./tanqi-music_0.1.0_amd64.deb
# or
chmod +x tanqi-music_0.1.0_x86_64.AppImage && ./tanqi-music_0.1.0_x86_64.AppImage
```

**Android:** open the APK on your phone and allow installs from your browser or file manager when asked. It will ask for permission to read your music and to show notifications.

### What's in it

- Plays your own FLAC, MP3, M4A/AAC and WAV files, grouped into artists, albums and tracks.
- Optional **audio analysis**: loudness, tempo, key, a frequency-balance chart and a suggested EQ you can fine-tune.
- Synced lyrics that follow along, from LRCLIB.
- Optional **AI lyrics analysis** (a deep dive and an English translation) with your own Ollama, or Claude or any OpenAI-compatible service with your own API key.
- A mini player that stays on top, media keys and MPRIS on Linux, lock-screen controls on Android, and a dark and a silver skin.

### Good to know

- **The numbers are a hint, not a measurement.** Tempo, key, loudness and above all the suggested EQ were tuned and checked by ear, on IEMs, by one person. Your mileage may vary, so fine-tune it, lower the strength, or switch it off.
- **The APK is a debug build** (about 120 MB), not a signed release build.
- Only Linux and Android phones (64-bit ARM) have been tried. Windows, macOS and iOS never have.
- With a hosted AI provider, the title, artist and lyrics of songs you analyse are sent to it and may use your credit or quota. The AI is off until you turn it on.

See the [README](https://github.com/syahrilshahiran/tanqi-music#readme) for building from source and how the AI providers are set up.
