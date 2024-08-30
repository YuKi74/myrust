use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Debug;
use std::str::FromStr;
use tracing::{Event, Id, Subscriber};
use tracing::field::{Field, Visit};
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

pub fn init(level: &str) {
    let level = tracing::Level::from_str(level)
        .expect(&format!("Invalid level: {}", level));
    // tracing_subscriber::fmt()
    //     .with_max_level(level)
    //     .with_writer(std::io::stdout)
    //     .with_ansi(false)
    //     .without_time()
    //     .with_span_events(FmtSpan::ACTIVE)
    //     .json()
    //     .init()
    tracing_subscriber::registry().with(CustomLayer).init();
}

struct CustomLayer;
impl<S: Subscriber> Layer<S> for CustomLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        println!("Got event!");
        println!("  level={:?}", event.metadata().level());
        println!("  target={:?}", event.metadata().target());
        println!("  name={:?}", event.metadata().name());
        println!("  file={:?}, line={:?}", event.metadata().file(), event.metadata().line());
        for field in event.fields() {
            println!("  field={}", field.name());
        }
    }
}

struct JsonVisitor(BTreeMap<&'static str,serde_json::Value>);
impl Visit for JsonVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name(),serde_json::json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        todo!()
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        todo!()
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        todo!()
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        todo!()
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        todo!()
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        todo!()
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        todo!()
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_tracing() {
        init("info");
        tracing::info!("hello world");
    }
}