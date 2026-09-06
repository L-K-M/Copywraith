package ch.lkmc.copywraith

import android.app.job.JobParameters
import android.app.job.JobService
import android.os.Handler
import android.os.Looper
import org.json.JSONObject

class ProbeJobService : JobService() {
    private val handler = Handler(Looper.getMainLooper())
    private var run: Run? = null
    private data class Run(val parameters: JobParameters, val token: Long)

    override fun onStartJob(parameters: JobParameters): Boolean {
        val token = RuntimeProbe.startJob(applicationInfo.dataDir)
        if (token == 0L) return false
        val current = Run(parameters, token)
        run = current
        poll(current)
        return true
    }

    private fun poll(current: Run) {
        handler.postDelayed({
            if (run !== current) return@postDelayed
            if (JSONObject(RuntimeProbe.snapshot()).getLong("completed") != current.token) {
                poll(current)
                return@postDelayed
            }
            run = null
            jobFinished(current.parameters, false)
        }, COMPLETION_POLL_MS)
    }

    override fun onStopJob(parameters: JobParameters): Boolean {
        val current = run ?: return true
        if (current.parameters !== parameters) return true
        run = null
        handler.removeCallbacksAndMessages(null)
        RuntimeProbe.stopJob(current.token)
        return true
    }

    private companion object { const val COMPLETION_POLL_MS = 50L }
}
