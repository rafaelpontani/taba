//! The node's logs as OpenTelemetry *log records* (doc 11, "Logs"): each `tracing` event
//! becomes a record with the OpenTelemetry severity, the fields as attributes (including
//! `event.name`) and the `trace_id` and `span_id` of the span in which the event happened.

use opentelemetry::logs::{AnyValue, LogRecord, Logger, Severity};
use opentelemetry::trace::TraceContextExt;
use std::sync::{Arc, RwLock};

use super::LogSlot;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// The logs layer. It sits in the subscriber from the start; the logger (and the minimum level, checked here
/// and not by a `tracing` filter) is that of the attached pipeline, and without a pipeline nothing goes out.
pub struct LogLayer {
    pub slot: Arc<RwLock<Option<LogSlot>>>,
}

fn severity(level: &Level) -> Severity {
    match *level {
        Level::ERROR => Severity::Error,
        Level::WARN => Severity::Warn,
        Level::INFO => Severity::Info,
        Level::DEBUG => Severity::Debug,
        Level::TRACE => Severity::Trace,
    }
}

struct Fields<'a, R: LogRecord> {
    record: &'a mut R,
}

impl<R: LogRecord> Fields<'_, R> {
    fn put(&mut self, field: &Field, value: AnyValue) {
        if field.name() == "message" {
            self.record.set_body(value);
        } else {
            self.record
                .add_attribute(opentelemetry::Key::new(field.name()), value);
        }
    }
}

impl<R: LogRecord> Visit for Fields<'_, R> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, AnyValue::from(value.to_owned()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, AnyValue::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, AnyValue::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        match i64::try_from(value) {
            Ok(signed) => self.put(field, AnyValue::from(signed)),
            Err(_) => self.put(field, AnyValue::from(value.to_string())),
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, AnyValue::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, AnyValue::from(format!("{value:?}")));
    }
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let slot = self.slot.read().unwrap_or_else(|e| e.into_inner());
        let Some(active) = slot.as_ref() else {
            return;
        };
        if *metadata.level() > active.max_level {
            return;
        }
        let mut record = active.logger.create_log_record();
        record.set_target(metadata.target().to_owned());
        record.set_severity_number(severity(metadata.level()));
        record.set_severity_text(metadata.level().as_str());
        record.set_timestamp(std::time::SystemTime::now());
        event.record(&mut Fields {
            record: &mut record,
        });
        let context = tracing::Span::current().context();
        let span = context.span();
        let span_context = span.span_context();
        if span_context.is_valid() {
            record.set_trace_context(
                span_context.trace_id(),
                span_context.span_id(),
                Some(span_context.trace_flags()),
            );
        }
        active.logger.emit(record);
    }
}
