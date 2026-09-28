package ch.lkmc.copywraith.share

import android.app.Activity
import android.app.Application
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.View
import android.view.Window
import android.view.WindowInsetsController
import android.view.WindowManager

/**
 * Keeps the status bar and navigation bar icons dark on Copywraith's activity.
 *
 * Copywraith's UI is always light and paints both safe-area strips white. The
 * MainActivity that tauri-cli generates calls androidx's enableEdgeToEdge()
 * with SystemBarStyle.auto, which picks the icon colour from the system night
 * mode, so with the phone in dark mode the clock, battery and navigation
 * buttons are white on white. src-tauri/gen/android is regenerated and not
 * tracked, so the override lives in this plugin.
 *
 * Only framework APIs are used because androidx.core is an implementation
 * dependency of tauri-android and therefore not on this module's compile
 * classpath. Each API level gets the same changes androidx's
 * WindowInsetsControllerCompat makes for setAppearanceLightStatusBars(true)
 * and setAppearanceLightNavigationBars(true).
 */
internal object SystemBars {
  private const val TAG = "CopywraithSystemBars"

  // androidx.activity's DefaultLightScrim. In night mode enableEdgeToEdge
  // paints the API 26-28 navigation bar with its dark scrim (#801B1B1B),
  // which would leave dark buttons on a dark bar.
  private const val LIGHT_NAVIGATION_BAR_SCRIM = 0xE6FFFFFF.toInt()

  private var resumeCallbacksRegistered = false

  /**
   * Applies the light appearance to [activity] now and again every time an
   * activity of the same class resumes.
   *
   * Plugin.onResume is not dispatched on tauri 2.11 (the generated
   * TauriLifecycleObserver is never registered), and a recreated MainActivity
   * runs enableEdgeToEdge() again without calling Plugin.load() again, so the
   * resume hook has to be process-wide. It is registered once per process.
   *
   * Icon colour is cosmetic, so neither step may throw into the caller's
   * share and Shizuku setup or into an activity resume.
   */
  fun install(activity: Activity) {
    applyLightAppearance(activity)
    if (resumeCallbacksRegistered) return
    try {
      activity.application.registerActivityLifecycleCallbacks(ResumeCallbacks(activity.javaClass))
      resumeCallbacksRegistered = true
    } catch (e: Throwable) {
      Log.w(TAG, "Could not watch activity resumes to keep dark system bar icons", e)
    }
  }

  fun applyLightAppearance(activity: Activity) {
    try {
      applyLightAppearance(activity.window)
    } catch (e: Throwable) {
      Log.w(TAG, "Could not switch the system bars to dark icons", e)
    }
  }

  @Suppress("DEPRECATION")
  private fun applyLightAppearance(window: Window) {
    val decorView = window.decorView
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      // Like androidx, also set the legacy flags so that a later
      // setSystemUiVisibility() call from elsewhere keeps the light appearance.
      decorView.systemUiVisibility = decorView.systemUiVisibility or
        View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or
        View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
      val light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
        WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
      window.insetsController?.setSystemBarsAppearance(light, light)
      return
    }

    // The light flags only take effect on bars whose background the window
    // draws itself. API 24-25 has no light navigation bar, so that bar keeps
    // enableEdgeToEdge's dark scrim and white buttons.
    window.clearFlags(WindowManager.LayoutParams.FLAG_TRANSLUCENT_STATUS)
    window.addFlags(WindowManager.LayoutParams.FLAG_DRAWS_SYSTEM_BAR_BACKGROUNDS)
    var lightFlags = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      window.clearFlags(WindowManager.LayoutParams.FLAG_TRANSLUCENT_NAVIGATION)
      lightFlags = lightFlags or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
      // API 29 keeps the navigation bar transparent and the platform derives
      // its scrim from the light flag; only 26-28 need the colour swapped.
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
        window.navigationBarColor = LIGHT_NAVIGATION_BAR_SCRIM
      }
    }
    decorView.systemUiVisibility = decorView.systemUiVisibility or lightFlags
  }

  /**
   * Holds the host activity's class rather than an instance, so the
   * process-wide registration cannot keep a destroyed activity alive.
   */
  private class ResumeCallbacks(
    private val hostActivityClass: Class<out Activity>
  ) : Application.ActivityLifecycleCallbacks {
    override fun onActivityResumed(activity: Activity) {
      // Only the activity that hosts the light web UI. An activity from a
      // library may be dark and need the white icons it asked for.
      if (activity.javaClass != hostActivityClass) return
      applyLightAppearance(activity)
    }

    override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}

    override fun onActivityStarted(activity: Activity) {}

    override fun onActivityPaused(activity: Activity) {}

    override fun onActivityStopped(activity: Activity) {}

    override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}

    override fun onActivityDestroyed(activity: Activity) {}
  }
}
