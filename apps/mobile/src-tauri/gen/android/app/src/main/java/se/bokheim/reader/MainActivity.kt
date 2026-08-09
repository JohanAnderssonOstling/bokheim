package se.bokheim.reader

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.view.KeyEvent
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import java.io.File
import java.util.UUID
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
      "application/webpub+zip",
      "text/html",
      "application/xhtml+xml",
    )
    private const val APP_NAME_LOWER = "bokheim"
    private const val EPUB_PICKED_EVENT = "${APP_NAME_LOWER}-epub-picked"
    private const val VOLUME_PAGE_TURN_EVENT = "${APP_NAME_LOWER}-volume-page"
    private const val NETWORK_CHANGED_EVENT = "${APP_NAME_LOWER}-network-changed"
  }

  private var readerWebView: WebView? = null
  private var volumePageTurningEnabled = false
  private val pressedVolumeKeys = mutableSetOf<Int>()
  private var pendingEpubRequestId: String? = null
  private var networkCallbackRegistered = false
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

    try {
      val (path, name) = stagePublication(uri)
      emitEpubPickerResult(requestId, path, name)
    } catch (error: Exception) {
      emitEpubPickerResult(requestId, error = error.message ?: "Could not read the selected publication")
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
  }

  override fun onStart() {
    super.onStart()
    if (!networkCallbackRegistered) {
      connectivityManager.registerDefaultNetworkCallback(networkCallback)
      networkCallbackRegistered = true
    }
  }

  override fun onStop() {
    if (networkCallbackRegistered) {
      connectivityManager.unregisterNetworkCallback(networkCallback)
      networkCallbackRegistered = false
    }
    super.onStop()
  }

  override fun dispatchKeyEvent(event: KeyEvent): Boolean {
    val isVolumeKey = event.keyCode == KeyEvent.KEYCODE_VOLUME_DOWN || event.keyCode == KeyEvent.KEYCODE_VOLUME_UP
    if (!volumePageTurningEnabled || !isVolumeKey) {
      return super.dispatchKeyEvent(event)
    }

    return when (event.action) {
      KeyEvent.ACTION_DOWN -> {
        // A held button produces repeated ACTION_DOWN events. The key remains
        // latched until ACTION_UP, so one physical press turns exactly one page.
        if (pressedVolumeKeys.add(event.keyCode)) {
          emitVolumePageTurn(if (event.keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) "next" else "previous")
        }
        true
      }
      KeyEvent.ACTION_UP -> {
        pressedVolumeKeys.remove(event.keyCode)
        true
      }
      else -> true
    }
  }

  override fun onPause() {
    pressedVolumeKeys.clear()
    super.onPause()
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
      ?.takeIf { name -> listOf("epub", "pdf", "mobi", "azw", "azw3", "fb2", "cbz", "html", "htm", "xhtml", "webpub").any { name.endsWith(".$it", ignoreCase = true) } }
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
        if (!enabled) pressedVolumeKeys.clear()
      }
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
  }
}
