# Metrics Service (metrics)

The Metrics Service in Yagna is responsible for collecting, aggregating, and exposing performance metrics from various components of the platform. It provides valuable insights into the operation and health of the Yagna ecosystem, enabling monitoring, optimization, and troubleshooting.

## Key Features

1. **Metric Collection**: Gathers metrics from different Yagna components and services.
2. **Metric Types**: Supports various metric types such as counters, gauges, and histograms.
3. **Aggregation**: Combines metrics from multiple sources and computes aggregate values.
4. **Exposure**: Provides interfaces for querying and retrieving metrics data.
5. **Integration**: Easily integrates with external monitoring and visualization tools.

## Metric Categories

The Metrics Service collects various types of metrics:

1. **System Metrics**: CPU usage, memory consumption, disk I/O, etc.
2. **Network Metrics**: Bandwidth usage, connection counts, latency, etc.
3. **Application Metrics**: Component-specific metrics for market, payment, activity, etc.
4. **Performance Metrics**: Response times, throughput, error rates, etc.

## Metric Collection Methods

The service employs different methods to collect metrics:

1. **Push-based**: Components actively push their metrics to the Metrics Service.
2. **Pull-based**: The Metrics Service periodically polls components for their metrics.
3. **Event-driven**: Metrics are collected based on specific events or triggers.

## Integration with Other Components

The Metrics Service interacts with all major Yagna components to collect relevant metrics:

1. **Network (net)**: Collects network performance and usage metrics.
2. **Marketplace (market)**: Gathers metrics on offer/demand matching, negotiations, etc.
3. **Payment**: Collects metrics on transaction volumes, processing times, etc.
4. **Activity**: Measures task execution times, resource utilization, etc.
5. **GSB (Service Bus)**: Monitors message volumes, routing performance, etc.

## Code Example: Collecting and Exposing Metrics

Here's a simplified example of how metrics might be collected and exposed using the Metrics Service:

\```rust
use ya_metrics::{MetricsService, Counter, Gauge, Histogram};

struct MyComponentMetrics {
    requests: Counter,
    active_connections: Gauge,
    response_time: Histogram,
}

impl MyComponentMetrics {
    fn new(metrics: &MetricsService) -> Self {
        Self {
            requests: metrics.register_counter("my_component_requests_total"),
            active_connections: metrics.register_gauge("my_component_active_connections"),
            response_time: metrics.register_histogram("my_component_response_time_seconds"),
        }
    }

    fn record_request(&self) {
        self.requests.inc(1);
    }

    fn set_active_connections(&self, count: i64) {
        self.active_connections.set(count);
    }

    fn observe_response_time(&self, duration: f64) {
        self.response_time.observe(duration);
    }
}

async fn run_metrics_server(metrics: MetricsService) -> Result<(), Box<dyn std::error::Error>> {
    metrics.serve_http("127.0.0.1:8080").await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metrics_service = MetricsService::new();
    let my_metrics = MyComponentMetrics::new(&metrics_service);

    // Simulate some activity
    my_metrics.record_request();
    my_metrics.set_active_connections(5);
    my_metrics.observe_response_time(0.1);

    // Start the metrics server
    run_metrics_server(metrics_service).await?;
    Ok(())
}
\```

This example demonstrates:
1. Defining custom metrics for a component.
2. Registering these metrics with the Metrics Service.
3. Updating metric values based on component activity.
4. Exposing the collected metrics via an HTTP server.

The Metrics Service plays a crucial role in maintaining the health and performance of the Yagna platform by providing visibility into its operations and enabling data-driven optimization and troubleshooting.