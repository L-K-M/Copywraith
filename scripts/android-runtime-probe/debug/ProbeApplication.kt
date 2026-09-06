package ch.lkmc.copywraith

import android.app.Activity
import android.app.Application
import android.os.Bundle

class ProbeApplication : Application(), Application.ActivityLifecycleCallbacks {
    override fun onCreate() {
        super.onCreate()
        registerActivityLifecycleCallbacks(this)
    }

    override fun onActivityStarted(activity: Activity) {
        // Wry registers its context in onCreate, before this notification.
        if (activity is MainActivity) RuntimeProbe.activityReady(System.identityHashCode(activity))
    }

    override fun onActivityDestroyed(activity: Activity) {
        if (activity is MainActivity) {
            activities.decrementAndGet()
            RuntimeProbe.activityDestroyed(System.identityHashCode(activity))
        }
    }

    override fun onActivityCreated(activity: Activity, state: Bundle?) {
        if (activity is MainActivity) activities.incrementAndGet()
    }

    internal companion object {
        private val activities = java.util.concurrent.atomic.AtomicInteger()
        fun activeActivityCount() = activities.get()
    }
    override fun onActivityResumed(activity: Activity) {}
    override fun onActivityPaused(activity: Activity) {}
    override fun onActivityStopped(activity: Activity) {}
    override fun onActivitySaveInstanceState(activity: Activity, state: Bundle) {}
}
