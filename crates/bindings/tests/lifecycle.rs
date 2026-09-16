use actuate::{Effect, NativeError};
use actuate_bindings::{Dispatcher, Session, parse_options};
use serde_json::{Value, json};
use std::{rc::Rc, sync::mpsc, thread};

struct Affine {
    owner: thread::ThreadId,
    _not_send: Rc<()>,
    dropped: mpsc::Sender<thread::ThreadId>,
}
impl Dispatcher for Affine {
    fn dispatch(&mut self, request: Value) -> actuate::Result<Value> {
        assert_eq!(thread::current().id(), self.owner);
        Ok(request)
    }
}
impl Drop for Affine {
    fn drop(&mut self) {
        self.dropped.send(thread::current().id()).unwrap();
    }
}
#[test]
fn native_lifetime_stays_on_one_thread_even_with_concurrent_callers() {
    let (tx, rx) = mpsc::channel();
    let caller = thread::current().id();
    let session = Session::with_factory(move || {
        Ok(Box::new(Affine {
            owner: thread::current().id(),
            _not_send: Rc::new(()),
            dropped: tx,
        }))
    })
    .unwrap();
    let callers: Vec<_> = (0..12)
        .map(|n| {
            let session = session.clone();
            thread::spawn(move || {
                assert_eq!(session.request(json!({"op":"echo","n":n})).unwrap()["n"], n)
            })
        })
        .collect();
    for caller in callers {
        caller.join().unwrap();
    }
    session.close().unwrap();
    assert_ne!(rx.recv().unwrap(), caller);
    session.close().unwrap();
    assert_eq!(
        session.request(json!({"op":"echo"})).unwrap_err().code,
        "session_closed"
    );
}
#[test]
fn preserves_effects_and_rejects_malformed_input_without_poisoning_session() {
    let session = Session::with_factory(|| {
        Ok(Box::new(|_| {
            Err(NativeError::new("native_failure", "sent").with_effect(Effect::Dispatched))
        }))
    })
    .unwrap();
    let bad: Value = serde_json::from_str(&session.request_json("[]")).unwrap();
    assert_eq!(bad["error"]["effect"], "none");
    let error: Value = serde_json::from_str(&session.request_json(r#"{"op":"action"}"#)).unwrap();
    assert_eq!(error["error"]["effect"], "dispatched");
    session.close().unwrap();
}
#[test]
fn worker_failure_is_uncertain_and_close_joins_it() {
    let session = Session::with_factory(|| {
        Ok(Box::new(|_| -> actuate::Result<Value> {
            panic!("provider failure")
        }))
    })
    .unwrap();
    assert!(matches!(
        session.request(json!({"op":"action"})).unwrap_err().effect,
        Effect::Unknown
    ));
    assert_eq!(session.close().unwrap_err().code, "worker_stopped");
    session.close().unwrap();
}
#[test]
fn connection_validation_and_errors_are_preserved() {
    assert!(parse_options(r#"{"provider":"typo"}"#).is_err());
    assert!(parse_options(r#"{"unexpected":true}"#).is_err());
    let result = Session::with_factory(|| Err(NativeError::new("permission", "denied")));
    assert_eq!(result.err().unwrap().code, "permission");
}
