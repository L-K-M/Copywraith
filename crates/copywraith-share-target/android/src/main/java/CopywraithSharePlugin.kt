package ch.lkmc.copywraith.share

import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.net.Uri
import android.os.IBinder
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.util.UUID
import rikka.shizuku.Shizuku

class ShizukuStartArgs {
  var server_url_primary: String? = null
  var server_url_fallback: String? = null
  var api_key: String? = null
}

@TauriPlugin
class CopywraithSharePlugin(private val activity: Activity) : Plugin(activity) {
  private var shizukuService: IShizukuClipboardService? = null
  private var shizukuRequested = false
  private var shizukuState = "disabled"
  private var shizukuMessage = "Shizuku clipboard listener is disabled."
  private var shizukuBackendUid: Int? = null
  private var lastShizukuText: String? = null
  private var lastShizukuTextAt: Long = 0L
  private var pendingShizukuConfig = ShizukuStartArgs()

  private val shizukuCallback = object : IShizukuClipboardCallback.Stub() {
    override fun onClipboardText(text: String?) {
      if (text.isNullOrBlank()) return
      if (text == lastShizukuText) return
      lastShizukuText = text
      lastShizukuTextAt = System.currentTimeMillis()

      val items = JSONArray()
      items.put(textShareItem(text).put("source_app", "Android Shizuku clipboard"))
      if (!persistShareBatch(items)) return

      val payload = JSObject()
      payload.put("captured_at", lastShizukuTextAt)
      trigger("shizuku-clipboard-staged", payload)
    }

    override fun onStatus(state: String?, message: String?) {
      updateShizukuStatus(state ?: "unknown", message ?: "Shizuku listener status changed.")
    }
  }

