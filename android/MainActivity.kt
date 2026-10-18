package dev.dioxus.main

// Android half of what MPRIS does on desktop:
//  - asks for the audio and notification permissions,
//  - keeps playback alive in the background with a foreground service,
//  - shows it on the lock screen and in the shade through a MediaSession.
//
// Rust talks to this over the webview: `TqAndroid.nowPlaying(json)` goes in (see
// `push_now_playing` in src/ui/player.rs), and button presses come back as
// `window.tqNativeCmd({action, value})`, the same actions MPRIS uses.
// dx copies this file verbatim over the generated MainActivity.kt.

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.net.Uri
import android.provider.Settings
import android.content.BroadcastReceiver
import android.content.IntentFilter
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

typealias BuildConfig = com.tanqi.music.BuildConfig

class MainActivity : WryActivity() {
    // We handle Back ourselves (below); the base class would otherwise walk the webview's history.
    override val handleBackNavigation: Boolean = false

    // A tapped notification brings the app to the front with where it should lead; the page asks for it.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        Native.pendingTarget = intent.getStringExtra("tq_target")
        // Already in front, so no resume follows: tell the page to look.
        Native.emit("appResumed", null)
    }

    override fun onWebViewCreate(webView: WebView) {
        Native.webView = webView
        webView.addJavascriptInterface(Bridge(applicationContext), "TqAndroid")
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Native.pendingTarget = intent?.getStringExtra("tq_target")
        // Back: let the page step back one level (close a sheet, leave a page, go to the Library tab);
        // when it is already at the Library's top level, close the app.
        onBackPressedDispatcher.addCallback(this, object : androidx.activity.OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val w = Native.webView
                if (w == null) { finish(); return }
                w.evaluateJavascript("(function(){ try { return !!(window.tqBack && window.tqBack()); } catch (e) { return false; } })()") { r ->
                    if (r != "true") finish()
                }
            }
        })
        val wanted = ArrayList<String>()
        wanted += if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE
        if (Build.VERSION.SDK_INT >= 33) wanted += Manifest.permission.POST_NOTIFICATIONS
        val missing = wanted.filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) ActivityCompat.requestPermissions(this, missing.toTypedArray(), 1)
    }

    override fun onResume() {
        super.onResume()
        // Back from Settings, say: the page re-checks the permission.
        Native.emit("appResumed", null)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        val audio = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE
        val i = permissions.indexOf(audio)
        if (i < 0) return
        // The startup Scan ran before the answer; ask Rust to Scan again, or to explain.
        if (grantResults[i] == PackageManager.PERMISSION_GRANTED) Native.emit("permissionsGranted", null)
        else Native.emit("permissionDenied", null)
    }
}

/** Shared between the activity, the JS bridge and the service (one process). */
object Native {
    @Volatile var webView: WebView? = null

    /** Where a tapped notification should lead (`eq`, `lyrics:<id>`...), until the page takes it. */
    @Volatile var pendingTarget: String? = null

    /** Runs a snippet in the page. */
    fun js(code: String) {
        webView?.let { w -> w.post { w.evaluateJavascript(code, null) } }
    }

    /** Sends a control request to the page, shaped like Rust's `MprisCommand`. */
    fun emit(action: String, value: Double?) {
        val cmd = JSONObject().put("action", action).put("value", value ?: JSONObject.NULL)
        val js = "window.tqNativeCmd && window.tqNativeCmd($cmd)"
        webView?.let { w -> w.post { w.evaluateJavascript(js, null) } }
    }
}

/** What the page reports; mirrors `NowPlaying` in src/mpris/mod.rs. */
data class State(
    val hasTrack: Boolean = false,
    val title: String = "",
    val artist: String = "",
    val album: String? = null,
    val artUrl: String? = null,
    val durationMs: Long = 0,
    val positionMs: Long = 0,
    val playing: Boolean = false,
    val canNext: Boolean = false,
    val canPrevious: Boolean = false,
)

