use crate::util::{env, id_gen::gen_id, radix32::radix_32};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    str::FromStr,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tracing::{
    field::Field,
    span::{Attributes, Record},
    Event, Id, Subscriber,
};
use tracing_subscriber::{
    field::Visit,
    fmt::format::FmtSpan,
    layer::{Context, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter,
};

pub fn setup(filter: &str) {
    if env::in_k8s() {
        setup_cloud_native(filter);
    } else if cfg!(debug_assertions) {
        setup_dev(filter);
    } else {
        setup_simple(filter)
    }
}

pub fn setup_dev(filter: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_str(filter).expect("invalid filter"))
        .pretty()
        .with_span_events(FmtSpan::CLOSE)
        .init()
}

pub fn setup_cloud_native(filter: &str) {
    tracing_subscriber::registry()
        .with(EnvFilter::from_str(filter).expect("invalid filter"))
        .with(CloudNativeLayer)
        .init();
}

pub fn setup_simple(filter: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_str(filter).expect("invalid filter"))
        .with_span_events(FmtSpan::CLOSE)
        .init()
}

struct CloudNativeLayer;
impl<S> tracing_subscriber::Layer<S> for CloudNativeLayer
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut span_scope = ctx.span_scope(id).unwrap();
        let span = span_scope.next().unwrap();
        let parent_span = span_scope.next();

        let mut fields = BTreeMap::new();
        let mut visitor = JsonVisitor(&mut fields, 0);
        attrs.record(&mut visitor);

        let trace_id: u128;
        let parent_id: Option<u128>;
        match parent_span {
            None => {
                if visitor.1 != 0 {
                    trace_id = visitor.1;
                } else {
                    trace_id = gen_id();
                }
                parent_id = None;
            }
            Some(parent_span) => {
                let extensions = parent_span.extensions();
                let storage = extensions.get::<Storage>().unwrap();
                trace_id = storage.trace_id;
                parent_id = Some(storage.span_id);
            }
        }

        let storage = Storage {
            trace_id,
            span_id: gen_id(),
            parent_id,
            created_at: Instant::now(),
            enter_at: None,
            busy_time: Duration::default(),
            fields,
        };

        let mut extentions = span.extensions_mut();
        extentions.insert(storage);
        insert_trace_id(id, trace_id);
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(span).unwrap();
        let mut extensions = span.extensions_mut();
        let storage = extensions.get_mut::<Storage>().unwrap();
        let mut visitor = JsonVisitor(&mut storage.fields, 0);
        values.record(&mut visitor);
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        let mut visitor = JsonVisitor(&mut fields, 0);
        event.record(&mut visitor);

        let mut obj: serde_json::map::Map<String, serde_json::Value>;
        match json!({
            "type": "event",
            "level": event.metadata().level().as_str(),
            "fields": fields,
            "target": event.metadata().target(),
            "file": event.metadata().file(),
            "line": event.metadata().line(),
        }) {
            serde_json::Value::Object(o) => { obj = o }
            _ => { panic!("event value is not Object") }
        };

        ctx.event_span(event).map(|span| {
            let extensions = span.extensions();
            let storage = extensions.get::<Storage>().unwrap();
            obj.insert("trace_id".into(), format!("{}", radix_32(storage.trace_id)).into());
            obj.insert("span_id".into(), format!("{}", radix_32(storage.span_id)).into());
        });

        println!("{}", serde_json::to_string(&obj).unwrap_or_else(|e| {
            format!("failed to serialize event, error: {}, name: {}", e, event.metadata().name())
        }))
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        let mut extensions = span.extensions_mut();
        let storage = extensions.get_mut::<Storage>().unwrap();
        storage.enter_at = Some(Instant::now());
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        let mut extensions = span.extensions_mut();
        let storage = extensions.get_mut::<Storage>().unwrap();
        storage.busy_time += storage.enter_at.unwrap().elapsed();
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).unwrap();
        let extensions = span.extensions();
        let storage = extensions.get::<Storage>().unwrap();

        let idle_time = storage.created_at.elapsed() - storage.busy_time;
        let mut obj: serde_json::map::Map<String, serde_json::Value>;
        match json!({
            "type": "span",
            "name": span.metadata().name(),
            "level": span.metadata().level().as_str(),
            "target": span.metadata().target(),
            "file": span.metadata().file(),
            "line": span.metadata().line(),
            "fields": storage.fields,
            "trace_id": format!("{}", radix_32(storage.trace_id)),
            "span_id": format!("{}", radix_32(storage.span_id)),
            "busy_time": format!("{:?}", storage.busy_time),
            "idle_time": format!("{:?}", idle_time),
        }) {
            serde_json::Value::Object(o) => { obj = o }
            _ => { panic!("span value is not Object") }
        }
        storage.parent_id.map(|parent_id| {
            obj.insert("parent_id".into(), format!("{}", radix_32(parent_id)).into());
        });
        println!("{}", serde_json::to_string(&obj).unwrap_or_else(|e| {
            format!("failed to serialize span, error: {}, name: {}", e, span.metadata().name())
        }));
        remove_trace_id(&id);
    }
}

struct Storage {
    trace_id: u128,
    span_id: u128,
    parent_id: Option<u128>,
    created_at: Instant,
    enter_at: Option<Instant>,
    busy_time: Duration,
    fields: BTreeMap<&'static str, serde_json::Value>,
}

struct JsonVisitor<'a>(&'a mut BTreeMap<&'static str, serde_json::Value>, u128);
impl<'a> Visit for JsonVisitor<'a> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name(), serde_json::Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name(), serde_json::Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name(), serde_json::Value::from(value));
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        match field.name() {
            "trace_id" => {
                self.1 = value;
            }
            _ => {
                self.0.insert(field.name(), serde_json::Value::from(value.to_string()));
            }
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name(), serde_json::Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name(), serde_json::Value::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0.insert(field.name(), serde_json::Value::from(format!("{:?}", value)));
    }
}

static TRACE_ID_MAP: LazyLock<Mutex<HashMap<u64, u128>>> = LazyLock::new(|| { Mutex::new(HashMap::new()) });

fn insert_trace_id(id: &Id, trace_id: u128) {
    TRACE_ID_MAP.lock().unwrap().insert(id.into_u64(), trace_id);
}

fn remove_trace_id(id: &Id) {
    TRACE_ID_MAP.lock().unwrap().remove(&id.into_u64());
}

pub fn get_trace_id(id: &Id) -> Option<u128> {
    TRACE_ID_MAP.lock().unwrap().get(&id.into_u64()).map(|v| *v)
}
