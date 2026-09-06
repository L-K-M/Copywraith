package ch.lkmc.copywraith

// Debug-only native boundary; loading the library does not launch Tauri.
internal object RuntimeProbe {
    init { System.loadLibrary("copywraith_tauri_lib") }
    @JvmStatic external fun acquireService(): Long
    @JvmStatic external fun releaseService(id: Long)
    @JvmStatic external fun initialize(dataDir: String)
    @JvmStatic external fun prepare(endpoint: String)
    @JvmStatic external fun startJob(dataDir: String): Long
    @JvmStatic external fun stopJob(id: Long)
    @JvmStatic external fun activityReady(id: Int)
    @JvmStatic external fun activityDestroyed(id: Int)
    @JvmStatic external fun snapshot(): String
}
