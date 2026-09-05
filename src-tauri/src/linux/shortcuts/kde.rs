//! KGlobalAccel transport. Only typed activations and connection health escape.
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbus::blocking::Connection;
use dbus::message::MatchRule;

const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const START_SERVICE_FLAGS: u32 = 0;
const SERVICE: &str = "org.kde.kglobalaccel";
const ROOT: &str = "/kglobalaccel";
const INTERFACE: &str = "org.kde.KGlobalAccel";
const COMPONENT_INTERFACE: &str = "org.kde.kglobalaccel.Component";
const COMPONENT: &str = "copywraith";
const TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const PRESSED: &str = "globalShortcutPressed";
const RELEASED: &str = "globalShortcutReleased";
const SET_PRESENT: u32 = 2;
type Keys = Vec<(Vec<i32>,)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Activation {
    Toggle,
    Starred,
    Plaintext,
}

impl Activation {
    const ALL: [Self; 3] = [Self::Toggle, Self::Starred, Self::Plaintext];

    fn id(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Starred => "starred",
            Self::Plaintext => "paste-plaintext",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Toggle => "Toggle popup",
            Self::Starred => "Starred popup",
            Self::Plaintext => "Paste as plain text",
        }
    }

    fn identity(self) -> Vec<&'static str> {
        vec![COMPONENT, self.id(), "Copywraith", self.label()]
    }
}

pub(super) struct Session {
    connection: Connection,
    owner: String,
    component_path: String,
    // Keep cleanup ownership even before registration finishes or replies time out.
    registered: Vec<(String, Activation)>,
    held: HashSet<Activation>,
    pending: Arc<Mutex<Vec<dbus::Message>>>,
}

impl Session {
    pub(super) fn connect() -> Result<Self, String> {
        let connection = Connection::new_session().map_err(|e| e.to_string())?;
        let pending = Arc::new(Mutex::new(Vec::new()));
        // Both KF5 repeats and KF6 releases pass through the same authentication.
        // Subscribe before registration so no initial press/release is missed.
        for member in [PRESSED, RELEASED] {
            let signals = pending.clone();
            connection
                .add_match(
                    MatchRule::new_signal(COMPONENT_INTERFACE, member).with_sender(SERVICE),
                    move |_: (String, String, i64), _, message| {
                        signals
                            .lock()
                            .unwrap()
                            .push(message.duplicate().expect("signal copy"));
                        true
                    },
                )
                .map_err(|e| e.to_string())?;
        }

        Ok(Self {
            connection,
            owner: String::new(),
            component_path: String::new(),
            registered: Vec::new(),
            held: HashSet::new(),
            pending,
        })
    }

    pub(super) fn poll(&mut self) -> Result<Vec<Activation>, String> {
        self.connection
            .process(POLL_INTERVAL)
            .map_err(|e| e.to_string())?;
        let owner = self.resolve_owner()?;

        // Polling also catches missed owner changes and service replacement.
        // Address registration to that unique owner, never to a moving alias.
        if owner != self.owner {
            // Old unique owners cannot authenticate events from a replacement.
            let _ = self.deactivate();
            self.owner.clear();
            self.held.clear();
            self.pending.lock().unwrap().clear();
            self.register(&owner)?;
            self.owner = owner;
        }

        let pending: Vec<_> = self.pending.lock().unwrap().drain(..).collect();
        Ok(pending
            .iter()
            .filter_map(|message| self.handle_signal(message))
            .collect())
    }

    pub(super) fn deactivate(&mut self) -> Result<(), String> {
        self.held.clear();
        let mut errors = Vec::new();
        // Attempt every action even if one owner or method fails. Keep failures
        // for Drop to retry; successful cleanup must not run twice.
        self.registered.retain(|(owner, action)| {
            let proxy = self.connection.with_proxy(owner, ROOT, TIMEOUT);
            match proxy.method_call::<(), _, _, _>(INTERFACE, "setInactive", (action.identity(),)) {
                Ok(()) => false,
                Err(error) => {
                    errors.push(error.to_string());
                    true
                }
            }
        });
        if errors.is_empty() {
            return Ok(());
        }
        Err(errors.join("; "))
    }

    fn handle_signal(&mut self, message: &dbus::Message) -> Option<Activation> {
        let action = self.activation(message)?;
        match &*message.member()? {
            RELEASED => {
                self.held.remove(&action);
                None
            }
            PRESSED if self.held.insert(action) => Some(action),
            _ => None,
        }
    }

    fn resolve_owner(&self) -> Result<String, String> {
        let bus = self.connection.with_proxy(BUS, BUS_PATH, TIMEOUT);
        let result: Result<(String,), dbus::Error> =
            bus.method_call(BUS, "GetNameOwner", (SERVICE,));
        match result {
            Ok((owner,)) => return Ok(owner),
            Err(error) if error.name() == Some("org.freedesktop.DBus.Error.NameHasNoOwner") => {}
            Err(error) => return Err(error.to_string()),
        }

        // Plasma can launch the service lazily, just as KDE's own client does.
        let (_result,): (u32,) = bus
            .method_call(BUS, "StartServiceByName", (SERVICE, START_SERVICE_FLAGS))
            .map_err(|e| e.to_string())?;
        let (owner,): (String,) = bus
            .method_call(BUS, "GetNameOwner", (SERVICE,))
            .map_err(|e| e.to_string())?;
        Ok(owner)
    }

    fn register(&mut self, owner: &str) -> Result<(), String> {
        let proxy = self.connection.with_proxy(owner, ROOT, TIMEOUT);
        for action in Activation::ALL {
            let registration = (owner.to_string(), action);
            if !self.registered.contains(&registration) {
                self.registered.push(registration);
            }
            proxy
                .method_call::<(), _, _, _>(INTERFACE, "doRegister", (action.identity(),))
                .map_err(|e| e.to_string())?;

            // SetPresent initializes fresh actions. Omitting NoAutoloading
            // preserves KDE assignments, including intentionally empty ones.
            let (_assigned,): (Keys,) = proxy
                .method_call(
                    INTERFACE,
                    "setShortcutKeys",
                    (action.identity(), Keys::new(), SET_PRESENT),
                )
                .map_err(|e| e.to_string())?;
        }
        let (path,): (dbus::Path<'static>,) = proxy
            .method_call(INTERFACE, "getComponent", (COMPONENT,))
            .map_err(|e| e.to_string())?;
        self.component_path = path.to_string();
        Ok(())
    }

    fn activation(&self, message: &dbus::Message) -> Option<Activation> {
        if &*message.sender()? != self.owner.as_str()
            || &*message.path()? != self.component_path.as_str()
        {
            return None;
        }
        let (component, action, _timestamp) = message.read3::<String, String, i64>().ok()?;
        if component != COMPONENT {
            return None;
        }
        Activation::ALL
            .into_iter()
            .find(|candidate| candidate.id() == action)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Error returns and quitting during retry must also release presence.
        let _ = self.deactivate();
    }
}

#[cfg(test)]
#[path = "kde_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "kde_runtime_tests.rs"]
mod runtime_tests;
