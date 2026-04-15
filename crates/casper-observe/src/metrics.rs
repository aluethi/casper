use prometheus::{
    Encoder, Histogram, IntCounter, IntCounterVec, Registry, TextEncoder,
    histogram_opts, opts,
};

/// Prometheus runtime metrics.
#[derive(Clone)]
pub struct RuntimeMetrics {
    pub registry: Registry,
    pub llm_calls_total: IntCounterVec,
    pub llm_call_duration: Histogram,
    pub tool_calls_total: IntCounterVec,
    pub actor_activations: IntCounter,
    pub actor_dehydrations: IntCounter,
    pub http_requests_total: IntCounterVec,
    pub http_request_duration: Histogram,
}

impl RuntimeMetrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let llm_calls_total = IntCounterVec::new(
            opts!("casper_llm_calls_total", "Total LLM API calls"),
            &["model", "source"],
        )
        .unwrap();

        let llm_call_duration = Histogram::with_opts(
            histogram_opts!(
                "casper_llm_call_duration_seconds",
                "LLM call duration in seconds"
            ),
        )
        .unwrap();

        let tool_calls_total = IntCounterVec::new(
            opts!("casper_tool_calls_total", "Total tool executions"),
            &["tool", "status"],
        )
        .unwrap();

        let actor_activations = IntCounter::new(
            "casper_actor_activations_total",
            "Total actor activations",
        )
        .unwrap();

        let actor_dehydrations = IntCounter::new(
            "casper_actor_dehydrations_total",
            "Total actor dehydrations",
        )
        .unwrap();

        let http_requests_total = IntCounterVec::new(
            opts!("casper_http_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )
        .unwrap();

        let http_request_duration = Histogram::with_opts(
            histogram_opts!(
                "casper_http_request_duration_seconds",
                "HTTP request duration in seconds"
            ),
        )
        .unwrap();

        registry.register(Box::new(llm_calls_total.clone())).unwrap();
        registry.register(Box::new(llm_call_duration.clone())).unwrap();
        registry.register(Box::new(tool_calls_total.clone())).unwrap();
        registry.register(Box::new(actor_activations.clone())).unwrap();
        registry.register(Box::new(actor_dehydrations.clone())).unwrap();
        registry.register(Box::new(http_requests_total.clone())).unwrap();
        registry.register(Box::new(http_request_duration.clone())).unwrap();

        Self {
            registry,
            llm_calls_total,
            llm_call_duration,
            tool_calls_total,
            actor_activations,
            actor_dehydrations,
            http_requests_total,
            http_request_duration,
        }
    }

    /// Render metrics in Prometheus text format.
    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_render() {
        let metrics = RuntimeMetrics::new();
        metrics.llm_calls_total.with_label_values(&["sonnet-4", "api"]).inc();
        metrics.actor_activations.inc();

        let output = metrics.render();
        assert!(output.contains("casper_llm_calls_total"));
        assert!(output.contains("casper_actor_activations_total"));
    }
}
