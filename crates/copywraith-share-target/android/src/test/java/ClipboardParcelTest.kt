package ch.lkmc.copywraith.share

import android.content.ClipData
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.Parcel
import android.os.Parcelable
import android.os.RemoteException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], manifest = Config.NONE)
class ClipboardParcelTest {
  @Test
  fun pieReadHasNoUserArgument() {
    val remote = receiver { code, data, _ ->
      assertEquals(PIE_READ, code)
      assertEquals(SHELL_PACKAGE, data.readString())
      assertEquals("Android 9 has no userId argument", 0, data.dataAvail())
    }
    assertNull(driver(remote).readPrimaryClip())
  }

  @Test
  fun pieRemovalHasOnlyTheListener() {
    var registered: IBinder? = null
    val calls = mutableListOf<Int>()
    val remote = receiver { code, data, _ ->
      calls.add(code)
      if (code == PIE_OBSERVE) {
        registered = data.readStrongBinder()
        assertEquals(SHELL_PACKAGE, data.readString())
      } else {
        assertEquals(PIE_REMOVE, code)
        assertNotNull(registered)
        assertEquals(registered, data.readStrongBinder())
      }
      assertEquals("Android 9 removal has no package or user", 0, data.dataAvail())
    }
    val driver = driver(remote)
    driver.startObserving { }
    driver.close()
    assertEquals(listOf(PIE_OBSERVE, PIE_REMOVE), calls)
  }

  @Test
  @Config(sdk = [24])
  fun nougatReadPrecedesTheDescriptionTransaction() {
    val remote = receiver { code, data, _ ->
      assertEquals(NOUGAT_READ, code)
      assertEquals(SHELL_PACKAGE, data.readString())
      assertEquals(0, data.dataAvail())
    }
    assertNull(driver(remote).readPrimaryClip())
  }

  @Test
  fun supportedWireProfilesMatchPublishedAidl() {
    // Parcel modeling on SDK 28; this does not claim system-service execution on each OS.
    for (api in Build.VERSION_CODES.N..Build.VERSION_CODES.BAKLAVA) {
      val read = when {
        api < Build.VERSION_CODES.P -> NOUGAT_READ
        api < Build.VERSION_CODES.S -> PIE_READ
        else -> SOURCE_READ
      }
      val observe = read + OBSERVE_OFFSET_FROM_READ
      val remove = observe + REMOVE_OFFSET_FROM_OBSERVE
      val calls = mutableListOf<Int>()
      var registered: IBinder? = null
      val remote = receiver { code, data, _ ->
        calls.add(code)
        if (code == observe) registered = data.readStrongBinder()
        if (code == remove) assertEquals(registered, data.readStrongBinder())

        val identityPresent = code != remove || api >= Build.VERSION_CODES.Q
        if (identityPresent) {
          assertEquals(SHELL_PACKAGE, data.readString())
          if (api >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) assertNull(data.readString())
          if (api >= Build.VERSION_CODES.Q) assertEquals(TEST_USER, data.readInt())
          if (api >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) assertEquals(DEFAULT_DEVICE, data.readInt())
        }
        assertEquals("Unexpected arguments for API $api call $code", 0, data.dataAvail())
      }
      val driver = ClipboardDriver(remote, api, TEST_USER)
      driver.readPrimaryClip()
      driver.startObserving { }
      driver.close()
      driver.close()
      assertEquals(listOf(read, observe, remove), calls)
    }
  }

  @Test
  fun clipboardPayloadRoundTripsWithoutTextCoercion() {
    val clip = ClipData.newHtmlText("fixture", "plain", "<b>rich</b>")
    val remote = receiver { _, data, reply ->
      data.readString()
      assertEquals(0, data.dataAvail())
      reply.writeInt(1)
      clip.writeToParcel(reply, Parcelable.PARCELABLE_WRITE_RETURN_VALUE)
    }
    val actual = requireNotNull(driver(remote).readPrimaryClip()).getItemAt(0)
    assertEquals("plain", actual.text.toString())
    assertEquals("<b>rich</b>", actual.htmlText)
  }

  @Test
  fun ambiguousRegistrationAndFailedRemovalRetainTheReceiver() {
    var attempts = 0
    var registered: IBinder? = null
    val remote = object : Binder() {
      override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
        data.enforceInterface(CLIPBOARD_DESCRIPTOR)
        val listener = data.readStrongBinder()
        attempts++
        when (attempts) {
          1 -> {
            assertEquals(PIE_OBSERVE, code)
            registered = listener
            // Registration may have committed even though delivery failed.
            return false
          }
          2 -> {
            assertEquals(PIE_REMOVE, code)
            assertEquals(registered, listener)
            requireNotNull(reply).writeException(SecurityException("fixture"))
          }
          3 -> {
            assertEquals(PIE_REMOVE, code)
            assertEquals(registered, listener)
            requireNotNull(reply).writeNoException()
          }
          else -> fail("Unexpected clipboard transaction")
        }
        return true
      }
    }
    val driver = driver(remote)
    assertThrows(RemoteException::class.java) { driver.startObserving { } }
    assertThrows(SecurityException::class.java) { driver.close() }
    driver.close()
    driver.close()
    assertEquals(3, attempts)
  }

  @Test
  fun unverifiedAndroidVersionsDoNotGuessATransaction() {
    val remote = receiver { _, _, _ -> fail("Unverified transaction") }
    assertThrows(UnsupportedOperationException::class.java) {
      ClipboardDriver(remote, Build.VERSION_CODES.BAKLAVA + 1, TEST_USER)
    }
  }

  private fun driver(remote: IBinder) = ClipboardDriver(remote, Build.VERSION.SDK_INT, TEST_USER)

  // Model the AIDL receiver, independent of the driver's private protocol table.
  private fun receiver(check: (Int, Parcel, Parcel) -> Unit): IBinder = object : Binder() {
    override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
      data.enforceInterface(CLIPBOARD_DESCRIPTOR)
      requireNotNull(reply).writeNoException()
      val bodyPosition = reply.dataPosition()
      check(code, data, reply)
      if (reply.dataPosition() == bodyPosition) reply.writeInt(0)
      return true
    }
  }

  private companion object {
    const val CLIPBOARD_DESCRIPTOR = "android.content.IClipboard"
    const val SHELL_PACKAGE = "com.android.shell"
    const val NOUGAT_READ = IBinder.FIRST_CALL_TRANSACTION + 1
    const val PIE_READ = IBinder.FIRST_CALL_TRANSACTION + 2
    const val SOURCE_READ = IBinder.FIRST_CALL_TRANSACTION + 3
    const val PIE_OBSERVE = IBinder.FIRST_CALL_TRANSACTION + 5
    const val PIE_REMOVE = IBinder.FIRST_CALL_TRANSACTION + 6
    const val OBSERVE_OFFSET_FROM_READ = 3
    const val REMOVE_OFFSET_FROM_OBSERVE = 1
    const val TEST_USER = 10
    const val DEFAULT_DEVICE = 0
  }
}