  private val shizukuConnection = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
      shizukuService = IShizukuClipboardService.Stub.asInterface(service)
      val boundService = shizukuService
      if (boundService == null) {
        updateShizukuStatus("unavailable", "Shizuku service returned no binder.")
        return
      }
      try {
        boundService.start(
          shizukuCallback,
          activity.packageName,
          pendingShizukuConfig.server_url_primary.orEmpty(),
          pendingShizukuConfig.server_url_fallback.orEmpty(),
          pendingShizukuConfig.api_key.orEmpty()
        )
        updateShizukuStatus("listening", "Shizuku clipboard listener is running.")
      } catch (e: Exception) {
        updateShizukuStatus("error", "Failed to start Shizuku listener: ${e.message ?: e.javaClass.simpleName}")
      }
    }

    override fun onServiceDisconnected(name: ComponentName?) {
      shizukuService = null
      updateShizukuStatus("stopped", "Shizuku clipboard listener disconnected.")
    }
  }

  private val binderReceivedListener = Shizuku.OnBinderReceivedListener {
    if (shizukuRequested) {
      startShizukuListenerIfReady()
    } else {
      updateShizukuStatus("available", "Shizuku is available but listener is disabled.")
    }
  }

  private val binderDeadListener = Shizuku.OnBinderDeadListener {
    shizukuService = null
    shizukuBackendUid = null
    updateShizukuStatus("unavailable", "Shizuku is not running.")
  }

  private val permissionResultListener = Shizuku.OnRequestPermissionResultListener { requestCode, grantResult ->
    if (requestCode == SHIZUKU_PERMISSION_REQUEST_CODE) {
      if (grantResult == PackageManager.PERMISSION_GRANTED) {
        startShizukuListenerIfReady()
      } else {
        updateShizukuStatus("permission_denied", "Shizuku permission was denied.")
      }
    }
  }

  override fun load(webView: WebView) {
    handleShareIntent(activity.intent)
    installShizukuCallbacks()
  }

  override fun onNewIntent(intent: Intent) {
    handleShareIntent(intent)
  }

  @Command
  fun collectPendingShare(invoke: Invoke) {
    val staged = handleShareIntent(activity.intent)
    val result = JSObject()
    result.put("staged", staged)
    invoke.resolve(result)
  }

  @Command
  fun shizukuStatus(invoke: Invoke) {
    refreshPassiveShizukuStatus()
    invoke.resolve(shizukuStatusObject())
  }

  @Command
  fun startShizukuClipboardListener(invoke: Invoke) {
    pendingShizukuConfig = try {
      invoke.parseArgs(ShizukuStartArgs::class.java)
    } catch (_: Exception) {
      ShizukuStartArgs()
    }
    shizukuRequested = true
    val started = startShizukuListenerIfReady(requestPermission = true)
    val result = shizukuStatusObject()
    result.put("started", started)
    invoke.resolve(result)
  }

  @Command
  fun stopShizukuClipboardListener(invoke: Invoke) {
    shizukuRequested = false
    try {
      shizukuService?.stop()
    } catch (_: Exception) {
    }
    shizukuService = null
    unbindShizukuService(remove = true)
    updateShizukuStatus("disabled", "Shizuku clipboard listener is disabled.")
    invoke.resolve(shizukuStatusObject())
  }

  @Command
  fun readShizukuClipboard(invoke: Invoke) {
    val text = try {
      shizukuService?.readCurrentText(activity.packageName).orEmpty()
    } catch (e: Exception) {
      updateShizukuStatus("error", "Failed to read Shizuku clipboard: ${e.message ?: e.javaClass.simpleName}")
      ""
    }

    val result = shizukuStatusObject()
    result.put("text", text)
    invoke.resolve(result)
  }

  private fun installShizukuCallbacks() {
    try {
      Shizuku.addBinderReceivedListenerSticky(binderReceivedListener)
      Shizuku.addBinderDeadListener(binderDeadListener)
      Shizuku.addRequestPermissionResultListener(permissionResultListener)
      refreshPassiveShizukuStatus()
    } catch (e: Throwable) {
      updateShizukuStatus("unavailable", "Shizuku is not available on this device.")
    }
  }

  private fun refreshPassiveShizukuStatus() {
    try {
      if (!Shizuku.pingBinder()) {
        updateShizukuStatus("unavailable", "Shizuku is not installed or not running.")
        return
      }
      if (Shizuku.isPreV11()) {
        updateShizukuStatus("unsupported", "Shizuku pre-v11 is not supported.")
        return
      }
      shizukuBackendUid = Shizuku.getUid()
      if (Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED) {
        if (shizukuRequested && shizukuService != null) {
          updateShizukuStatus("listening", "Shizuku clipboard listener is running.")
        } else {
          updateShizukuStatus("available", "Shizuku permission granted; listener is disabled.")
        }
      } else {
        updateShizukuStatus("permission_required", "Grant Shizuku permission to enable the listener.")
      }
    } catch (e: Throwable) {
      updateShizukuStatus("unavailable", "Shizuku is not available: ${e.message ?: e.javaClass.simpleName}")
    }
  }

  private fun startShizukuListenerIfReady(requestPermission: Boolean = false): Boolean {
    return try {
      if (!Shizuku.pingBinder()) {
        updateShizukuStatus("unavailable", "Shizuku is not installed or not running.")
        return false
      }
      if (Shizuku.isPreV11()) {
        updateShizukuStatus("unsupported", "Shizuku pre-v11 is not supported.")
        return false
      }
      shizukuBackendUid = Shizuku.getUid()
      if (Shizuku.checkSelfPermission() != PackageManager.PERMISSION_GRANTED) {
        if (requestPermission && !Shizuku.shouldShowRequestPermissionRationale()) {
          Shizuku.requestPermission(SHIZUKU_PERMISSION_REQUEST_CODE)
          updateShizukuStatus("permission_requested", "Waiting for Shizuku permission.")
        } else {
          updateShizukuStatus("permission_required", "Grant Shizuku permission to enable the listener.")
        }
        return false
      }
      if (Shizuku.getVersion() < 10) {
        updateShizukuStatus("unsupported", "Shizuku user services require Shizuku v10 or newer.")
        return false
      }
      if (shizukuService != null) {
        updateShizukuStatus("listening", "Shizuku clipboard listener is already running.")
        return true
      }

      updateShizukuStatus("starting", "Starting Shizuku clipboard listener.")
      Shizuku.bindUserService(shizukuUserServiceArgs(), shizukuConnection)
      true
    } catch (e: Throwable) {
      updateShizukuStatus("unavailable", "Shizuku listener unavailable: ${e.message ?: e.javaClass.simpleName}")
      false
    }
  }

  private fun unbindShizukuService(remove: Boolean) {
    try {
      if (Shizuku.pingBinder() && Shizuku.getVersion() >= 10) {
        Shizuku.unbindUserService(shizukuUserServiceArgs(), shizukuConnection, remove)
      }
    } catch (_: Throwable) {
    }
  }

  private fun shizukuUserServiceArgs(): Shizuku.UserServiceArgs {
    return Shizuku.UserServiceArgs(ComponentName(activity.packageName, ShizukuClipboardService::class.java.name))
      .daemon(true)
      .processNameSuffix("shizuku-clipboard")
      .tag("copywraith-shizuku-clipboard")
      .debuggable((activity.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0)
      .version(SHIZUKU_USER_SERVICE_VERSION)
  }

  private fun updateShizukuStatus(state: String, message: String) {
    shizukuState = state
    shizukuMessage = message
    val payload = shizukuStatusObject()
    trigger("shizuku-status", payload)
  }

  private fun shizukuStatusObject(): JSObject {
    val result = JSObject()
    result.put("state", shizukuState)
    result.put("message", shizukuMessage)
    result.put("available", shizukuState != "unavailable")
    result.put("enabled", shizukuRequested)
    result.put("listening", shizukuState == "listening")
    result.put("backend_uid", shizukuBackendUid ?: JSONObject.NULL)
    result.put("last_clipboard_text_at", if (lastShizukuTextAt > 0) lastShizukuTextAt else JSONObject.NULL)
    return result
  }

  private fun handleShareIntent(intent: Intent?): Boolean {
    if (intent == null) return false
    val action = intent.action ?: return false
    if (action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE) return false

    val staged = when (action) {
      Intent.ACTION_SEND -> handleSendIntent(intent)
      Intent.ACTION_SEND_MULTIPLE -> handleSendMultipleIntent(intent)
      else -> false
    }

    if (staged) {
      intent.action = Intent.ACTION_MAIN
      intent.removeExtra(Intent.EXTRA_TEXT)
      intent.removeExtra(Intent.EXTRA_STREAM)
    }

    return staged
  }

  private fun handleSendIntent(intent: Intent): Boolean {
    val items = JSONArray()

    intent.getStringExtra(Intent.EXTRA_TEXT)
      ?.takeIf { it.isNotBlank() }
      ?.let { text -> items.put(textShareItem(text)) }

    intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
      ?.let { uri -> persistSharedUri(uri, intent.type)?.let(items::put) }

    return persistShareBatch(items)
  }

  private fun handleSendMultipleIntent(intent: Intent): Boolean {
    val items = JSONArray()
    val uris = intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM) ?: return false
    for (uri in uris) {
      persistSharedUri(uri, intent.type)?.let(items::put)
    }

    return persistShareBatch(items)
  }

  private fun textShareItem(text: String): JSONObject {
    return JSONObject()
      .put("type", "text")
      .put("text", text)
  }

  private fun persistSharedUri(uri: Uri, fallbackMimeType: String?): JSONObject? {
    val resolver = activity.contentResolver
    val mimeType = resolver.getType(uri) ?: fallbackMimeType ?: "application/octet-stream"
    val displayName = getDisplayName(uri) ?: fallbackFileName(mimeType)
    val targetDir = File(activity.dataDir, "pending-shares/files").apply { mkdirs() }
    val target = File(
      targetDir,
      "${System.currentTimeMillis()}-${UUID.randomUUID()}-${safeFileName(displayName)}"
    )

    return try {
      resolver.openInputStream(uri)?.use { input ->
        FileOutputStream(target).use { output ->
          if (!copyWithLimit(input, output, MAX_SHARED_FILE_BYTES)) {
            target.delete()
            return null
          }
        }
      } ?: return null

      JSONObject()
        .put("type", "file")
        .put("mime_type", mimeType)
        .put("file_name", displayName)
        .put("stored_path", target.absolutePath)
    } catch (_: Exception) {
      target.delete()
      null
    }
  }

  private fun persistShareBatch(items: JSONArray): Boolean {
    if (items.length() == 0) return false

    val pendingDir = File(activity.dataDir, "pending-shares").apply { mkdirs() }
    val batch = JSONObject()
      .put("items", items)
      .put("created_at", System.currentTimeMillis())
    val target = File(pendingDir, "${System.currentTimeMillis()}-${UUID.randomUUID()}.json")
    target.writeText(batch.toString())
    return true
  }

  private fun getDisplayName(uri: Uri): String? {
    if (uri.scheme == "file") {
      return uri.lastPathSegment
    }

    return try {
      activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
          val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
          if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        }
    } catch (_: Exception) {
      null
    }
  }

  private fun safeFileName(name: String): String {
    return name
      .ifBlank { "shared-file" }
      .replace(Regex("[^A-Za-z0-9._-]"), "_")
      .take(120)
  }

  private fun fallbackFileName(mimeType: String): String {
    val extension = MimeTypeMap.getSingleton().getExtensionFromMimeType(mimeType)
    return if (extension.isNullOrBlank()) "shared-file" else "shared.$extension"
  }

  private fun copyWithLimit(input: java.io.InputStream, output: FileOutputStream, limit: Long): Boolean {
    val buffer = ByteArray(8192)
    var copied = 0L
    while (true) {
      val read = input.read(buffer)
      if (read < 0) return true
      copied += read
      if (copied > limit) return false
      output.write(buffer, 0, read)
    }
  }

  private companion object {
    const val MAX_SHARED_FILE_BYTES = 64L * 1024L * 1024L
    const val SHIZUKU_PERMISSION_REQUEST_CODE = 3742
    const val SHIZUKU_USER_SERVICE_VERSION = 1
  }
}
