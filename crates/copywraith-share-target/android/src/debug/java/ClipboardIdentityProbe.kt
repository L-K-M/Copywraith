package ch.lkmc.copywraith.share

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Looper
import android.os.Process
import android.system.Os
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/** Debug-only app_process entry point for a disposable Android emulator. */
object ClipboardIdentityProbe {
  private const val SHELL_PACKAGE = "com.android.shell"
  private const val CALLBACK_TIMEOUT_SECONDS = 10L

  @JvmStatic
  fun main(args: Array<String>) {
    check(args.contentEquals(arrayOf("--isolated-emulator")))
    check(Os.getuid() == Process.ROOT_UID || Os.getuid() == Process.SHELL_UID)
    check(Process.myPid() == Process.myTid())
    // app_process has no Activity-owned main looper, matching Shizuku's bootstrap.
    @Suppress("DEPRECATION")
    if (Looper.getMainLooper() == null) Looper.prepareMainLooper()

    // Match Shizuku's main-thread construction before testing actual Binder attribution.
    val threadClass = Class.forName("android.app.ActivityThread")
    val thread = threadClass.getDeclaredMethod("systemMain").invoke(null)
    val systemContext = threadClass.getDeclaredMethod("getSystemContext").invoke(thread) as Context
    val context = systemContext.createPackageContext(SHELL_PACKAGE, Context.CONTEXT_IGNORE_SECURITY)
    val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    ShizukuClipboardService()
    check(Os.getuid() == Process.SHELL_UID)

    val marker = UUID.randomUUID().toString()
    val received = CountDownLatch(1)
    val failure = AtomicReference<Throwable?>()
    val driver = ClipboardDriver.connect(Process.SHELL_UID)
    try {
      driver.startObserving {
        try {
          if (driver.readPrimaryClip()?.getItemAt(0)?.text?.toString() == marker) received.countDown()
        } catch (error: Throwable) {
          failure.set(error)
          received.countDown()
        }
      }
      manager.setPrimaryClip(ClipData.newPlainText("Copywraith test", marker))
      check(received.await(CALLBACK_TIMEOUT_SECONDS, TimeUnit.SECONDS)) { "Clipboard callback timed out" }
      check(failure.get() == null) { "Clipboard callback read failed" }
      check(driver.readPrimaryClip()?.getItemAt(0)?.text?.toString() == marker) { "Clipboard read failed" }
    } finally {
      driver.close()
    }
    println("Clipboard identity probe passed")
  }
}
