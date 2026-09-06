package ch.lkmc.copywraith

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Intent
import android.os.Process
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean

@RunWith(AndroidJUnit4::class)
class RuntimeLifecycleTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext

    @Test fun coldServiceDestroyExchangeReopen() {
        val pid = Process.myPid()
        assertEquals(0, state().getInt("startups"))
        shell("am start-foreground-service -n ${context.packageName}/.ProbeService")
        await { state().getBoolean("core") }
        assertEquals(0, state().getInt("startups"))
        assertEquals(0, state().getInt("windows"))
        assertEquals(1, state().getInt("leases"))

        var ui = launch()
        functional(ui)
        assertTrue(state().getBoolean("uiCoreMatches"))
        ui.close() // ActivityScenario waits for actual final Activity destruction.
        instrumentation.waitForIdleSync()
        await { state().getInt("windows") == 0 }
        assertEquals(0, ProbeApplication.activeActivityCount())
        assertEquals(pid, Process.myPid())

        // Configure only after final destruction: all fixture HTTP must be headless.
        RuntimeProbe.prepare(ENDPOINT)
        val scheduler = context.getSystemService(JobScheduler::class.java)
        val job = JobInfo.Builder(JOB_ID, ComponentName(context, ProbeJobService::class.java))
            .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY).setMinimumLatency(JOB_DELAY_MS).build()
        assertEquals(JobScheduler.RESULT_SUCCESS, scheduler.schedule(job))
        shell("cmd jobscheduler run -f ${context.packageName} $JOB_ID")
        await { state().getLong("completed") > 0L && state().getBoolean("downloaded") }
        assertEquals(0, state().getInt("windows"))
        assertEquals(1, state().getInt("leases"))
        val connection = URL("$ENDPOINT/probe/evidence").openConnection() as HttpURLConnection
        val evidence = JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
        connection.disconnect()
        assertTrue(evidence.getBoolean("uploaded"))
        assertTrue(evidence.getInt("operations") > 0)
        assertTrue(evidence.getInt("feeds") > 0)
        val entries = URL("$ENDPOINT/api/entries").openConnection() as HttpURLConnection
        entries.setRequestProperty("Authorization", "Bearer fixture-password")
        val persisted = entries.inputStream.bufferedReader().use { it.readText() }
        entries.disconnect()
        assertTrue(persisted.contains("android-headless-upload"))

        ui = launch()
        functional(ui, "android-headless-download")
        ui.recreate() // Exercises Wry's retained attributes with a new Activity instance.
        functional(ui, "android-headless-download")
        repeat(RAPID_REOPENS) {
            ui.close()
            ui = launch() // No process-resume wait; this targets the rapid-reopen race.
            functional(ui, "android-headless-download")
        }
        assertEquals(pid, Process.myPid())
        assertEquals(1, state().getInt("startups"))
        assertEquals(1, state().getInt("windows"))
        assertTrue(state().getBoolean("uiCoreMatches"))
        assertFalse(state().getBoolean("failed"))
        context.stopService(Intent(context, ProbeService::class.java))
        await { state().getInt("leases") == 0 }
        scheduler.cancel(JOB_ID)
        // Keep the final Activity alive until instrumentation returns; Tao may exit otherwise.
    }

    private fun functional(ui: ActivityScenario<MainActivity>, expected: String = "") {
        val ready = AtomicBoolean()
        await {
            ui.onActivity { activity ->
                val webview = findWebView(activity.window.decorView) ?: return@onActivity
                val expectedJson = JSONObject.quote(expected)
                webview.evaluateJavascript("""
                    (async () => {
                      const invoke = window.__TAURI_INTERNALS__.invoke;
                      const platform = await invoke('get_platform');
                      const entries = await invoke('get_entries', {limit:100, offset:0, starredOnly:false, search:null});
                      document.documentElement.dataset.probe = String(platform === 'android' && document.body.innerText.length > 0 &&
                        ($expectedJson === '' || JSON.stringify(entries).includes($expectedJson)));
                    })().catch(() => { document.documentElement.dataset.probe = 'false'; });
                """.trimIndent(), null)
                webview.evaluateJavascript("document.documentElement.dataset.probe === 'true'") { ready.set(it == "true") }
            }
            ready.get()
        }
        assertEquals(1, state().getInt("windows"))
    }

    private fun findWebView(view: View): WebView? {
        if (view is WebView) return view
        if (view !is ViewGroup) return null
        for (index in 0 until view.childCount) {
            findWebView(view.getChildAt(index))?.let { return it }
        }
        return null
    }

    private fun launch(): ActivityScenario<MainActivity> = ActivityScenario.launch(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    private fun state() = JSONObject(RuntimeProbe.snapshot())
    private fun shell(command: String) {
        instrumentation.uiAutomation.executeShellCommand(command).use { descriptor ->
            java.io.FileInputStream(descriptor.fileDescriptor).use { it.readBytes() }
        }
    }
    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + TIMEOUT_MS
        while (!condition()) {
            assertFalse("Native probe failed", state().getBoolean("failed"))
            assertTrue("Runtime gate timed out", SystemClock.elapsedRealtime() < deadline)
            SystemClock.sleep(POLL_MS)
        }
    }

    private companion object {
        const val ENDPOINT = "http://127.0.0.1:18763"
        const val JOB_ID = 7302
        const val JOB_DELAY_MS = 60_000L
        const val TIMEOUT_MS = 30_000L
        const val POLL_MS = 50L
        const val RAPID_REOPENS = 3
    }
}
