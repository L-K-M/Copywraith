package ch.lkmc.copywraith.share

import android.os.Binder
import android.os.RemoteException
import android.util.Log

/** Credential-free observation; the app owns persistence and synchronization. */
class ShizukuClipboardService : IShizukuClipboardService.Stub() {
  private var callback: IShizukuClipboardCallback? = null
  private var driver: ClipboardDriver? = null
  private var lastText: String? = null
  private var statusText = "idle"

  init {
    // Shizuku constructs this dedicated service on its process main thread.
    ClipboardDriver.preparePrivilegedProcess()
  }

  override fun start(callback: IShizukuClipboardCallback?) {
    this.callback = callback
    try {
      driver?.close()
      driver = null
      val connected = ClipboardDriver.connect(Binder.getCallingUid())
      driver = connected
      connected.startObserving { publishCurrentClipboard() }
      setStatus("listening", "Shizuku clipboard observation is active.")
      publishCurrentClipboard()
    } catch (error: Exception) {
      setStatus("unavailable", "Clipboard observation failed. Check Shizuku and Android user support.")
      logFailure("Clipboard registration", error)
    }
  }

  override fun stop() {
    // A failed unregister retains the handle for retry; do not claim it stopped.
    driver?.close()
    driver = null
    setStatus("stopped", "Shizuku clipboard observation stopped.")
    callback = null
  }

  override fun readCurrentText(): String = readClipboardText().orEmpty()

  override fun status(): String = statusText

  override fun destroy() {
    try {
      stop()
    } finally {
      System.exit(0)
    }
  }

  private fun publishCurrentClipboard() {
    val text = readClipboardText()?.takeIf { it.isNotBlank() } ?: return
    if (text == lastText) return
    lastText = text
    try {
      callback?.onClipboardText(text)
    } catch (error: RemoteException) {
      logFailure("Clipboard delivery", error)
    }
  }

  private fun readClipboardText(): String? {
    return try {
      val clip = driver?.readPrimaryClip() ?: return null
      val item = if (clip.itemCount > 0) clip.getItemAt(0) else return null
      item.text?.toString()?.takeIf { it.isNotBlank() }
        ?: item.coerceToText(null)?.toString()?.takeIf { it.isNotBlank() }
    } catch (error: Exception) {
      setStatus("unavailable", "Clipboard read failed. Check Shizuku and Android user support.")
      logFailure("Clipboard read", error)
      null
    }
  }

  private fun setStatus(state: String, message: String) {
    statusText = "$state: $message"
    try {
      callback?.onStatus(state, message)
    } catch (_: RemoteException) {
    }
  }

  private fun logFailure(operation: String, error: Exception) {
    // Remote exception messages may contain data supplied by another process.
    Log.w(TAG, "$operation failed (${error.javaClass.simpleName})")
  }

  private companion object {
    const val TAG = "CopywraithShizuku"
  }
}
