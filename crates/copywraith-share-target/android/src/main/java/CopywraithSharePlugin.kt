package ch.lkmc.copywraith.share

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.webkit.WebView
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Plugin
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.util.UUID

@TauriPlugin
class CopywraithSharePlugin(private val activity: Activity) : Plugin(activity) {
  override fun load(webView: WebView) {
    handleShareIntent(activity.intent)
  }

  override fun onNewIntent(intent: Intent) {
    handleShareIntent(intent)
  }

  private fun handleShareIntent(intent: Intent?) {
    if (intent == null) return
    val action = intent.action ?: return
    if (action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE) return

    when (action) {
      Intent.ACTION_SEND -> handleSendIntent(intent)
      Intent.ACTION_SEND_MULTIPLE -> handleSendMultipleIntent(intent)
    }
  }

  private fun handleSendIntent(intent: Intent) {
    val items = JSONArray()

    intent.getStringExtra(Intent.EXTRA_TEXT)
      ?.takeIf { it.isNotBlank() }
      ?.let { text -> items.put(textShareItem(text)) }

    intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
      ?.let { uri -> persistSharedUri(uri, intent.type)?.let(items::put) }

    persistShareBatch(items)
  }

  private fun handleSendMultipleIntent(intent: Intent) {
    val items = JSONArray()
    val uris = intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM) ?: return
    for (uri in uris) {
      persistSharedUri(uri, intent.type)?.let(items::put)
    }

    persistShareBatch(items)
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

  private fun persistShareBatch(items: JSONArray) {
    if (items.length() == 0) return

    val pendingDir = File(activity.dataDir, "pending-shares").apply { mkdirs() }
    val batch = JSONObject()
      .put("items", items)
      .put("created_at", System.currentTimeMillis())
    val target = File(pendingDir, "${System.currentTimeMillis()}-${UUID.randomUUID()}.json")
    target.writeText(batch.toString())
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
  }
}
