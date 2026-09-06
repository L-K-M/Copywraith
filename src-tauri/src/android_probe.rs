//! Debug-only JNI boundary. Loading this library never starts Tauri or Wry.
use crate::mobile_core::{shared_core, MobileCore};
use crate::mobile_runtime::{Lease, LeaseKind, MobileRuntime};
use jni::{
    objects::{JClass, JString},
    sys::{jint, jlong, jstring},
    JNIEnv,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use tauri::Manager;

const WINDOW_LABEL: &str = "popup";
static OWNER: OnceLock<Arc<MobileRuntime>> = OnceLock::new();
static EXECUTOR: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static SERVICES: OnceLock<Mutex<HashMap<u64, Lease>>> = OnceLock::new();
static CORE: OnceLock<Arc<MobileCore>> = OnceLock::new();
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static ACTIVITY: Mutex<Option<i32>> = Mutex::new(None);
static STARTUPS: AtomicUsize = AtomicUsize::new(0);
static UI_CORE_MATCHES: AtomicBool = AtomicBool::new(false);
static FAILED: AtomicBool = AtomicBool::new(false);
static SERVICE_ID: AtomicUsize = AtomicUsize::new(1);

fn owner() -> &'static Arc<MobileRuntime> {
    OWNER.get_or_init(|| Arc::new(MobileRuntime::default()))
}

fn executor() -> &'static tokio::runtime::Runtime {
    EXECUTOR.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("probe runtime")
    })
}

pub(crate) fn tauri_starting() {
    STARTUPS.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn observe_ui_core(core: &Arc<MobileCore>) {
    UI_CORE_MATCHES.store(
        CORE.get().is_some_and(|headless| {
            Arc::ptr_eq(headless, core)
                && Arc::ptr_eq(&headless.storage(), &core.storage())
                && Arc::ptr_eq(&headless.sync_client(), &core.sync_client())
        }),
        Ordering::SeqCst,
    );
}

pub(crate) fn attach(app: tauri::AppHandle) {
    if APP.set(app).is_err() {
        FAILED.store(true, Ordering::SeqCst);
    }
    reconcile();
}

// Queue after native context registration AND after the old window's removal.
fn reconcile() {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    let handle = app.clone();
    // Dispatch from a worker: Tauri executes inline on its own event thread.
    executor().spawn(async move {
        if app
            .run_on_main_thread(move || {
                if ACTIVITY.lock().unwrap().is_none()
                    || handle.get_webview_window(WINDOW_LABEL).is_some()
                {
                    return;
                }
                let Some(config) = handle
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|config| config.label == WINDOW_LABEL)
                else {
                    FAILED.store(true, Ordering::SeqCst);
                    return;
                };
                let result = tauri::WebviewWindowBuilder::from_config(&handle, config)
                    .and_then(|builder| builder.build());
                if result.is_err() {
                    FAILED.store(true, Ordering::SeqCst);
                }
            })
            .is_err()
        {
            FAILED.store(true, Ordering::SeqCst);
        }
    });
}

pub(crate) fn event(_app: &tauri::AppHandle, event: &tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { api, .. } if owner().prevents_exit() => api.prevent_exit(),
        tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::Destroyed,
            ..
        } => reconcile(),
        _ => {}
    }
}

fn checked<T: Default>(env: &mut JNIEnv, result: anyhow::Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(_) => {
            FAILED.store(true, Ordering::SeqCst);
            // Deliberately omit underlying network/settings errors from Java logs.
            let _ = env.throw_new("java/lang/IllegalStateException", "Runtime probe failed");
            T::default()
        }
    }
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_acquireService(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let lease = owner().acquire(LeaseKind::Service);
    let id = SERVICE_ID.fetch_add(1, Ordering::SeqCst) as u64;
    SERVICES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(id, lease);
    id as jlong
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_releaseService(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    SERVICES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(&(id as u64));
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_initialize(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    let result = (|| {
        let path = PathBuf::from(env.get_string(&path)?.to_string_lossy().into_owned());
        let core = shared_core(&path)?;
        if let Some(previous) = CORE.get() {
            anyhow::ensure!(Arc::ptr_eq(previous, &core), "core changed");
        } else {
            let _ = CORE.set(core);
        }
        Ok(())
    })();
    checked(&mut env, result);
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_prepare(
    mut env: JNIEnv,
    _class: JClass,
    endpoint: JString,
) {
    let result = (|| {
        let endpoint = env.get_string(&endpoint)?.to_string_lossy().into_owned();
        CORE.get()
            .ok_or_else(|| anyhow::anyhow!("not initialized"))?
            .prepare_probe(&endpoint)
    })();
    checked(&mut env, result);
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_startJob(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jlong {
    // Executor creation itself must also be protected from final-window exit.
    let _startup = owner().acquire(LeaseKind::Job);
    let result = (|| {
        let path = PathBuf::from(env.get_string(&path)?.to_string_lossy().into_owned());
        // Initialization is inside the job reservation and off Android's main thread.
        Ok(owner()
            .start_job(executor().handle(), async move {
                let result = match shared_core(&path) {
                    Ok(core) => core.exchange().await,
                    Err(error) => Err(error),
                };
                if result.is_err() {
                    FAILED.store(true, Ordering::SeqCst);
                }
            })
            .unwrap_or_default() as jlong)
    })();
    checked(&mut env, result)
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_stopJob(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    owner().stop_job(id as u64);
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_activityReady(
    _env: JNIEnv,
    _class: JClass,
    id: jint,
) {
    *ACTIVITY.lock().unwrap() = Some(id);
    reconcile();
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_activityDestroyed(
    _env: JNIEnv,
    _class: JClass,
    id: jint,
) {
    let mut activity = ACTIVITY.lock().unwrap();
    if *activity == Some(id) {
        *activity = None;
    }
}

#[no_mangle]
extern "system" fn Java_ch_lkmc_copywraith_RuntimeProbe_snapshot(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| {
        let snapshot = serde_json::json!({
            "core": CORE.get().is_some(),
            "uiCoreMatches": UI_CORE_MATCHES.load(Ordering::SeqCst),
            "startups": STARTUPS.load(Ordering::SeqCst),
            "leases": owner().lease_count(),
            "job": owner().job_id(),
            "completed": owner().completed(),
            "windows": APP.get().map(|app| app.webview_windows().len()).unwrap_or_default(),
            "failed": FAILED.load(Ordering::SeqCst),
            "downloaded": CORE.get().map(|core| core.probe_contains("android-headless-download")).transpose()?.unwrap_or_default(),
        });
        Ok(env.new_string(snapshot.to_string())?.into_raw())
    })();
    checked(&mut env, result)
}
