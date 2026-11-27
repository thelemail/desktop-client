use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, Global};
use thelemail_core::Core;
use tokio::runtime::{Handle, Runtime};
use tracing_appender::non_blocking::WorkerGuard;

const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

pub struct Services {
    core: Arc<Core>,
    handle: Handle,
    runtime: Option<Runtime>,
    log: Option<WorkerGuard>,
}

impl Global for Services {}

impl Services {
    pub fn new(core: Arc<Core>, runtime: Runtime, log: Option<WorkerGuard>) -> Self {
        Self {
            core,
            handle: runtime.handle().clone(),
            runtime: Some(runtime),
            log,
        }
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    pub fn runtime(&self) -> &Handle {
        &self.handle
    }

    pub fn shutdown(cx: &mut App) {
        let services = cx.global_mut::<Services>();
        services.core.shutdown();
        if let Some(runtime) = services.runtime.take() {
            runtime.shutdown_timeout(SHUTDOWN_GRACE);
        }
        tracing::info!("quit");
        services.log.take();
    }
}

pub fn runtime() -> std::io::Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("core")
        .enable_all()
        .build()
}
