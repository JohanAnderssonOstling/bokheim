package se.bokheim.reader

import android.app.PendingIntent
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import java.io.File

class BokheimPlaybackService : MediaSessionService() {
  companion object {
    const val EXTRA_LIBRARY_ID = "bokheim.library_id"
    const val EXTRA_CONTENT_HASH = "bokheim.content_hash"
    const val EXTRA_SOURCE_PATH = "bokheim.source_path"
    private const val PREFERENCES = "bokheim-audiobook-playback"
    private const val SAVE_INTERVAL_MS = 5_000L
  }

  private lateinit var player: ExoPlayer
  private var mediaSession: MediaSession? = null
  private val handler = Handler(Looper.getMainLooper())
  private val preferences by lazy { getSharedPreferences(PREFERENCES, MODE_PRIVATE) }
  private val saveProgress = object : Runnable {
    override fun run() {
      persistPlaybackState()
      if (::player.isInitialized && player.isPlaying) handler.postDelayed(this, SAVE_INTERVAL_MS)
    }
  }

  private val playerListener = object : Player.Listener {
    override fun onEvents(player: Player, events: Player.Events) {
      persistPlaybackState()
      handler.removeCallbacks(saveProgress)
      if (player.isPlaying) handler.postDelayed(saveProgress, SAVE_INTERVAL_MS)
    }

    override fun onPlayerError(error: PlaybackException) {
      persistPlaybackState()
    }
  }

  override fun onCreate() {
    super.onCreate()
    val audioAttributes = AudioAttributes.Builder()
      .setContentType(C.AUDIO_CONTENT_TYPE_SPEECH)
      .setUsage(C.USAGE_MEDIA)
      .build()
    player = ExoPlayer.Builder(this).build().apply {
      setAudioAttributes(audioAttributes, true)
      setHandleAudioBecomingNoisy(true)
      addListener(playerListener)
    }

    val launchIntent = packageManager.getLaunchIntentForPackage(packageName)
    val sessionActivity = launchIntent?.let {
      PendingIntent.getActivity(
        this,
        0,
        it.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    }
    mediaSession = MediaSession.Builder(this, player).apply {
      sessionActivity?.let(::setSessionActivity)
    }.build()
    restorePlaybackState()
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = mediaSession

  override fun onDestroy() {
    handler.removeCallbacks(saveProgress)
    persistPlaybackState()
    mediaSession?.release()
    mediaSession = null
    if (::player.isInitialized) {
      player.removeListener(playerListener)
      player.release()
    }
    super.onDestroy()
  }

  private fun persistPlaybackState() {
    if (!::player.isInitialized) return
    val item = player.currentMediaItem
    if (item == null) {
      preferences.edit().clear().apply()
      return
    }
    val extras = item.mediaMetadata.extras
    preferences.edit()
      .putBoolean("active", true)
      .putString("key", item.mediaId)
      .putString("library_id", extras?.getString(EXTRA_LIBRARY_ID).orEmpty())
      .putString("content_hash", extras?.getString(EXTRA_CONTENT_HASH).orEmpty())
      .putString("source_path", extras?.getString(EXTRA_SOURCE_PATH).orEmpty())
      .putString("title", item.mediaMetadata.title?.toString().orEmpty())
      .putString("author", item.mediaMetadata.artist?.toString().orEmpty())
      .putLong("position_ms", player.currentPosition.coerceAtLeast(0L))
      .putFloat("playback_rate", player.playbackParameters.speed)
      .apply()
  }

  private fun restorePlaybackState() {
    if (!preferences.getBoolean("active", false)) return
    val key = preferences.getString("key", null) ?: return
    val sourcePath = preferences.getString("source_path", null)?.takeIf(String::isNotBlank) ?: return
    if (!File(sourcePath).isFile) {
      preferences.edit().clear().apply()
      return
    }
    val extras = Bundle().apply {
      putString(EXTRA_LIBRARY_ID, preferences.getString("library_id", ""))
      putString(EXTRA_CONTENT_HASH, preferences.getString("content_hash", ""))
      putString(EXTRA_SOURCE_PATH, sourcePath)
    }
    val metadata = MediaMetadata.Builder()
      .setTitle(preferences.getString("title", "Audiobook"))
      .setArtist(preferences.getString("author", ""))
      .setMediaType(MediaMetadata.MEDIA_TYPE_AUDIO_BOOK)
      .setExtras(extras)
      .build()
    val item = MediaItem.Builder()
      .setMediaId(key)
      .setUri(Uri.fromFile(File(sourcePath)))
      .setMediaMetadata(metadata)
      .build()
    player.setMediaItem(item, preferences.getLong("position_ms", 0L).coerceAtLeast(0L))
    player.setPlaybackSpeed(preferences.getFloat("playback_rate", 1f).coerceIn(.5f, 3f))
    player.prepare()
  }
}
