//! Idle actor reaper: background task that scans the actor registry
//! and removes actors that have been idle longer than a configured timeout.
//!
//! Uses [`tokio_util::sync::CancellationToken`] for graceful shutdown.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::actor::ActorRegistry;

/// Configuration for the idle reaper.
pub struct ReaperConfig {
    /// How often to scan for idle actors.
    pub scan_interval: Duration,
    /// Maximum idle time before an actor is reaped.
    pub idle_timeout: Duration,
}

impl Default for ReaperConfig {
    fn default() -> Self {
        Self {
            scan_interval: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Start the idle reaper as a background task.
///
/// Returns a `JoinHandle` that resolves when the reaper exits (either
/// because the cancellation token was triggered or an error occurred).
pub fn start_reaper(
    registry: Arc<ActorRegistry>,
    config: ReaperConfig,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(
            scan_interval_secs = config.scan_interval.as_secs(),
            idle_timeout_secs = config.idle_timeout.as_secs(),
            "idle reaper started"
        );

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("idle reaper shutting down (cancelled)");
                    break;
                }
                _ = tokio::time::sleep(config.scan_interval) => {
                    reap_idle(&registry, config.idle_timeout).await;
                }
            }
        }
    })
}

/// Scan the registry and remove actors idle longer than `timeout`.
async fn reap_idle(registry: &ActorRegistry, timeout: Duration) {
    let now = Instant::now();
    let keys = registry.keys();
    let mut reaped = 0;

    for key in &keys {
        if let Some(last) = registry.last_activity(key).await
            && now.duration_since(last) > timeout
        {
            tracing::info!(actor = %key, "reaping idle actor");
            registry.remove(key);
            reaped += 1;
        }
    }

    if reaped > 0 {
        tracing::info!(
            reaped,
            remaining = registry.active_count(),
            "idle reaper scan complete"
        );
    } else {
        tracing::trace!(
            active = registry.active_count(),
            "idle reaper scan: no idle actors"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaper_config_default() {
        let config = ReaperConfig::default();
        assert_eq!(config.scan_interval.as_secs(), 60);
        assert_eq!(config.idle_timeout.as_secs(), 300);
    }

    #[tokio::test]
    async fn reaper_can_be_cancelled() {
        let registry = Arc::new(ActorRegistry::new(32));
        let cancel = CancellationToken::new();
        let config = ReaperConfig {
            scan_interval: Duration::from_secs(3600), // won't fire
            idle_timeout: Duration::from_secs(1),
        };

        let handle = start_reaper(registry, config, cancel.clone());

        // Cancel immediately
        cancel.cancel();

        // Should exit promptly
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("reaper did not exit in time")
            .expect("reaper task panicked");
    }
}
