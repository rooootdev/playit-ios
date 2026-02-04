#![allow(clippy::missing_safety_doc)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use playit_agent_core::network::origin_lookup::OriginLookup;
use playit_agent_core::network::tcp::tcp_settings::TcpSettings;
use playit_agent_core::network::udp::udp_settings::UdpSettings;
use playit_agent_core::playit_agent::{PlayitAgent, PlayitAgentSettings};
use playit_api_client::PlayitApi;
use serde::Deserialize;
use tokio::sync::watch;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;

#[derive(Deserialize, Clone)]
struct FfiConfig {
    secret_key: String,
    #[serde(default)]
    api_url: Option<String>,
    #[serde(default)]
    poll_interval_ms: Option<u64>,
}

#[repr(C)]
pub struct PlayitStatus {
    pub code: i32,
    pub last_address: *const c_char,
    pub last_error: *const c_char,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum PlayitStatusCode {
    Stopped = 0,
    Connecting = 1,
    Connected = 2,
    Disconnected = 3,
    Error = 4,
}

type LogCallback = extern "C" fn(level: i32, message: *const c_char, user_data: *mut c_void);

struct LogCallbackState {
    callback: Option<LogCallback>,
    user_data: *mut c_void,
}

struct StatusSnapshot {
    code: PlayitStatusCode,
    last_address: Option<CString>,
    last_error: Option<CString>,
}

struct GlobalState {
    config: Option<FfiConfig>,
    status: Arc<Mutex<StatusSnapshot>>,
    running: bool,
    stop_tx: Option<watch::Sender<bool>>,
    stopped_rx: Option<std::sync::mpsc::Receiver<()>>,
    keep_running: Option<Arc<AtomicBool>>,
}

static STATE: OnceLock<Mutex<GlobalState>> = OnceLock::new();
static LOG_CALLBACK: OnceLock<Mutex<LogCallbackState>> = OnceLock::new();
static LOG_INIT: OnceLock<()> = OnceLock::new();

fn state() -> &'static Mutex<GlobalState> {
    STATE.get_or_init(|| {
        Mutex::new(GlobalState {
            config: None,
            status: Arc::new(Mutex::new(StatusSnapshot {
                code: PlayitStatusCode::Stopped,
                last_address: None,
                last_error: None,
            })),
            running: false,
            stop_tx: None,
            stopped_rx: None,
            keep_running: None,
        })
    })
}

fn log_state() -> &'static Mutex<LogCallbackState> {
    LOG_CALLBACK.get_or_init(|| {
        Mutex::new(LogCallbackState {
            callback: None,
            user_data: std::ptr::null_mut(),
        })
    })
}

fn set_status(code: PlayitStatusCode, address: Option<String>, error: Option<String>) {
    let status = state()
        .lock()
        .expect("state lock poisoned")
        .status
        .clone();
    let mut lock = status.lock().expect("status lock poisoned");
    lock.code = code;
    lock.last_address = address.and_then(|v| cstring_sanitize(v).ok());
    lock.last_error = error.and_then(|v| cstring_sanitize(v).ok());
}

fn cstring_sanitize(value: String) -> Result<CString, std::ffi::NulError> {
    let cleaned = value.replace('\0', "");
    CString::new(cleaned)
}

fn ensure_logging() {
    LOG_INIT.get_or_init(|| {
        let layer = CallbackLayer;
        let subscriber = tracing_subscriber::registry().with(layer);
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

struct CallbackLayer;

impl<S> Layer<S> for CallbackLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        let mut message = visitor.message.unwrap_or_else(|| {
            if visitor.fields.is_empty() {
                event.metadata().target().to_string()
            } else {
                visitor
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        });

        if !visitor.fields.is_empty() && visitor.message.is_some() {
            let extra = visitor
                .fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(" ");
            if !extra.is_empty() {
                message = format!("{} {}", message, extra);
            }
        }

        send_log(level, &message);
    }
}

#[derive(Default)]
struct LogVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value).trim_matches('"').to_string());
        } else {
            self.fields
                .push((field.name().to_string(), format!("{:?}", value)));
        }
    }
}

fn send_log(level: Level, message: &str) {
    let lock = log_state().lock().expect("log callback lock poisoned");
    let Some(callback) = lock.callback else {
        return;
    };

    let level_code = match level {
        Level::ERROR => 3,
        Level::WARN => 2,
        Level::INFO => 1,
        Level::DEBUG => 0,
        Level::TRACE => -1,
    };

    let cleaned = message.replace('\0', "");
    let Ok(c_message) = CString::new(cleaned) else {
        return;
    };

    callback(level_code, c_message.as_ptr(), lock.user_data);
}

#[no_mangle]
pub extern "C" fn playit_set_log_callback(callback: Option<LogCallback>, user_data: *mut c_void) {
    ensure_logging();
    let mut lock = log_state().lock().expect("log callback lock poisoned");
    lock.callback = callback;
    lock.user_data = user_data;
}

#[no_mangle]
pub unsafe extern "C" fn playit_init(config_json: *const c_char) -> i32 {
    ensure_logging();
    if config_json.is_null() {
        return -1;
    }

    let c_str = CStr::from_ptr(config_json);
    let json = match c_str.to_str() {
        Ok(v) => v,
        Err(_) => return -2,
    };

    let config: FfiConfig = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return -3,
    };

    {
        let mut lock = state().lock().expect("state lock poisoned");
        lock.config = Some(config);
        lock.keep_running = None;
        lock.running = false;
        lock.stop_tx = None;
        lock.stopped_rx = None;
    }
    set_status(PlayitStatusCode::Stopped, None, None);
    0
}

