use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default)]
pub struct Metrics {
    inner: Arc<MetricCounters>,
}

#[derive(Default)]
struct MetricCounters {
    requests_total: AtomicU64,
    responses_2xx: AtomicU64,
    responses_4xx: AtomicU64,
    responses_5xx: AtomicU64,
}

impl Metrics {
    pub fn observe_request(&self) {
        self.inner.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn observe_response(&self, status: u16) {
        match status {
            200..=399 => self.inner.responses_2xx.fetch_add(1, Ordering::Relaxed),
            400..=499 => self.inner.responses_4xx.fetch_add(1, Ordering::Relaxed),
            _ => self.inner.responses_5xx.fetch_add(1, Ordering::Relaxed),
        };
    }

    pub fn render_prometheus(&self) -> String {
        format!(
            "# HELP knotree_registry_requests_total Total HTTP requests.\n# TYPE knotree_registry_requests_total counter\nknotree_registry_requests_total {}\n# HELP knotree_registry_responses_total HTTP responses by status class.\n# TYPE knotree_registry_responses_total counter\nknotree_registry_responses_total{{class=\"2xx\"}} {}\nknotree_registry_responses_total{{class=\"4xx\"}} {}\nknotree_registry_responses_total{{class=\"5xx\"}} {}\n",
            self.inner.requests_total.load(Ordering::Relaxed),
            self.inner.responses_2xx.load(Ordering::Relaxed),
            self.inner.responses_4xx.load(Ordering::Relaxed),
            self.inner.responses_5xx.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counters_in_prometheus_text_format() {
        let metrics = Metrics::default();
        metrics.observe_request();
        metrics.observe_response(200);
        metrics.observe_response(404);
        assert!(
            metrics
                .render_prometheus()
                .contains("knotree_registry_requests_total 1")
        );
        assert!(metrics.render_prometheus().contains("class=\"4xx\"} 1"));
    }
}
