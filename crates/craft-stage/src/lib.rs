//! craft-stage: mark where a long operation is, so a stall can say where it stuck. **Ours, never
//! offered upstream** (part of the self-logging work; see craft-log).
//!
//! ```
//! let job = craft_stage::scope("select.subject");
//! job.stage("load");
//! job.stage_with("grabcut", &[("iteration", 3.0), ("pixels", 1.2e6)]);
//! // … the scope ends when `job` is dropped
//! ```
//!
//! Code that does not hold the scope (a helper deep in a library) marks a stage on the innermost
//! scope open on its own thread:
//!
//! ```
//! let _job = craft_stage::scope("import.prproj");
//! craft_stage::stage("parse"); // a stage of `import.prproj`
//! craft_stage::stage_with("relink", &[("clips", 12.0)]);
//! ```
//!
//! **One scope per parallel item.** With parallel work under one scope, the last stage call wins,
//! so give each item its own scope and the watch names the stuck one. The free [`stage`] follows
//! the thread, not the work: on a worker thread with no scope of its own it marks nothing.
//!
//! No dependencies, and next to free when nothing listens: until a listener is registered with
//! [`set_hook`] (craft-log does that when a program installs its logger), [`scope`] is one atomic
//! load and returns an inert scope, and its calls do nothing. Tests and other embedders pay
//! nothing. No clock is read here, so it builds and runs anywhere the library does.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Receives scope events. Implemented by craft-log's stall watch.
pub trait Hook: Sync {
    /// A scope named `name` began; `id` is unique in the process.
    fn enter(&self, id: u64, name: &'static str);
    /// Scope `id` moved to stage `stage`; `numbers` are a few (name, value) facts about it.
    fn stage(&self, id: u64, stage: &'static str, numbers: &[(&'static str, f64)]);
    /// Scope `id` ended.
    fn leave(&self, id: u64);
}

static HOOK: OnceLock<&'static dyn Hook> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The innermost scope open on this thread (0: none). Only touched while a listener is
    /// registered.
    static CURRENT: Cell<u64> = const { Cell::new(0) };
}

/// Register the listener. Only the first call takes effect; returns whether this one did.
pub fn set_hook(hook: &'static dyn Hook) -> bool {
    HOOK.set(hook).is_ok()
}

/// Whether a listener is registered.
pub fn listening() -> bool {
    HOOK.get().is_some()
}

/// A named piece of work in progress. Ends when dropped (also when unwinding from a panic).
#[must_use = "the scope ends when this value is dropped"]
pub struct Scope {
    id: u64,
    /// The scope that was current on the creating thread before this one.
    prev: u64,
    hook: Option<&'static dyn Hook>,
}

/// Begin a scope named `name` (a command or job name: `import.prproj`, `export`, `select.subject`).
pub fn scope(name: &'static str) -> Scope {
    let Some(hook) = HOOK.get().copied() else { return Scope { id: 0, prev: 0, hook: None } };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let prev = CURRENT.try_with(|c| c.replace(id)).unwrap_or(0);
    hook.enter(id, name);
    Scope { id, prev, hook: Some(hook) }
}

/// The innermost scope open on this thread moves on to stage `name`; nothing when this thread has
/// no scope open. Without a listener this is one atomic load.
pub fn stage(name: &'static str) {
    stage_with(name, &[]);
}

/// [`stage`], with a few numbers that say how far or how big.
pub fn stage_with(name: &'static str, numbers: &[(&'static str, f64)]) {
    // the hook first: without a listener, thread-local state is never touched
    let Some(hook) = HOOK.get().copied() else { return };
    let id = CURRENT.try_with(Cell::get).unwrap_or(0);
    if id != 0 {
        hook.stage(id, name, numbers);
    }
}

impl Scope {
    /// The scope moves on to stage `name`.
    pub fn stage(&self, name: &'static str) {
        self.stage_with(name, &[]);
    }

