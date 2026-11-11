//! A `tracing` subscriber for tests: it keeps the name and the fields of every span opened on this
//! thread while it is the default, so a test can check that a use case ran in its span and what
//! the span recorded.
//!
//! The tests of a binary run on many threads at once, and `tracing` caches per callsite whether
//! anyone listens. A callsite first reached on a thread without a recorder could be cached as
//! "never" for the threads that have one; `quiet_default` installs a process default that records
//! nothing and asks every callsite to check again each time, so that cache never says "never".
//!
//! ```ignore
//! let spans = Spans::record();
//! approve(&tracker, "acme:1")?;
//! assert_eq!(spans.field("approve", "ok"), Some("true".to_string()));
//! ```

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::{DefaultGuard, Interest};
use tracing::{Event, Metadata, Subscriber};
use tracing_core::span::Current;

/// One span the recorder saw: its name and the fields it was given, at its start or later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanSeen {
    pub name: String,
    pub fields: BTreeMap<String, String>,
}

/// The spans opened on this thread while the recorder is the default subscriber, in order.
/// Dropping it puts the previous subscriber back.
pub struct Spans {
    seen: Arc<Mutex<Vec<SpanSeen>>>,
    _guard: DefaultGuard,
}

impl Spans {
    /// Makes a fresh recorder the default subscriber of this thread until the value is dropped.
    /// A `#[tokio::test]` runs on one thread, so the spans of its async code are seen too.
    pub fn record() -> Spans {
        quiet_default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Recorder {
            seen: Arc::clone(&seen),
            metadata: Mutex::new(Vec::new()),
            entered: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
        };
        let guard = tracing::subscriber::set_default(recorder);
        tracing_core::callsite::rebuild_interest_cache();
        Spans {
            seen,
            _guard: guard,
        }
    }

    /// Every span seen so far.
    pub fn all(&self) -> Vec<SpanSeen> {
        self.seen.lock().unwrap().clone()
    }

    /// The names of the spans seen so far, in order.
    pub fn names(&self) -> Vec<String> {
        self.all().into_iter().map(|span| span.name).collect()
    }

    /// The field `field` of the last span named `name`, as text (`Debug` of the value; a string
    /// is given without quotes).
    pub fn field(&self, name: &str, field: &str) -> Option<String> {
        self.all()
            .into_iter()
            .rev()
            .find(|span| span.name == name)
            .and_then(|span| span.fields.get(field).cloned())
    }
}

/// Installs, once per process, the default subscriber of the threads without a recorder: it records
/// nothing and answers every callsite `Interest::sometimes()`. A test that sets its own scoped
/// subscriber (`tracing::subscriber::with_default`) calls it first for the same reason.
pub fn quiet_default() {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| {
        // Fails only when the binary already has a default, and then that one stays.
        let _ = tracing::subscriber::set_global_default(Quiet);
    });
}

struct Quiet;

impl Subscriber for Quiet {
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        false
    }

    fn new_span(&self, _attributes: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

struct Recorder {
    seen: Arc<Mutex<Vec<SpanSeen>>>,
    /// Each span's metadata, at the same place as in `seen`.
    metadata: Mutex<Vec<&'static Metadata<'static>>>,
    /// The spans entered and not yet left, innermost last: what `Span::current()` asks for.
    entered: Mutex<Vec<Id>>,
    next: AtomicU64,
}

struct Fields<'a>(&'a mut BTreeMap<String, String>);

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

impl Subscriber for Recorder {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut fields = BTreeMap::new();
        attributes.record(&mut Fields(&mut fields));
        self.metadata.lock().unwrap().push(attributes.metadata());
        let mut seen = self.seen.lock().unwrap();
        seen.push(SpanSeen {
            name: attributes.metadata().name().to_string(),
            fields,
        });
        // An id is the span's place in `seen`, plus one (an id is never zero).
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        debug_assert_eq!(id as usize, seen.len());
        Id::from_u64(id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut seen = self.seen.lock().unwrap();
        if let Some(span) = seen.get_mut(span.into_u64() as usize - 1) {
            values.record(&mut Fields(&mut span.fields));
        }
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, span: &Id) {
        self.entered.lock().unwrap().push(span.clone());
    }

    fn exit(&self, span: &Id) {
        let mut entered = self.entered.lock().unwrap();
        if let Some(at) = entered.iter().rposition(|id| id == span) {
            entered.remove(at);
        }
    }

    fn current_span(&self) -> Current {
        let entered = self.entered.lock().unwrap();
        let Some(id) = entered.last() else {
            return Current::none();
        };
        let metadata = self.metadata.lock().unwrap()[id.into_u64() as usize - 1];
        Current::new(id.clone(), metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tracing::instrument(skip_all, fields(items = items.len(), kept = tracing::field::Empty))]
    fn sort_out(items: &[&str]) -> usize {
        let kept = items.iter().filter(|item| !item.is_empty()).count();
        tracing::Span::current().record("kept", kept);
        kept
    }

    #[test]
    fn a_span_is_seen_with_the_fields_given_at_its_start_and_later() {
        let spans = Spans::record();
        assert_eq!(sort_out(&["a", "", "b"]), 2);
        assert_eq!(spans.names(), ["sort_out"]);
        assert_eq!(spans.field("sort_out", "items"), Some("3".to_string()));
        assert_eq!(spans.field("sort_out", "kept"), Some("2".to_string()));
        assert_eq!(spans.field("sort_out", "missing"), None);
    }

    #[tokio::test]
    async fn the_spans_of_async_code_on_the_test_thread_are_seen() {
        #[tracing::instrument(skip_all, fields(label = "fetch"))]
        async fn fetch() -> u8 {
            tokio::task::yield_now().await;
            7
        }
        let spans = Spans::record();
        assert_eq!(fetch().await, 7);
        assert_eq!(spans.field("fetch", "label"), Some("fetch".to_string()));
    }

    #[tracing::instrument(skip_all)]
    fn after_the_test() {}

    #[test]
    fn nothing_is_seen_once_the_recorder_is_dropped() {
        let spans = Spans::record();
        let seen = Arc::clone(&spans.seen);
        drop(spans);
        after_the_test();
        assert!(seen.lock().unwrap().is_empty());
    }
}
