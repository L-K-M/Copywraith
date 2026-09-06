package ch.lkmc.copywraith

import android.app.job.JobParameters
import org.json.JSONObject

// Observe framework delivery independently of whether the service accepts the stop.
internal object ProbeJobEvidence {
    private data class Delivery(val parameters: JobParameters, val token: Long, var stops: Int = 0, var sameReference: Boolean = false)
    private val deliveries = mutableMapOf<Int, Delivery>()

    @Synchronized fun started(parameters: JobParameters, token: Long) {
        deliveries[parameters.jobId] = Delivery(parameters, token)
    }

    @Synchronized fun stopped(parameters: JobParameters) {
        val delivery = deliveries[parameters.jobId] ?: return
        delivery.stops++
        delivery.sameReference = delivery.parameters === parameters
    }

    @Synchronized fun snapshot(jobId: Int): JSONObject {
        val delivery = deliveries[jobId] ?: return JSONObject()
        return JSONObject().put("token", delivery.token).put("stops", delivery.stops)
            .put("sameReference", delivery.sameReference)
    }
}