class Bridge(private val ctx: Context) {
    /** A background job finished (or failed): a normal notification that opens the app. */
    @JavascriptInterface
    fun notify(title: String, body: String, target: String) {
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return
        val nm = ctx.getSystemService(NotificationManager::class.java)
        // HIGH is what makes it pop up on screen (DEFAULT only plays the sound). A channel's importance can not be
        // raised once created, hence the new id.
        nm.createNotificationChannel(NotificationChannel("jobs_popup", "Background jobs", NotificationManager.IMPORTANCE_HIGH))
        // One id per notification: it is also the PendingIntent's request code, so two notifications
        // with different targets do not overwrite each other's.
        val id = (System.currentTimeMillis() and 0x7fffffff).toInt()
        val open = PendingIntent.getActivity(
            ctx, id,
            Intent(ctx, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                .putExtra("tq_target", target),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val n = Notification.Builder(ctx, "jobs_popup")
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        nm.notify(id, n)
    }

    /** The target of the notification that opened the app, once; empty if there is none. */
    @JavascriptInterface
    fun takeTarget(): String {
        val t = Native.pendingTarget
        Native.pendingTarget = null
        return t ?: ""
    }

    @JavascriptInterface
    fun hasAudioPermission(): Boolean {
        val p = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE
        return ContextCompat.checkSelfPermission(ctx, p) == PackageManager.PERMISSION_GRANTED
    }

    /** Android stops showing the prompt after a refusal, so the page sends people to Settings. */
    @JavascriptInterface
    fun openAppSettings() {
        ctx.startActivity(
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${ctx.packageName}"))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }

    @JavascriptInterface
    fun nowPlaying(json: String) {
        val o = JSONObject(json)
        val s = State(
            hasTrack = o.optBoolean("hasTrack"),
            title = o.optString("title"),
            artist = o.optString("artist"),
            album = if (o.isNull("album")) null else o.optString("album"),
            artUrl = if (o.isNull("artUrl")) null else o.optString("artUrl"),
            durationMs = o.optDouble("durationMs", 0.0).toLong(),
            positionMs = o.optDouble("positionMs", 0.0).toLong(),
            playing = o.optBoolean("playing"),
            canNext = o.optBoolean("canGoNext"),
            canPrevious = o.optBoolean("canGoPrevious"),
        )
        PlaybackService.state = s
        if (!s.hasTrack) {
            ctx.stopService(Intent(ctx, PlaybackService::class.java))
        } else if (PlaybackService.running) {
            PlaybackService.instance?.refresh()
        } else if (s.playing) {
            // Only start while playing: a service may not start from the background otherwise.
            ContextCompat.startForegroundService(ctx, Intent(ctx, PlaybackService::class.java))
        }
    }
}

class PlaybackService : Service() {
    private lateinit var session: MediaSession
    private var lastKey = ""
    private var art: Bitmap? = null
    private var artFor: String? = null

    // Audio focus: we play inside the WebView, so we answer focus changes by
    // telling the page what to do. `pausedByFocus` means we should resume on regain.
    private lateinit var audio: AudioManager
    private var focusRequest: AudioFocusRequest? = null
    private var hasFocus = false
    private var pausedByFocus = false
    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_GAIN -> {
                Native.js("window.tqEngine && tqEngine.duck(1)")
                if (pausedByFocus) { pausedByFocus = false; Native.emit("play", null) }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> Native.js("window.tqEngine && tqEngine.duck(0.25)")
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                if (state.playing) { pausedByFocus = true; Native.emit("pause", null) }
            }
            AudioManager.AUDIOFOCUS_LOSS -> {
                // Another player took over for good: stop, and do not resume by ourselves.
                hasFocus = false
                pausedByFocus = false
                if (state.playing) Native.emit("pause", null)
            }
        }
    }

    // Headphones unplugged / Bluetooth gone: do not blast the speaker.
    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (state.playing) Native.emit("pause", null)
        }
    }

    private fun requestFocus() {
        if (hasFocus) return
        val req = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build()
            )
            .setOnAudioFocusChangeListener(focusListener)
            .build()
            .also { focusRequest = it }
        hasFocus = audio.requestAudioFocus(req) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    private fun abandonFocus() {
        focusRequest?.let { audio.abandonAudioFocusRequest(it) }
        hasFocus = false
        pausedByFocus = false
        Native.js("window.tqEngine && tqEngine.duck(1)")
    }

