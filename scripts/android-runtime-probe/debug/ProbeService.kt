package ch.lkmc.copywraith

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.IBinder
import java.util.concurrent.Executors

class ProbeService : Service() {
    private val executor = Executors.newSingleThreadExecutor()
    private var lease = RuntimeProbe.NO_LEASE

    override fun onCreate() {
        super.onCreate()
        lease = RuntimeProbe.acquireService()
        if (lease == RuntimeProbe.NO_LEASE) {
            stopSelf()
            return
        }
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, "Runtime test", NotificationManager.IMPORTANCE_LOW))
        }
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL) else Notification.Builder(this)
        startForeground(NOTIFICATION_ID, builder.setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Copywraith runtime test").build())
        executor.execute {
            try {
                RuntimeProbe.initialize(applicationInfo.dataDir)
            } catch (_: IllegalStateException) {
                stopSelf()
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int) = START_NOT_STICKY
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Drain initialization before releasing the service's exit protection.
        val endingLease = lease
        if (endingLease != RuntimeProbe.NO_LEASE) {
            executor.execute { RuntimeProbe.releaseService(endingLease) }
        }
        executor.shutdown()
        super.onDestroy()
    }

    override fun onTimeout(startId: Int, fgsType: Int) { stopSelf() }

    private companion object {
        const val CHANNEL = "runtime-probe"
        const val NOTIFICATION_ID = 7301
    }
}
