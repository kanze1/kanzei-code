//! Admission to a shared local inference service. Interactive runs reserve the
//! service across tool steps; maintenance yields only its uncommitted inference.
//! Cloud endpoints retain their own concurrency policy.
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::LlmError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestPriority {
    #[default]
    Interactive,
    Maintenance,
}

#[derive(Default)]
struct State {
    interactive: usize,
    maintenance: Option<CancellationToken>,
}

#[derive(Default)]
struct Service {
    state: Mutex<State>,
    changed: Notify,
}

fn service(base_url: &str) -> Option<Arc<Service>> {
    static SERVICES: OnceLock<Mutex<HashMap<String, Weak<Service>>>> = OnceLock::new();
    let url = url::Url::parse(base_url).ok()?;
    let host = match url.host()? {
        url::Host::Domain("localhost") => "127.0.0.1".to_owned(),
        url::Host::Ipv4(ip) if ip.is_loopback() => ip.to_string(),
        url::Host::Ipv6(ip) if ip.is_loopback() => ip.to_string(),
        _ => return None,
    };
    let key = format!("{host}:{}", url.port_or_known_default()?);
    let mut services = SERVICES.get_or_init(Default::default).lock().unwrap();
    services.retain(|_, service| service.strong_count() > 0);
    if let Some(service) = services.get(&key).and_then(Weak::upgrade) {
        return Some(service);
    }
    let service = Arc::new(Service::default());
    services.insert(key, Arc::downgrade(&service));
    Some(service)
}

/// An interactive reservation starts before request preparation and survives tool
/// execution. Dropping it (including cancellation) wakes deferred maintenance.
pub struct InteractiveReservation(Option<Arc<Service>>);

pub fn reserve_interactive(base_url: &str) -> InteractiveReservation {
    let service = service(base_url);
    if let Some(service) = &service {
        let mut state = service.state.lock().unwrap();
        state.interactive += 1;
        if let Some(active) = &state.maintenance {
            active.cancel();
        }
    }
    InteractiveReservation(service)
}

impl Drop for InteractiveReservation {
    fn drop(&mut self) {
        if let Some(service) = &self.0 {
            service.state.lock().unwrap().interactive -= 1;
            service.changed.notify_waiters();
        }
    }
}

pub struct Admission {
    service: Option<Arc<Service>>,
    reservation: Option<InteractiveReservation>,
    yielded: CancellationToken,
}

impl Admission {
    pub async fn yielded(&self) {
        self.yielded.cancelled().await;
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if self.reservation.is_none() {
            if let Some(service) = &self.service {
                service.state.lock().unwrap().maintenance = None;
                service.changed.notify_waiters();
            }
        }
    }
}

pub async fn admit(base_url: &str, priority: RequestPriority) -> Admission {
    let reservation =
        (priority == RequestPriority::Interactive).then(|| reserve_interactive(base_url));
    let service = reservation
        .as_ref()
        .and_then(|r| r.0.clone())
        .or_else(|| service(base_url));
    let yielded = CancellationToken::new();
    if let Some(service) = &service {
        loop {
            // Register before inspecting state, so a release cannot be lost.
            let changed = service.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = service.state.lock().unwrap();
                if state.maintenance.is_none()
                    && (priority == RequestPriority::Interactive || state.interactive == 0)
                {
                    if priority == RequestPriority::Maintenance {
                        state.maintenance = Some(yielded.clone());
                    }
                    break;
                }
            }
            changed.await;
        }
    }
    Admission {
        service,
        reservation,
        yielded,
    }
}

/// Re-enter a pure, uncommitted inference operation after an interactive yield.
/// Never wrap tool execution or external writes in this boundary.
pub async fn maintenance<T, F, Fut>(base_url: &str, mut inference: F) -> Result<T, LlmError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, LlmError>>,
{
    loop {
        let admission = admit(base_url, RequestPriority::Maintenance).await;
        let result = tokio::select! {
            biased;
            _ = admission.yielded() => None,
            result = inference() => Some(result),
        };
        drop(admission);
        if let Some(result) = result {
            return result;
        }
        tracing::info!("maintenance inference yielded to interactive work");
    }
}

#[cfg(test)]
mod tests;
