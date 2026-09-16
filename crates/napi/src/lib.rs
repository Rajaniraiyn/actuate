use actuate_bindings::{Session, envelope, parse_options};
use napi::{Env, Result, Task, bindgen_prelude::AsyncTask};
use napi_derive::napi;

#[napi]
pub struct NativeSession {
    session: Session,
}
#[napi]
impl NativeSession {
    #[napi]
    pub fn request(&self, request: String) -> AsyncTask<Request> {
        AsyncTask::new(Request {
            session: self.session.clone(),
            request,
        })
    }
    #[napi]
    pub fn close(&self) -> AsyncTask<Close> {
        AsyncTask::new(Close {
            session: self.session.clone(),
        })
    }
}
pub struct Connect {
    options: String,
}
#[napi]
impl Task for Connect {
    type Output = Session;
    type JsValue = NativeSession;
    fn compute(&mut self) -> Result<Session> {
        parse_options(&self.options)
            .and_then(Session::connect)
            .map_err(|e| {
                napi::Error::from_reason(
                    serde_json::to_string(&e).expect("serializable native error"),
                )
            })
    }
    fn resolve(&mut self, _: Env, session: Session) -> Result<NativeSession> {
        Ok(NativeSession { session })
    }
}
pub struct Request {
    session: Session,
    request: String,
}
#[napi]
impl Task for Request {
    type Output = String;
    type JsValue = String;
    fn compute(&mut self) -> Result<String> {
        Ok(self.session.request_json(&self.request))
    }
    fn resolve(&mut self, _: Env, output: String) -> Result<String> {
        Ok(output)
    }
}
pub struct Close {
    session: Session,
}
#[napi]
impl Task for Close {
    type Output = String;
    type JsValue = String;
    fn compute(&mut self) -> Result<String> {
        Ok(envelope(
            self.session.close().map(|_| serde_json::Value::Null),
        ))
    }
    fn resolve(&mut self, _: Env, output: String) -> Result<String> {
        Ok(output)
    }
}

#[napi]
pub fn connect(options: String) -> AsyncTask<Connect> {
    AsyncTask::new(Connect { options })
}