#[no_mangle]
pub extern "C" fn playit_start() -> i32 {
    ensure_logging();
    let (config, status) = {
        let mut lock = state().lock().expect("state lock poisoned");
        if lock.running {
            return -2;
        }

        let config = match lock.config.clone() {
            Some(v) => v,
            None => return -1,
        };

        lock.running = true;
        let status = lock.status.clone();
        lock.keep_running = None;
        (config, status)
    };
    set_status(PlayitStatusCode::Connecting, None, None);

    let (stop_tx, stop_rx) = watch::channel(false);
    let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();

    {
        let mut lock = state().lock().expect("state lock poisoned");
        lock.stop_tx = Some(stop_tx);
        lock.stopped_rx = Some(stopped_rx);
    }

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
        {
            Ok(rt) => rt,
            Err(error) => {
                let mut status_lock = status.lock().expect("status lock poisoned");
                status_lock.code = PlayitStatusCode::Error;
                status_lock.last_error =
                    cstring_sanitize(format!("failed to create runtime: {}", error)).ok();
                let _ = stopped_tx.send(());
                return;
            }
        };

        runtime.block_on(async move {
            if let Err(error) = run_agent(config, status.clone(), stop_rx).await {
                let mut status_lock = status.lock().expect("status lock poisoned");
                status_lock.code = PlayitStatusCode::Error;
                status_lock.last_error = cstring_sanitize(error).ok();
            }
        });

        {
            let mut lock = state().lock().expect("state lock poisoned");
            lock.running = false;
            lock.keep_running = None;
            lock.stop_tx = None;
            lock.stopped_rx = None;
        }

        let _ = stopped_tx.send(());
    });

    0
}

#[no_mangle]
pub extern "C" fn playit_stop() -> i32 {
    let (stop_tx, stopped_rx, keep_running) = {
        let mut lock = state().lock().expect("state lock poisoned");
        if !lock.running {
            return 0;
        }
        lock.running = false;
        (
            lock.stop_tx.take(),
            lock.stopped_rx.take(),
            lock.keep_running.take(),
        )
    };

    if let Some(keep_running) = keep_running {
        keep_running.store(false, Ordering::SeqCst);
    }

    if let Some(stop_tx) = stop_tx {
        let _ = stop_tx.send(true);
    }

    if let Some(stopped_rx) = stopped_rx {
        let _ = stopped_rx.recv_timeout(Duration::from_secs(2));
    }

    set_status(PlayitStatusCode::Stopped, None, None);
    0
}

#[no_mangle]
pub extern "C" fn playit_get_status() -> PlayitStatus {
    let status = state()
        .lock()
        .expect("state lock poisoned")
        .status
        .lock()
        .expect("status lock poisoned");

    PlayitStatus {
        code: status.code as i32,
        last_address: status
            .last_address
            .as_ref()
            .map(|v| v.as_ptr())
            .unwrap_or(std::ptr::null()),
        last_error: status
            .last_error
            .as_ref()
            .map(|v| v.as_ptr())
            .unwrap_or(std::ptr::null()),
    }
}

async fn run_agent(
    config: FfiConfig,
    status: Arc<Mutex<StatusSnapshot>>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    let api_url = config
        .api_url
        .unwrap_or_else(|| "https://api.playit.gg".to_string());
    let poll_interval = Duration::from_millis(config.poll_interval_ms.unwrap_or(3_000));

    let api = PlayitApi::create(api_url.clone(), Some(config.secret_key.clone()));
    let lookup = Arc::new(OriginLookup::default());

    let initial_data = api
        .v1_agents_rundata()
        .await
        .map_err(|e| format!("failed to load run data: {}", e))?;
    lookup.update_from_run_data(&initial_data).await;

    update_status_from_rundata(&status, &initial_data);

    let settings = PlayitAgentSettings {
        udp_settings: UdpSettings::default(),
        tcp_settings: TcpSettings::default(),
        api_url,
        secret_key: config.secret_key.clone(),
    };

    let agent = PlayitAgent::new(settings, lookup.clone())
        .await
        .map_err(|e| format!("failed to setup agent: {:?}", e))?;

    {
        let mut state_lock = state().lock().expect("state lock poisoned");
        state_lock.keep_running = Some(agent.keep_running());
    }

    tokio::spawn(agent.run());

    loop {
        if *stop_rx.borrow() {
            break;
        }
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(poll_interval) => {
                match api.v1_agents_rundata().await {
                    Ok(data) => {
                        lookup.update_from_run_data(&data).await;
                        update_status_from_rundata(&status, &data);
                    }
                    Err(error) => {
                        let mut status_lock = status.lock().expect("status lock poisoned");
                        status_lock.code = PlayitStatusCode::Error;
                        status_lock.last_error =
                            cstring_sanitize(format!("failed to poll run data: {}", error)).ok();
                    }
                }
            }
        }
    }

    Ok(())
}

fn update_status_from_rundata(
    status: &Arc<Mutex<StatusSnapshot>>,
    data: &playit_api_client::api::AgentRunDataV1,
) {
    let address = data
        .tunnels
        .iter()
        .find(|t| t.disabled_reason.is_none())
        .map(|t| t.display_address.clone());

    let mut status_lock = status.lock().expect("status lock poisoned");
    if let Some(address) = address {
        status_lock.code = PlayitStatusCode::Connected;
        status_lock.last_address = cstring_sanitize(address).ok();
    } else {
        status_lock.code = PlayitStatusCode::Disconnected;
        status_lock.last_address = None;
    }
    status_lock.last_error = None;
}