    /// The scope moves on to stage `name`, with a few numbers that say how far or how big.
    pub fn stage_with(&self, name: &'static str, numbers: &[(&'static str, f64)]) {
        if let Some(h) = self.hook {
            h.stage(self.id, name, numbers);
        }
    }

    /// Whether anything is listening to this scope.
    pub fn is_active(&self) -> bool {
        self.hook.is_some()
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(h) = self.hook {
            // Restore the creating thread's previous scope, but only where this one is still the
            // current one: a scope dropped on another thread, or out of order, leaves that
            // thread's current scope alone. (The creating thread then keeps this ended id as its
            // current one; stages marked against it are ignored by the listener, which no longer
            // knows the id, until the thread's next scope replaces it.)
            let _ = CURRENT.try_with(|c| {
                if c.get() == self.id {
                    c.set(self.prev);
                }
            });
            h.leave(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn without_a_listener_a_scope_does_nothing() {
        // the listener below is registered by the other test, possibly first: either way this
        // scope must work and never panic
        let s = scope("idle");
        s.stage("one");
        s.stage_with("two", &[("n", 1.0)]);
        drop(s);
    }

    struct Recorder(Mutex<Vec<String>>);
    impl Hook for Recorder {
        fn enter(&self, id: u64, name: &'static str) {
            self.0.lock().unwrap().push(format!("enter {id} {name}"));
        }
        fn stage(&self, id: u64, stage: &'static str, numbers: &[(&'static str, f64)]) {
            self.0.lock().unwrap().push(format!("stage {id} {stage} {numbers:?}"));
        }
        fn leave(&self, id: u64) {
            self.0.lock().unwrap().push(format!("leave {id}"));
        }
    }

    static REC: Recorder = Recorder(Mutex::new(Vec::new()));

    /// Register the one recorder all tests share (the hook can be set once per process).
    fn listen() {
        set_hook(&REC);
        assert!(listening());
    }

    /// The events recorded for scope `id`.
    fn events(id: u64) -> Vec<String> {
        REC.0.lock().unwrap().iter().filter(|l| l.split(' ').nth(1) == Some(&id.to_string())).cloned().collect()
    }

    /// Whether any stage named `stage` was recorded, for any scope.
    fn any_stage(stage: &str) -> bool {
        REC.0.lock().unwrap().iter().any(|l| l.starts_with("stage ") && l.split(' ').nth(2) == Some(stage))
    }

    #[test]
    fn a_listener_hears_enter_stages_and_leave_in_order() {
        listen();
        let id;
        {
            let s = scope("import");
            assert!(s.is_active());
            id = s.id;
            s.stage("parse");
            s.stage_with("relink", &[("clips", 12.0)]);
        }
        assert_eq!(
            events(id),
            [format!("enter {id} import"), format!("stage {id} parse []"), format!("stage {id} relink [(\"clips\", 12.0)]"), format!("leave {id}")]
        );
        assert!(!set_hook(&REC), "only the first listener is taken");
    }

    #[test]
    fn the_free_stage_marks_the_innermost_scope_on_this_thread() {
        listen();
        stage("nest-before-any-scope");
        let outer = scope("nest.outer");
        stage("nest-o1");
        let inner_id;
        {
            let inner = scope("nest.inner");
            inner_id = inner.id;
            stage_with("nest-i1", &[("n", 2.0)]);
        }
        stage("nest-o2");
        let outer_id = outer.id;
        drop(outer);
        stage("nest-after-all-scopes");
        assert_eq!(
            events(outer_id),
            [
                format!("enter {outer_id} nest.outer"),
                format!("stage {outer_id} nest-o1 []"),
                format!("stage {outer_id} nest-o2 []"),
                format!("leave {outer_id}")
            ]
        );
        assert_eq!(events(inner_id), [format!("enter {inner_id} nest.inner"), format!("stage {inner_id} nest-i1 [(\"n\", 2.0)]"), format!("leave {inner_id}")]);
        assert!(!any_stage("nest-before-any-scope") && !any_stage("nest-after-all-scopes"));
    }

    #[test]
    fn a_scope_dropped_on_another_thread_leaves_that_threads_scope_alone() {
        listen();
        let moved = scope("moved.here");
        let moved_id = moved.id;
        let (worker_id, done_id) = std::thread::spawn(move || {
            // the moved scope is not this thread's: a stage here before any scope marks nothing
            stage("moved-worker-without-scope");
            let own = scope("moved.worker");
            drop(moved);
            // still the worker's own scope after the moved one ended here
            stage("moved-worker-own");
            let own_id = own.id;
            drop(own);
            let after = scope("moved.worker.after");
            (own_id, after.id)
        })
        .join()
        .unwrap();
        assert!(!any_stage("moved-worker-without-scope"));
        assert_eq!(
            events(worker_id),
            [format!("enter {worker_id} moved.worker"), format!("stage {worker_id} moved-worker-own []"), format!("leave {worker_id}")]
        );
        assert_eq!(events(moved_id), [format!("enter {moved_id} moved.here"), format!("leave {moved_id}")]);
        // the worker's last scope ended with its value at the end of the closure
        assert_eq!(events(done_id).last(), Some(&format!("leave {done_id}")));
        // on this thread a new scope still nests and restores normally
        let next = scope("moved.next");
        stage("moved-next-stage");
        assert_eq!(events(next.id)[1], format!("stage {} moved-next-stage []", next.id));
    }
}
