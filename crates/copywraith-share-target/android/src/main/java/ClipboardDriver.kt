package ch.lkmc.copywraith.share

import android.content.ClipData
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.Parcel
import android.os.Process
import android.os.RemoteException
import android.system.Os

/** Privileged clipboard I/O. Callers never construct framework parcels. */
internal class ClipboardDriver(
  private val clipboard: IBinder,
  apiLevel: Int,
  private val userId: Int,
  private val prepareSender: () -> Unit
) : AutoCloseable {
  private enum class Call { Read, Observe, Remove }

  // AOSP IClipboard declaration offsets, relative to FIRST_CALL_TRANSACTION.
  private enum class Protocol(private val read: Int, private val observe: Int, private val remove: Int) {
    Nougat(1, 4, 5), Pie(2, 5, 6), MultiUser(2, 5, 6), Source(3, 6, 7), Device(3, 6, 7);

    fun transaction(call: Call): Int = IBinder.FIRST_CALL_TRANSACTION + when (call) {
      Call.Read -> read
      Call.Observe -> observe
      Call.Remove -> remove
    }
  }

  private val protocol = when (apiLevel) {
    in Build.VERSION_CODES.N until Build.VERSION_CODES.P -> Protocol.Nougat
    Build.VERSION_CODES.P -> Protocol.Pie
    in Build.VERSION_CODES.Q until Build.VERSION_CODES.S -> Protocol.MultiUser
    in Build.VERSION_CODES.S until Build.VERSION_CODES.UPSIDE_DOWN_CAKE -> Protocol.Source
    in Build.VERSION_CODES.UPSIDE_DOWN_CAKE..Build.VERSION_CODES.BAKLAVA -> Protocol.Device
    else -> throw UnsupportedOperationException("Unsupported Android clipboard protocol.")
  }
  private var listener: IBinder? = null

  fun readPrimaryClip(): ClipData? = transact(Call.Read) { reply ->
    if (reply.readInt() == 0) null else ClipData.CREATOR.createFromParcel(reply)
  }

  fun startObserving(onChanged: () -> Unit) {
    check(listener == null) { "Clipboard observation is already registered." }
    val receiver = object : Binder() {
      init {
        attachInterface(null, LISTENER_DESCRIPTOR)
      }

      override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
        if (code == IBinder.INTERFACE_TRANSACTION) {
          reply?.writeString(LISTENER_DESCRIPTOR)
          return true
        }
        if (code != IBinder.FIRST_CALL_TRANSACTION) return super.onTransact(code, data, reply, flags)

        data.enforceInterface(LISTENER_DESCRIPTOR)
        onChanged()
        return true
      }
    }

    // Keep ownership after an ambiguous registration error so close can retry.
    listener = receiver
    transact(Call.Observe, receiver) { }
  }

  override fun close() {
    val receiver = listener ?: return
    transact(Call.Remove, receiver) { }
    listener = null
  }

  private fun writeIdentity(data: Parcel, call: Call) {
    val legacy = protocol == Protocol.Nougat || protocol == Protocol.Pie
    if (legacy && call == Call.Remove) return

    data.writeString(SHELL_PACKAGE)
    if (legacy) return

    if (protocol == Protocol.Device) data.writeString(null)
    data.writeInt(userId)
    if (protocol == Protocol.Device) data.writeInt(DEFAULT_DEVICE_ID)
  }

  private fun <T> transact(call: Call, receiver: IBinder? = null, decode: (Parcel) -> T): T {
    prepareSender()
    val data = Parcel.obtain()
    val reply = Parcel.obtain()
    try {
      data.writeInterfaceToken(CLIPBOARD_DESCRIPTOR)
      if (receiver != null) data.writeStrongBinder(receiver)
      writeIdentity(data, call)
      if (!clipboard.transact(protocol.transaction(call), data, reply, SYNCHRONOUS_CALL)) {
        throw RemoteException("Android rejected the clipboard transaction.")
      }
      reply.readException()
      return decode(reply)
    } finally {
      reply.recycle()
      data.recycle()
    }
  }

  companion object {
    private const val CLIPBOARD_DESCRIPTOR = "android.content.IClipboard"
    private const val LISTENER_DESCRIPTOR = "android.content.IOnPrimaryClipChangedListener"
    private const val SHELL_PACKAGE = "com.android.shell"
    private const val DEFAULT_DEVICE_ID = 0
    private const val SYSTEM_USER_ID = 0
    private const val SYNCHRONOUS_CALL = 0
    @Volatile private var processIdentityPrepared = false

    fun preparePrivilegedProcess() {
      if (processIdentityPrepared) return
      if (Os.getuid() == Process.SHELL_UID) {
        prepareSendingThread()
        processIdentityPrepared = true
        return
      }
      check(Os.getuid() == Process.ROOT_UID && Process.myTid() == Process.myPid()) {
        "Clipboard identity must be initialized by the privileged process leader."
      }

      // Older Binder kernels attribute transactions to the process leader.
      prepareSendingThread()
      processIdentityPrepared = true
    }

    private fun prepareSendingThread() {
      // Newer kernels use the sending thread; pre-existing Binder workers may
      // still have root credentials after the process leader changed identity.
      if (Os.getuid() == Process.ROOT_UID) {
        @Suppress("DEPRECATION") // This runs in a privileged user service, not an ordinary app.
        Os.setuid(Process.SHELL_UID)
      }
      check(Os.getuid() == Process.SHELL_UID && Os.geteuid() == Process.SHELL_UID) {
        "Clipboard shell identity was not established."
      }
    }

    fun connect(ownerUid: Int): ClipboardDriver {
      check(processIdentityPrepared) { "Clipboard process identity is not initialized." }
      val userHandle = Class.forName("android.os.UserHandle")
      val userId = userHandle.getDeclaredMethod("getUserId", Int::class.javaPrimitiveType)
        .invoke(null, ownerUid) as Int

      check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q || userId == SYSTEM_USER_ID) {
        "This Android version cannot select another user's clipboard."
      }

      val manager = Class.forName("android.os.ServiceManager")
      val binder = manager.getDeclaredMethod("getService", String::class.java)
        .invoke(null, "clipboard") as? IBinder
        ?: throw IllegalStateException("Android clipboard service is unavailable.")
      return ClipboardDriver(binder, Build.VERSION.SDK_INT, userId, ::prepareSendingThread)
    }
  }
}
