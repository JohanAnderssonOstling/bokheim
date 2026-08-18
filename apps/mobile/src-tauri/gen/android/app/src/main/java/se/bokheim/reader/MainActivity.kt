package se.bokheim.reader

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import android.provider.OpenableColumns
import android.view.KeyEvent
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.core.content.FileProvider
import androidx.lifecycle.lifecycleScope
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicBoolean
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

class MainActivity : TauriActivity() {
  companion object {
    private const val EPUB_MIME = "application/epub+zip"
    private val BOOK_MIME_TYPES = arrayOf(
      EPUB_MIME,
      "application/pdf",
      "application/x-mobipocket-ebook",
      "application/x-fictionbook+xml",
      "application/vnd.comicbook+zip",
      "application/zip",
      "application/webpub+zip",
      "audio/mp4",
      "audio/x-m4b",
      "text/html",
      "application/xhtml+xml",
    )
    private const val APP_NAME_LOWER = "bokheim"
    private const val EPUB_PICKED_EVENT = "${APP_NAME_LOWER}-epub-picked"
    private const val VOLUME_PAGE_TURN_EVENT = "${APP_NAME_LOWER}-volume-page"
    private const val NETWORK_CHANGED_EVENT = "${APP_NAME_LOWER}-network-changed"
    private const val AUDIOBOOK_STATE_EVENT = "${APP_NAME_LOWER}-audiobook-state"
    private const val APK_UPDATE_EVENT = "${APP_NAME_LOWER}-apk-update"
    private const val RELEASE_API = "https://api.github.com/repos/JohanAnderssonOstling/bokheim-releases/releases/latest"
    private const val RELEASE_DOWNLOAD_PREFIX = "https://github.com/JohanAnderssonOstling/bokheim-releases/releases/download/"
    private const val UPDATE_CHECK_INTERVAL_MS = 24L * 60L * 60L * 1000L
  }

