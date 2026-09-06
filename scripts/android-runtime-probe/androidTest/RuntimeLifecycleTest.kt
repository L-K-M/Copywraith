package ch.lkmc.copywraith

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Intent
import android.os.Process
import android.os.Bundle
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
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class RuntimeLifecycleTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val proofScript = instrumentation.context.assets.open("ui-proof.js").bufferedReader().use { it.readText() }

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
        control("hold")
        schedule(scheduler, JOB_ID)
        forceRun(JOB_ID)
        await { evidence().getInt("heldReplies") == 1 }
        val retryToken = stopAndRetry(scheduler)
        await {
            val snapshot = state()
            snapshot.getLong("completed") == retryToken && snapshot.isNull("job") &&
                snapshot.getBoolean("downloaded") && snapshot.getInt("leases") == 1
        }
        assertEquals(0, state().getInt("windows"))
        assertEquals(1, state().getInt("leases"))
        val evidence = evidence()
        assertTrue(evidence.getBoolean("uploaded"))
        assertTrue(evidence.getInt("operations") > 0)
        assertTrue(evidence.getInt("feeds") > 0)
        assertEquals(2, evidence.getInt("requests"))
        assertTrue(evidence.getBoolean("identicalRequests"))
        assertTrue(evidence.getBoolean("identicalReceipts"))
        assertEquals(0, state().getInt("unsynced"))
        progress("replayed", evidence)
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
        scheduler.cancel(BUSY_JOB_ID)
        // Keep the final Activity alive until instrumentation returns; Tao may exit otherwise.
    }

    private fun stopAndRetry(scheduler: JobScheduler): Long {
        val token = state().getLong("job")
        assertEquals(2, state().getInt("leases"))
        assertEquals(1, state().getInt("unsynced"))
        assertEquals(0, evidence().getInt("returnedReplies"))

        // A second OS delivery reaches the same busy native reservation deterministically.
        schedule(scheduler, BUSY_JOB_ID)
        forceRun(BUSY_JOB_ID)
        assertTrue("Busy job was not delivered", waitUntil(STOP_TEARDOWN_TIMEOUT_MS) {
            ProbeJobEvidence.snapshot(BUSY_JOB_ID).has("token")
        })
        assertEquals(RuntimeProbe.NO_LEASE, ProbeJobEvidence.snapshot(BUSY_JOB_ID).getLong("token"))
        val settled = waitUntil(STOP_TEARDOWN_TIMEOUT_MS) {
            val status = schedulerState(BUSY_JOB_ID)
            !status.contains("active") && !status.contains("pending")
        }
        val retained = settled && scheduler.getPendingJob(BUSY_JOB_ID) != null
        progress("busy", JSONObject().put("retryRetained", retained)
            .put("scheduler", schedulerState(BUSY_JOB_ID)).put("native", state()))
        assertEquals(token, state().getLong("job"))
        assertEquals(2, state().getInt("leases"))

        // push_protocol deliberately pulls before freezing candidates; preserve that baseline.
        val beforeStop = state()
        val beforeStopFixture = evidence()
        progress("before-stop", JSONObject().put("native", beforeStop).put("fixture", beforeStopFixture))

        // API36 publishes `stop`; the observer proves delivery before any cancellation claim.
        // USER applies retry backoff here; PREEMPT may immediately restart the same job.
        shell("cmd jobscheduler stop -u 0 -s ${JobParameters.STOP_REASON_USER} ${context.packageName} $JOB_ID")
        assertTrue("Framework stop was not delivered", waitUntil(STOP_TEARDOWN_TIMEOUT_MS) {
            ProbeJobEvidence.snapshot(JOB_ID).getInt("stops") > 0
        })
        val delivery = ProbeJobEvidence.snapshot(JOB_ID)
        progress("stop-delivered", delivery)
        assertFalse("Framework stop must exercise a separately parceled object", delivery.getBoolean("sameReference"))
        val tornDown = waitUntil(STOP_TEARDOWN_TIMEOUT_MS) {
            val snapshot = state()
            snapshot.isNull("job") && snapshot.getLong("completed") == token && snapshot.getInt("leases") == 1
        }
        progress("stop-result", JSONObject().put("tornDown", tornDown).put("native", state()).put("fixture", evidence()))
        val failures = mutableListOf<String>()
        if (!retained) failures.add("Busy admission consumed the scheduler retry")
        if (!tornDown) failures.add("Delivered stop did not cancel native work or release its lease")
        assertTrue(failures.joinToString("; "), failures.isEmpty())
        assertQuiescent(token, beforeStop, beforeStopFixture)
        // Observe beyond several service completion polls while the response stays held.
        SystemClock.sleep(QUIET_OBSERVATION_MS)
        assertQuiescent(token, beforeStop, beforeStopFixture)
        progress("quiescent", JSONObject().put("native", state()).put("fixture", evidence()))

        // Retry the already retained wakeup, without schedule() repairing a lost job.
        scheduler.cancel(JOB_ID)
        control("release")
        forceRun(BUSY_JOB_ID)
        await { ProbeJobEvidence.snapshot(BUSY_JOB_ID).getLong("token") != RuntimeProbe.NO_LEASE }
        val retryToken = ProbeJobEvidence.snapshot(BUSY_JOB_ID).getLong("token")
        assertNotEquals(token, retryToken)
        return retryToken
    }

    private fun assertQuiescent(token: Long, beforeStop: JSONObject, beforeStopFixture: JSONObject) {
        val snapshot = state()
        assertTrue(snapshot.isNull("job"))
        assertEquals(token, snapshot.getLong("completed"))
        assertEquals(1, snapshot.getInt("leases"))
        assertEquals(1, snapshot.getInt("unsynced"))
        assertEquals(0, snapshot.getInt("windows"))
        assertFalse(snapshot.getBoolean("failed"))
        assertEquals(beforeStop.getBoolean("downloaded"), snapshot.getBoolean("downloaded"))

        val fixture = evidence()
        assertEquals(0, fixture.getInt("returnedReplies"))
        for (counter in listOf("feeds", "operations", "requests", "heldReplies")) {
            assertEquals("Work continued after stop: $counter", beforeStopFixture.getInt(counter), fixture.getInt(counter))
        }
    }

    private fun schedule(scheduler: JobScheduler, id: Int) {
        val job = JobInfo.Builder(id, ComponentName(context, ProbeJobService::class.java))
            .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY).setMinimumLatency(JOB_DELAY_MS).build()
        assertEquals(JobScheduler.RESULT_SUCCESS, scheduler.schedule(job))
    }

    private fun forceRun(id: Int) {
        val result = shell("cmd jobscheduler run -f ${context.packageName} $id")
        assertTrue(result, result.contains("Running job"))
    }

    private fun schedulerState(id: Int) = shell("cmd jobscheduler get-job-state ${context.packageName} $id").trim()

    private fun evidence(): JSONObject {
        val connection = URL("$ENDPOINT/probe/evidence").openConnection() as HttpURLConnection
        connection.connectTimeout = FIXTURE_TIMEOUT_MS
        connection.readTimeout = FIXTURE_TIMEOUT_MS
        return try {
            JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
        } finally {
            connection.disconnect()
        }
    }

    private fun control(action: String) {
        val connection = URL("$ENDPOINT/probe/$action").openConnection() as HttpURLConnection
        connection.requestMethod = "POST"
        connection.connectTimeout = FIXTURE_TIMEOUT_MS
        connection.readTimeout = FIXTURE_TIMEOUT_MS
        try {
            assertEquals(HttpURLConnection.HTTP_NO_CONTENT, connection.responseCode)
        } finally {
            connection.disconnect()
        }
    }

    private fun progress(phase: String, evidence: JSONObject) {
        instrumentation.sendStatus(PROBE_PROGRESS_STATUS, Bundle().apply {
            putString("stream", "\nANDROID_JOB_STOP $phase $evidence\n")
        })
    }

    private fun functional(ui: ActivityScenario<MainActivity>, expected: String = "") {
        val ready = AtomicBoolean()
        val token = JSONObject.quote(UUID.randomUUID().toString())
        val expectedJson = JSONObject.quote(expected)
        await {
            ui.onActivity { activity ->
                val webview = findWebView(activity.window.decorView) ?: return@onActivity
                webview.evaluateJavascript("$proofScript\nwindow.runtimeProbe.start($token, $expectedJson);", null)
                webview.evaluateJavascript("window.runtimeProbe.ready($token)") { ready.set(it == "true") }
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
    private fun shell(command: String): String {
        return instrumentation.uiAutomation.executeShellCommand(command).use { descriptor ->
            java.io.FileInputStream(descriptor.fileDescriptor).bufferedReader().use { it.readText() }
        }
    }
    private fun await(condition: () -> Boolean) {
        assertTrue("Runtime gate timed out", waitUntil(TIMEOUT_MS, condition))
    }

    private fun waitUntil(timeoutMs: Long, condition: () -> Boolean): Boolean {
        val deadline = SystemClock.elapsedRealtime() + timeoutMs
        while (!condition()) {
            assertFalse("Native probe failed", state().getBoolean("failed"))
            if (SystemClock.elapsedRealtime() >= deadline) return false
            SystemClock.sleep(POLL_MS)
        }
        return true
    }

    private companion object {
        const val ENDPOINT = "http://127.0.0.1:18763"
        const val JOB_ID = 7302
        const val BUSY_JOB_ID = 7303
        const val JOB_DELAY_MS = 60_000L
        const val TIMEOUT_MS = 30_000L
        const val POLL_MS = 50L
        const val RAPID_REOPENS = 3
        // Finish well before the real client's 30-second HTTP timeout.
        const val STOP_TEARDOWN_TIMEOUT_MS = 3_000L
        const val FIXTURE_TIMEOUT_MS = 2_000
        const val QUIET_OBSERVATION_MS = 500L
        const val PROBE_PROGRESS_STATUS = 2
    }
}