    override fun onCreate() {
        super.onCreate()
        instance = this
        running = true
        audio = getSystemService(AudioManager::class.java)
        ContextCompat.registerReceiver(
            this, noisyReceiver, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED
        )
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW))
        session = MediaSession(this, "Tanqi").apply {
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = Native.emit("play", null)
                override fun onPause() = Native.emit("pause", null)
                override fun onSkipToNext() = Native.emit("next", null)
                override fun onSkipToPrevious() = Native.emit("previous", null)
                override fun onStop() = Native.emit("stop", null)
                override fun onSeekTo(pos: Long) = Native.emit("setPosition", pos / 1000.0)
            })
            isActive = true
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_PLAY_PAUSE -> Native.emit("playPause", null)
            ACTION_NEXT -> Native.emit("next", null)
            ACTION_PREVIOUS -> Native.emit("previous", null)
        }
        refresh(force = true)
        return START_NOT_STICKY
    }

    /** Pushes the latest state to the session; rebuilds the notification only when it would change. */
    fun refresh(force: Boolean = false) {
        val s = state
        // Playing means we hold focus. A pause the user (or focus loss) caused releases it
        // unless we are waiting to resume after a short interruption.
        if (s.playing) requestFocus() else if (!pausedByFocus) abandonFocus()
        val actions = PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_STOP or PlaybackState.ACTION_SEEK_TO or
            (if (s.canNext) PlaybackState.ACTION_SKIP_TO_NEXT else 0L) or
            (if (s.canPrevious) PlaybackState.ACTION_SKIP_TO_PREVIOUS else 0L)
        session.setPlaybackState(
            PlaybackState.Builder()
                .setActions(actions)
                .setState(if (s.playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED, s.positionMs, if (s.playing) 1f else 0f)
                .build()
        )

        val key = "${s.title}|${s.artist}|${s.album}|${s.artUrl}|${s.durationMs}|${s.playing}|${s.canNext}|${s.canPrevious}"
        if (!force && key == lastKey) return
        lastKey = key

        if (s.artUrl != artFor) {
            artFor = s.artUrl
            art = loadArt(s.artUrl)
        }
        session.setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, s.title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, s.artist)
                .putString(MediaMetadata.METADATA_KEY_ALBUM, s.album)
                .putLong(MediaMetadata.METADATA_KEY_DURATION, s.durationMs)
                .putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, art)
                .build()
        )

        val notification = buildNotification(s)
        if (s.playing) {
            try {
                if (Build.VERSION.SDK_INT >= 29) {
                    startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
                } else {
                    startForeground(NOTIFICATION_ID, notification)
                }
            } catch (e: Exception) {
                // Android 12+ can refuse to re-promote from the background; the notification still shows.
                getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
            }
        } else {
            // Paused: stay as a swipe-away notification rather than pinning the app.
            stopForeground(STOP_FOREGROUND_DETACH)
            getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
        }
    }

    private fun buildNotification(s: State): Notification {
        fun action(icon: Int, label: String, act: String): Notification.Action {
            val pi = PendingIntent.getService(
                this, act.hashCode(), Intent(this, PlaybackService::class.java).setAction(act),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            return Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, icon), label, pi).build()
        }
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(s.title)
            .setContentText(s.artist)
            .setLargeIcon(art)
            .setContentIntent(open)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .addAction(action(android.R.drawable.ic_media_previous, "Previous", ACTION_PREVIOUS))
            .addAction(
                if (s.playing) action(android.R.drawable.ic_media_pause, "Pause", ACTION_PLAY_PAUSE)
                else action(android.R.drawable.ic_media_play, "Play", ACTION_PLAY_PAUSE)
            )
            .addAction(action(android.R.drawable.ic_media_next, "Next", ACTION_NEXT))
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView(0, 1, 2))
            .build()
    }

    /** `art_url` is a `file://` URI for a library cover, or an https URL (skipped here). */
    private fun loadArt(url: String?): Bitmap? {
        if (url == null || !url.startsWith("file://")) return null
        return try {
            val path = Uri.parse(url).path ?: return null
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(path, bounds)
            val opts = BitmapFactory.Options().apply { inSampleSize = maxOf(1, maxOf(bounds.outWidth, bounds.outHeight) / 512) }
            BitmapFactory.decodeFile(path, opts)
        } catch (e: Exception) {
            null
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        running = false
        instance = null
        abandonFocus()
        unregisterReceiver(noisyReceiver)
        session.release()
        super.onDestroy()
    }

    companion object {
        @Volatile var state = State()
        @Volatile var running = false
        @Volatile var instance: PlaybackService? = null
        private const val CHANNEL = "playback"
        private const val NOTIFICATION_ID = 1
        const val ACTION_PLAY_PAUSE = "com.tanqi.music.PLAY_PAUSE"
        const val ACTION_NEXT = "com.tanqi.music.NEXT"
        const val ACTION_PREVIOUS = "com.tanqi.music.PREVIOUS"
    }
}