  private var readerWebView: WebView? = null
  private var volumePageTurningEnabled = false
  private var readerImmersiveEnabled = false
  private var pendingEpubRequestId: String? = null
  private var networkCallbackRegistered = false
  private var audiobookController: MediaController? = null
  private var audiobookControllerFuture: ListenableFuture<MediaController>? = null
  private val updateOperationActive = AtomicBoolean(false)
  private var pendingUpdateApk: File? = null
  @Volatile private var latestAudiobookState = JSONObject().apply { put("available", false); put("active", false) }.toString()
  private val audiobookStateHandler = Handler(Looper.getMainLooper())
  private val audiobookStateTicker = object : Runnable {
    override fun run() {
      updateAudiobookState()
      audiobookStateHandler.postDelayed(this, 750L)
    }
  }
  private val audiobookListener = object : Player.Listener {
    override fun onEvents(player: Player, events: Player.Events) = updateAudiobookState()
  }
  private val connectivityManager by lazy { getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager }
  private val networkCallback = object : ConnectivityManager.NetworkCallback() {
    override fun onAvailable(network: Network) = emitNetworkStatus()
    override fun onLost(network: Network) = emitNetworkStatus()
    override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) = emitNetworkStatus()
  }
  private val epubPickerLauncher = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
    val requestId = pendingEpubRequestId ?: return@registerForActivityResult
    pendingEpubRequestId = null
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      emitEpubPickerResult(requestId)
      return@registerForActivityResult
    }

    lifecycleScope.launch {
      try {
        val (path, name) = withContext(Dispatchers.IO) { stagePublication(uri) }
        emitEpubPickerResult(requestId, path, name)
      } catch (error: Exception) {
        emitEpubPickerResult(requestId, error = error.message ?: "Could not read the selected publication")
      }
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    readerWebView = webView
    webView.addJavascriptInterface(VolumeNavigationBridge(), "BokheimAndroid")
    emitNetworkStatus()
    ensureAudiobookController()
    audiobookStateHandler.removeCallbacks(audiobookStateTicker)
    audiobookStateHandler.post(audiobookStateTicker)
  }

  override fun onStart() {
    super.onStart()
    ensureAudiobookController()
    audiobookStateHandler.removeCallbacks(audiobookStateTicker)
    audiobookStateHandler.post(audiobookStateTicker)
    if (!networkCallbackRegistered) {
      connectivityManager.registerDefaultNetworkCallback(networkCallback)
      networkCallbackRegistered = true
    }
  }

  override fun onStop() {
    audiobookStateHandler.removeCallbacks(audiobookStateTicker)
    if (networkCallbackRegistered) {
      connectivityManager.unregisterNetworkCallback(networkCallback)
      networkCallbackRegistered = false
    }
    super.onStop()
  }

  override fun onDestroy() {
    audiobookStateHandler.removeCallbacks(audiobookStateTicker)
    audiobookController?.removeListener(audiobookListener)
    audiobookController = null
    audiobookControllerFuture?.let(MediaController::releaseFuture)
    audiobookControllerFuture = null
    readerWebView = null
    super.onDestroy()
  }

  override fun dispatchKeyEvent(event: KeyEvent): Boolean {
    val isVolumeKey = event.keyCode == KeyEvent.KEYCODE_VOLUME_DOWN || event.keyCode == KeyEvent.KEYCODE_VOLUME_UP
    if (!volumePageTurningEnabled || !isVolumeKey) {
      return super.dispatchKeyEvent(event)
    }

    return when (event.action) {
      KeyEvent.ACTION_DOWN -> {
        // repeatCount is reliable even when Android drops ACTION_UP while focus
        // changes. Depending on a latched pressed-key set could otherwise leave
        // page turning permanently stuck after a system UI interruption.
        if (event.repeatCount == 0) {
          emitVolumePageTurn(if (event.keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) "next" else "previous")
        }
        true
      }
      KeyEvent.ACTION_UP -> true
      else -> true
    }
  }

  override fun onResume() {
    super.onResume()
    if (readerImmersiveEnabled) applyReaderImmersiveMode(true)
    pendingUpdateApk?.takeIf { it.isFile }?.let { apk ->
      if (android.os.Build.VERSION.SDK_INT < 26 || packageManager.canRequestPackageInstalls()) {
        pendingUpdateApk = null
        openApkInstaller(apk)
      }
    }
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus && readerImmersiveEnabled) applyReaderImmersiveMode(true)
  }

  private fun applyReaderImmersiveMode(enabled: Boolean) {
    readerImmersiveEnabled = enabled
    WindowCompat.getInsetsController(window, window.decorView).apply {
      systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      if (enabled) hide(WindowInsetsCompat.Type.systemBars())
      else show(WindowInsetsCompat.Type.systemBars())
    }
  }

  private fun emitVolumePageTurn(direction: String) {
      readerWebView?.post {
        readerWebView?.evaluateJavascript(
        "window.dispatchEvent(new CustomEvent('$VOLUME_PAGE_TURN_EVENT', { detail: '$direction' }));",
        null,
      )
    }
  }

  private fun currentNetworkStatus(): JSONObject {
    val network = connectivityManager.activeNetwork
    val capabilities = network?.let(connectivityManager::getNetworkCapabilities)
    val connected = network != null && capabilities != null
    val transport = when {
      !connected -> "offline"
      capabilities!!.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
      capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
      capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
      capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> "vpn"
      else -> "other"
    }
    return JSONObject().apply {
      put("transport", transport)
      put("connected", connected)
      put("metered", if (connected) connectivityManager.isActiveNetworkMetered else JSONObject.NULL)
      put("validated", if (connected) capabilities!!.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) else false)
    }
  }

  private fun emitNetworkStatus() {
    val detail = currentNetworkStatus()
    readerWebView?.post {
      readerWebView?.evaluateJavascript(
        "window.dispatchEvent(new CustomEvent('$NETWORK_CHANGED_EVENT', { detail: $detail }));",
        null,
      )
    }
  }

  private fun ensureAudiobookController() {
    if (audiobookController != null || audiobookControllerFuture != null) return
    val token = SessionToken(this, ComponentName(this, BokheimPlaybackService::class.java))
    val future = MediaController.Builder(this, token).buildAsync()
    audiobookControllerFuture = future
    future.addListener({
      try {
        if (isDestroyed) {
          MediaController.releaseFuture(future)
          return@addListener
        }
        audiobookController = future.get().also { it.addListener(audiobookListener) }
        updateAudiobookState()
      } catch (error: Exception) {
        latestAudiobookState = JSONObject().apply {
          put("available", false)
          put("active", false)
          put("error", error.message ?: "Could not connect to Android media playback")
        }.toString()
        emitAudiobookState()
      }
    }, mainExecutor)
  }

  private fun withAudiobookController(action: (MediaController) -> Unit) {
    runOnUiThread {
      val controller = audiobookController
      if (controller != null) action(controller)
      else {
        ensureAudiobookController()
        audiobookControllerFuture?.addListener({ audiobookController?.let(action) }, mainExecutor)
      }
    }
  }

  private fun currentAudiobookState(): JSONObject {
    val controller = audiobookController ?: return JSONObject().apply {
      put("available", false)
      put("active", false)
    }
    val item = controller.currentMediaItem
    val metadata = item?.mediaMetadata
    val extras = metadata?.extras
    val duration = controller.duration.takeUnless { it == C.TIME_UNSET || it < 0L }
    return JSONObject().apply {
      put("available", true)
      put("active", item != null)
      put("key", item?.mediaId ?: JSONObject.NULL)
      put("libraryId", extras?.getString(BokheimPlaybackService.EXTRA_LIBRARY_ID) ?: JSONObject.NULL)
      put("contentHash", extras?.getString(BokheimPlaybackService.EXTRA_CONTENT_HASH) ?: JSONObject.NULL)
      put("sourcePath", extras?.getString(BokheimPlaybackService.EXTRA_SOURCE_PATH) ?: JSONObject.NULL)
      put("title", metadata?.title?.toString() ?: JSONObject.NULL)
      put("author", metadata?.artist?.toString() ?: JSONObject.NULL)
      put("positionMs", if (item == null) 0L else controller.currentPosition.coerceAtLeast(0L))
      put("durationMs", duration ?: JSONObject.NULL)
      put("playing", controller.isPlaying)
      put("playbackRate", controller.playbackParameters.speed.toDouble())
      put("error", controller.playerError?.message ?: JSONObject.NULL)
    }
  }

  private fun updateAudiobookState() {
    latestAudiobookState = currentAudiobookState().toString()
    emitAudiobookState()
  }

  private fun emitAudiobookState() {
    val detail = latestAudiobookState
    readerWebView?.post {
      readerWebView?.evaluateJavascript(
        "window.dispatchEvent(new CustomEvent('$AUDIOBOOK_STATE_EVENT', { detail: $detail }));",
        null,
      )
    }
  }

  private fun emitApkUpdateState(
    phase: String,
    version: String? = null,
    url: String? = null,
    sha256: String? = null,
    fileName: String? = null,
    progress: Double? = null,
    error: String? = null,
  ) {
    val detail = JSONObject().apply {
      put("phase", phase)
      put("version", version ?: JSONObject.NULL)
      put("currentVersion", BuildConfig.VERSION_NAME)
      put("url", url ?: JSONObject.NULL)
      put("sha256", sha256 ?: JSONObject.NULL)
      put("fileName", fileName ?: JSONObject.NULL)
      put("progress", progress ?: JSONObject.NULL)
      put("error", error ?: JSONObject.NULL)
    }
    readerWebView?.post {
      readerWebView?.evaluateJavascript(
        "window.dispatchEvent(new CustomEvent('$APK_UPDATE_EVENT', { detail: $detail }));",
        null,
      )
    }
  }

  private fun openConnection(url: String): HttpURLConnection = (URL(url).openConnection() as HttpURLConnection).apply {
    connectTimeout = 15_000
    readTimeout = 30_000
    instanceFollowRedirects = true
    setRequestProperty("Accept", "application/vnd.github+json")
    setRequestProperty("User-Agent", "Bokheim-Android/${BuildConfig.VERSION_NAME}")
  }

  private fun readHttpsText(url: String): String {
    require(url.startsWith("https://")) { "Update metadata must use HTTPS" }
    val connection = openConnection(url)
    try {
      if (connection.responseCode !in 200..299) error("Update server returned HTTP ${connection.responseCode}")
      return connection.inputStream.bufferedReader().use { it.readText() }
    } finally {
      connection.disconnect()
    }
  }

  private fun versionParts(value: String): List<Int> = value.trim().removePrefix("v").split('.', '-', '+').take(3).map { it.toIntOrNull() ?: 0 }.let { it + List((3 - it.size).coerceAtLeast(0)) { 0 } }

  private fun isNewerVersion(candidate: String, current: String): Boolean {
    val candidateParts = versionParts(candidate)
    val currentParts = versionParts(current)
    return (0..2).firstNotNullOfOrNull { index ->
      (candidateParts[index] - currentParts[index]).takeIf { it != 0 }
    }?.let { it > 0 } ?: false
  }

  private fun checkForApkUpdate(force: Boolean) {
    if (BuildConfig.DEBUG || !updateOperationActive.compareAndSet(false, true)) return
    val preferences = getSharedPreferences("bokheim-updater", Context.MODE_PRIVATE)
    val lastCheck = preferences.getLong("last-successful-check", 0L)
    if (!force && System.currentTimeMillis() - lastCheck < UPDATE_CHECK_INTERVAL_MS) {
      val cachedVersion = preferences.getString("available-version", null)
      val cachedUrl = preferences.getString("available-url", null)
      val cachedSha256 = preferences.getString("available-sha256", null)
      val cachedFileName = preferences.getString("available-file-name", null)
      if (cachedVersion != null && cachedUrl != null && cachedSha256 != null && cachedFileName != null && isNewerVersion(cachedVersion, BuildConfig.VERSION_NAME)) {
        emitApkUpdateState("available", cachedVersion, cachedUrl, cachedSha256, cachedFileName)
      }
      updateOperationActive.set(false)
      return
    }
    emitApkUpdateState("checking")
    Thread {
      try {
        val release = JSONObject(readHttpsText(RELEASE_API))
        val version = release.getString("tag_name").removePrefix("v")
        if (!isNewerVersion(version, BuildConfig.VERSION_NAME)) {
          preferences.edit()
            .putLong("last-successful-check", System.currentTimeMillis())
            .remove("available-version").remove("available-url").remove("available-sha256").remove("available-file-name")
            .apply()
          emitApkUpdateState("current", version = version)
          return@Thread
        }
        val fileName = "bokheim-v${version}-arm64.apk"
        val checksumName = "$fileName.sha256"
        var apkUrl: String? = null
        var checksumUrl: String? = null
        val assets = release.getJSONArray("assets")
        for (index in 0 until assets.length()) {
          val asset = assets.getJSONObject(index)
          when (asset.getString("name")) {
            fileName -> apkUrl = asset.getString("browser_download_url")
            checksumName -> checksumUrl = asset.getString("browser_download_url")
          }
        }
        val verifiedApkUrl = requireNotNull(apkUrl) { "Release is missing $fileName" }
        require(verifiedApkUrl.startsWith(RELEASE_DOWNLOAD_PREFIX)) { "Unexpected APK download address" }
        val verifiedChecksumUrl = requireNotNull(checksumUrl) { "Release is missing $checksumName" }
        require(verifiedChecksumUrl.startsWith(RELEASE_DOWNLOAD_PREFIX)) { "Unexpected checksum download address" }
        val sha256 = readHttpsText(verifiedChecksumUrl).trim().split(Regex("\\s+"), limit = 2).first().lowercase()
        require(sha256.matches(Regex("[0-9a-f]{64}"))) { "Release checksum is invalid" }
        preferences.edit()
          .putLong("last-successful-check", System.currentTimeMillis())
          .putString("available-version", version)
          .putString("available-url", verifiedApkUrl)
          .putString("available-sha256", sha256)
          .putString("available-file-name", fileName)
          .apply()
        emitApkUpdateState("available", version, verifiedApkUrl, sha256, fileName)
      } catch (error: Exception) {
        emitApkUpdateState("error", error = error.message ?: "Could not check for updates")
      } finally {
        updateOperationActive.set(false)
      }
    }.apply { name = "bokheim-update-check" }.start()
  }

  private fun downloadAndInstallApk(url: String, expectedSha256: String, fileName: String) {
    if (BuildConfig.DEBUG || !updateOperationActive.compareAndSet(false, true)) return
    if (!url.startsWith(RELEASE_DOWNLOAD_PREFIX) || !fileName.matches(Regex("bokheim-v[0-9]+\\.[0-9]+\\.[0-9]+-arm64\\.apk")) || !expectedSha256.matches(Regex("[0-9a-fA-F]{64}"))) {
      updateOperationActive.set(false)
      emitApkUpdateState("error", error = "The update package information is invalid")
      return
    }
    emitApkUpdateState("downloading", fileName = fileName, progress = 0.0)
    Thread {
      val target = File(cacheDir, "updates/$fileName")
      val temporary = File(target.parentFile, "$fileName.part")
      try {
        target.parentFile?.mkdirs()
        temporary.delete()
        val digest = MessageDigest.getInstance("SHA-256")
        val connection = openConnection(url)
        try {
          if (connection.responseCode !in 200..299) error("APK download returned HTTP ${connection.responseCode}")
          val length = connection.contentLengthLong
          var received = 0L
          var lastReported = -1
          connection.inputStream.use { input ->
            temporary.outputStream().use { output ->
              val buffer = ByteArray(64 * 1024)
              while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                output.write(buffer, 0, count)
                digest.update(buffer, 0, count)
                received += count
                if (length > 0) {
                  val percent = ((received * 100) / length).toInt()
                  if (percent >= lastReported + 2) {
                    lastReported = percent
                    emitApkUpdateState("downloading", fileName = fileName, progress = percent / 100.0)
                  }
                }
              }
            }
          }
        } finally {
          connection.disconnect()
        }
        val actualSha256 = digest.digest().joinToString("") { "%02x".format(it) }
        require(actualSha256.equals(expectedSha256, ignoreCase = true)) { "Downloaded APK failed checksum verification" }
        target.delete()
        require(temporary.renameTo(target)) { "Could not stage the downloaded APK" }
        pendingUpdateApk = target
        runOnUiThread { requestApkInstallation(target) }
      } catch (error: Exception) {
        temporary.delete()
        emitApkUpdateState("error", error = error.message ?: "Could not download the update")
      } finally {
        updateOperationActive.set(false)
      }
    }.apply { name = "bokheim-update-download" }.start()
  }

  private fun requestApkInstallation(apk: File) {
    if (android.os.Build.VERSION.SDK_INT >= 26 && !packageManager.canRequestPackageInstalls()) {
      emitApkUpdateState("permission_required")
      startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
      return
    }
    pendingUpdateApk = null
    openApkInstaller(apk)
  }

  private fun openApkInstaller(apk: File) {
    try {
      val uri = FileProvider.getUriForFile(this, "$packageName.fileprovider", apk)
      emitApkUpdateState("installing")
      startActivity(Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, "application/vnd.android.package-archive")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      })
    } catch (error: Exception) {
      emitApkUpdateState("error", error = error.message ?: "Could not open Android’s package installer")
    }
  }

  private fun stagePublication(uri: Uri): Pair<String, String> {
    val displayName = contentResolver.query(
      uri,
      arrayOf(OpenableColumns.DISPLAY_NAME),
      null,
      null,
      null,
    )?.use { cursor ->
      val column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
      if (column >= 0 && cursor.moveToFirst()) cursor.getString(column) else null
    }
    val safeName = displayName
      ?.substringAfterLast('/')
      ?.substringAfterLast('\\')
      ?.takeIf { name -> listOf("epub", "pdf", "mobi", "azw", "azw3", "fb2", "cbz", "html", "htm", "xhtml", "webpub", "m4b").any { name.endsWith(".$it", ignoreCase = true) } }
      ?: "book.epub"
    val extension = safeName.substringAfterLast('.', "epub").lowercase()
    val staged = File(cacheDir, "book-import-${UUID.randomUUID()}.$extension")
    try {
      contentResolver.openInputStream(uri)?.use { input ->
        staged.outputStream().use { output -> input.copyTo(output) }
      } ?: error("Could not open the selected publication")
    } catch (error: Exception) {
      staged.delete()
      throw error
    }
    return staged.absolutePath to safeName
  }

  private fun emitEpubPickerResult(
    requestId: String,
    path: String? = null,
    name: String? = null,
    error: String? = null,
  ) {
    val detail = JSONObject().apply {
      put("requestId", requestId)
      put("path", path ?: JSONObject.NULL)
      put("name", name ?: JSONObject.NULL)
      put("error", error ?: JSONObject.NULL)
    }
    readerWebView?.post {
      readerWebView?.evaluateJavascript(
        "window.dispatchEvent(new CustomEvent('$EPUB_PICKED_EVENT', { detail: $detail }));",
        null,
      )
    }
  }

  private inner class VolumeNavigationBridge {
    @JavascriptInterface
    fun setVolumePageTurningEnabled(enabled: Boolean) {
      runOnUiThread {
        volumePageTurningEnabled = enabled
      }
    }

    @JavascriptInterface
    fun setReaderImmersiveEnabled(enabled: Boolean) {
      runOnUiThread { applyReaderImmersiveMode(enabled) }
    }

    @JavascriptInterface
    fun pickEpub(requestId: String) {
      runOnUiThread {
        if (pendingEpubRequestId != null) {
          emitEpubPickerResult(requestId, error = "The EPUB picker is already open")
          return@runOnUiThread
        }
        pendingEpubRequestId = requestId
        val picker = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
          addCategory(Intent.CATEGORY_OPENABLE)
          type = "*/*"
          putExtra(Intent.EXTRA_MIME_TYPES, BOOK_MIME_TYPES)
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        try {
          epubPickerLauncher.launch(picker)
        } catch (error: Exception) {
          pendingEpubRequestId = null
          emitEpubPickerResult(requestId, error = error.message ?: "Could not open the EPUB picker")
        }
      }
    }

    @JavascriptInterface
    fun getNetworkStatus(): String = currentNetworkStatus().toString()

    @JavascriptInterface
    fun isNativeAudiobookAvailable(): Boolean = true

    @JavascriptInterface
    fun getAudiobookState(): String = latestAudiobookState

    @JavascriptInterface
    fun openAudiobook(
      key: String,
      libraryId: String,
      contentHash: String,
      sourcePath: String,
      title: String,
      author: String,
      initialPositionMs: Double,
    ) {
      withAudiobookController { controller ->
        if (controller.currentMediaItem?.mediaId == key) {
          controller.play()
          return@withAudiobookController
        }
        val extras = Bundle().apply {
          putString(BokheimPlaybackService.EXTRA_LIBRARY_ID, libraryId)
          putString(BokheimPlaybackService.EXTRA_CONTENT_HASH, contentHash)
          putString(BokheimPlaybackService.EXTRA_SOURCE_PATH, sourcePath)
        }
        val metadata = MediaMetadata.Builder()
          .setTitle(title)
          .setArtist(author)
          .setMediaType(MediaMetadata.MEDIA_TYPE_AUDIO_BOOK)
          .setExtras(extras)
          .build()
        val item = MediaItem.Builder()
          .setMediaId(key)
          .setUri(Uri.fromFile(File(sourcePath)))
          .setMediaMetadata(metadata)
          .build()
        controller.setMediaItem(item, initialPositionMs.toLong().coerceAtLeast(0L))
        controller.prepare()
        controller.play()
      }
    }

    @JavascriptInterface
    fun toggleAudiobookPlayback() = withAudiobookController { controller ->
      if (controller.isPlaying) controller.pause() else controller.play()
    }

    @JavascriptInterface
    fun seekAudiobookTo(positionMs: Double) = withAudiobookController { controller ->
      controller.seekTo(positionMs.toLong().coerceAtLeast(0L))
    }

    @JavascriptInterface
    fun seekAudiobookBy(offsetMs: Double) = withAudiobookController { controller ->
      val duration = controller.duration.takeUnless { it == C.TIME_UNSET || it < 0L } ?: Long.MAX_VALUE
      controller.seekTo((controller.currentPosition + offsetMs.toLong()).coerceIn(0L, duration))
    }

    @JavascriptInterface
    fun setAudiobookPlaybackRate(rate: Double) = withAudiobookController { controller ->
      controller.setPlaybackSpeed(rate.toFloat().coerceIn(.5f, 3f))
    }

    @JavascriptInterface
    fun closeAudiobook() = withAudiobookController { controller ->
      controller.pause()
      controller.clearMediaItems()
      stopService(Intent(this@MainActivity, BokheimPlaybackService::class.java))
    }

    @JavascriptInterface
    fun isIgnoringBatteryOptimizations(): Boolean {
      val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
      return powerManager.isIgnoringBatteryOptimizations(packageName)
    }

    @JavascriptInterface
    fun openBatteryOptimizationSettings() {
      runOnUiThread {
        val intent = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
        try {
          startActivity(intent)
        } catch (_: Exception) {
          startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
        }
      }
    }

    @JavascriptInterface
    fun isApkUpdaterAvailable(): Boolean = !BuildConfig.DEBUG

    @JavascriptInterface
    fun checkForApkUpdate(force: Boolean) = this@MainActivity.checkForApkUpdate(force)

    @JavascriptInterface
    fun downloadAndInstallApk(url: String, sha256: String, fileName: String) = this@MainActivity.downloadAndInstallApk(url, sha256, fileName)

    @JavascriptInterface
    fun openApkInstallSettings() {
      runOnUiThread {
        startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
      }
    }
  }
}
