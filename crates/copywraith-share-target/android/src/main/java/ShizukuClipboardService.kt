package ch.lkmc.copywraith.share

import android.content.ClipData
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.Parcel
import android.os.RemoteException
import android.system.Os
import android.util.Log
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

class ShizukuClipboardService : IShizukuClipboardService.Stub() {
  private var callback: IShizukuClipboardCallback? = null
  private var listener: IBinder? = null
  private var clipboard: IBinder? = null
  private var callingPackage: String = "ch.lkmc.copywraith"
  private var primaryServerUrl: String = ""
  private var fallbackServerUrl: String = ""
  private var apiKey: String = ""
  private var lastText: String? = null
  private var lastUploadedHash: String? = null
  private var statusText: String = "idle"

  override fun start(
    callback: IShizukuClipboardCallback?,
    callingPackage: String?,
    primaryServerUrl: String?,
    fallbackServerUrl: String?,
    apiKey: String?
  ) {
    this.callback = callback
    this.callingPackage = callingPackage?.takeIf { it.isNotBlank() } ?: "ch.lkmc.copywraith"
    this.primaryServerUrl = normalizeBaseUrl(primaryServerUrl)
    this.fallbackServerUrl = normalizeBaseUrl(fallbackServerUrl)
    this.apiKey = apiKey.orEmpty()
    try {
      val service = getSystemServiceBinder("clipboard")
      clipboard = service
      if (service == null) {
        setStatus("unavailable", "Android clipboard service is unavailable.")
        return
      }

      val clipListener = object : Binder() {
        init {
          attachInterface(null, CLIP_LISTENER_DESCRIPTOR)
        }

        override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
          return when (code) {
            IBinder.INTERFACE_TRANSACTION -> {
              reply?.writeString(CLIP_LISTENER_DESCRIPTOR)
              true
            }
            TRANSACTION_DISPATCH_PRIMARY_CLIP_CHANGED -> {
              data.enforceInterface(CLIP_LISTENER_DESCRIPTOR)
              publishCurrentClipboard()
              true
            }
            else -> super.onTransact(code, data, reply, flags)
          }
        }
      }
      listener = clipListener
      addListener(service, clipListener)
      setStatus("listening", "Shizuku clipboard listener running as UID ${Os.getuid()}.")
      publishCurrentClipboard()
    } catch (e: Throwable) {
      setStatus("error", "Failed to start Shizuku clipboard listener: ${e.message ?: e.javaClass.simpleName}")
      Log.w(TAG, "Failed to start listener", e)
    }
  }

  override fun stop() {
    val service = clipboard
    val clipListener = listener
    if (service != null && clipListener != null) {
      try {
        removeListener(service, clipListener)
      } catch (e: Throwable) {
        Log.d(TAG, "Failed to remove clipboard listener", e)
      }
    }
    listener = null
    callback = null
    setStatus("stopped", "Shizuku clipboard listener stopped.")
  }

  override fun readCurrentText(callingPackage: String?): String {
    if (!callingPackage.isNullOrBlank()) {
      this.callingPackage = callingPackage
    }
    return readClipboardText() ?: ""
  }

  override fun status(): String = statusText

  override fun destroy() {
    stop()
    System.exit(0)
  }

  private fun publishCurrentClipboard() {
    val text = readClipboardText()?.takeIf { it.isNotBlank() } ?: return
    if (text == lastText) return
    lastText = text
    try {
      callback?.onClipboardText(text)
    } catch (e: RemoteException) {
      Log.d(TAG, "Clipboard callback failed", e)
    }
    uploadClipboardText(text)
  }

  private fun uploadClipboardText(text: String) {
    val contentHash = sha256Hex(text.toByteArray(Charsets.UTF_8))
    if (contentHash == lastUploadedHash) return
    val endpoints = listOf(primaryServerUrl, fallbackServerUrl).filter { it.isNotBlank() }
    if (endpoints.isEmpty()) {
      setStatus("listening", "Shizuku listener captured clipboard; no server URL configured for direct upload.")
      return
    }

    Thread {
      val uploaded = endpoints.any { endpoint -> postClipboardText(endpoint, text, contentHash) }
      if (uploaded) {
        lastUploadedHash = contentHash
        setStatus("listening", "Shizuku listener uploaded clipboard text to the server.")
      } else {
        setStatus("sync_failed", "Shizuku listener captured clipboard, but direct server upload failed.")
      }
    }.start()
  }

  private fun postClipboardText(endpoint: String, text: String, contentHash: String): Boolean {
    return try {
      val body = JSONObject()
        .put("content_type", "text")
        .put("text_content", text)
        .put("flavors", JSONObject().put("text_plain", text))
        .put("source_app", "Android Shizuku clipboard")
        .put("starred", false)
        .put("content_hash", contentHash)
        .toString()
        .toByteArray(Charsets.UTF_8)

      val connection = (URL("$endpoint/api/entries").openConnection() as HttpURLConnection).apply {
        requestMethod = "POST"
        connectTimeout = 10000
        readTimeout = 30000
        doOutput = true
        setRequestProperty("Content-Type", "application/json")
        if (apiKey.isNotBlank()) {
          setRequestProperty("Authorization", "Bearer $apiKey")
        }
      }
      connection.outputStream.use { it.write(body) }
      val code = connection.responseCode
      val responseStream = if (code >= 400) connection.errorStream else connection.inputStream
      responseStream?.close()
      connection.disconnect()
      code in 200..299
    } catch (e: Throwable) {
      Log.d(TAG, "Failed direct Shizuku upload to $endpoint", e)
      false
    }
  }

  private fun readClipboardText(): String? {
    val service = clipboard ?: getSystemServiceBinder("clipboard")?.also {
      clipboard = it
    } ?: return null

    return try {
      val clip = getPrimaryClip(service) ?: return null
      val item = if (clip.itemCount > 0) clip.getItemAt(0) else return null
      item.text?.toString()?.takeIf { it.isNotBlank() }
        ?: item.coerceToText(null)?.toString()?.takeIf { it.isNotBlank() }
    } catch (e: Throwable) {
      setStatus("error", "Failed to read clipboard through Shizuku: ${e.message ?: e.javaClass.simpleName}")
      Log.d(TAG, "Failed to read clipboard", e)
      null
    }
  }

  private fun getPrimaryClip(service: IBinder): ClipData? {
    val data = Parcel.obtain()
    val reply = Parcel.obtain()
    return try {
      data.writeInterfaceToken(CLIPBOARD_DESCRIPTOR)
      data.writeString(callingPackage)
      if (Build.VERSION.SDK_INT >= 34) {
        data.writeString(null)
        data.writeInt(currentUserId())
        data.writeInt(0)
      } else {
        data.writeInt(currentUserId())
      }

      service.transact(transactionGetPrimaryClip(), data, reply, 0)
      reply.readException()
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        reply.readTypedObject(ClipData.CREATOR)
      } else if (reply.readInt() != 0) {
        ClipData.CREATOR.createFromParcel(reply)
      } else {
        null
      }
    } finally {
      reply.recycle()
      data.recycle()
    }
  }

  private fun addListener(service: IBinder, clipListener: IBinder) {
    transactListener(service, transactionAddPrimaryClipChangedListener(), clipListener)
  }

  private fun removeListener(service: IBinder, clipListener: IBinder) {
    transactListener(service, transactionRemovePrimaryClipChangedListener(), clipListener)
  }

  private fun transactListener(
    service: IBinder,
    transactionCode: Int,
    clipListener: IBinder
  ) {
    val data = Parcel.obtain()
    val reply = Parcel.obtain()
    try {
      data.writeInterfaceToken(CLIPBOARD_DESCRIPTOR)
      data.writeStrongBinder(clipListener)
      data.writeString(callingPackage)
      if (Build.VERSION.SDK_INT >= 34) {
        data.writeString(null)
        data.writeInt(currentUserId())
        data.writeInt(0)
      } else {
        data.writeInt(currentUserId())
      }
      service.transact(transactionCode, data, reply, 0)
      reply.readException()
    } finally {
      reply.recycle()
      data.recycle()
    }
  }

  private fun currentUserId(): Int {
    return try {
      val userHandle = Class.forName("android.os.UserHandle")
      val myUserId = userHandle.getDeclaredMethod("myUserId")
      myUserId.invoke(null) as Int
    } catch (_: Throwable) {
      0
    }
  }

  private fun getSystemServiceBinder(name: String): IBinder? {
    return try {
      val serviceManager = Class.forName("android.os.ServiceManager")
      val getService = serviceManager.getDeclaredMethod("getService", String::class.java)
      getService.invoke(null, name) as? IBinder
    } catch (e: Throwable) {
      Log.d(TAG, "Failed to get system service $name", e)
      null
    }
  }

  private fun normalizeBaseUrl(url: String?): String {
    return url.orEmpty().trim().trimEnd('/')
  }

  private fun sha256Hex(bytes: ByteArray): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
    return digest.joinToString("") { byte -> "%02x".format(byte) }
  }

  private fun setStatus(state: String, message: String) {
    statusText = "$state: $message"
    try {
      callback?.onStatus(state, message)
    } catch (_: RemoteException) {
    }
  }

  private companion object {
    const val TAG = "CopywraithShizuku"
    const val CLIPBOARD_DESCRIPTOR = "android.content.IClipboard"
    const val CLIP_LISTENER_DESCRIPTOR = "android.content.IOnPrimaryClipChangedListener"
    const val TRANSACTION_DISPATCH_PRIMARY_CLIP_CHANGED = IBinder.FIRST_CALL_TRANSACTION

    fun transactionGetPrimaryClip(): Int = if (Build.VERSION.SDK_INT >= 31) 4 else 3
    fun transactionAddPrimaryClipChangedListener(): Int = if (Build.VERSION.SDK_INT >= 31) 7 else 6
    fun transactionRemovePrimaryClipChangedListener(): Int = if (Build.VERSION.SDK_INT >= 31) 8 else 7
  }
}
