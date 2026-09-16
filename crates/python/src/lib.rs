use actuate_bindings::{Session, envelope, parse_options};
use pyo3::{exceptions::PyRuntimeError, prelude::*};

#[pyclass(frozen, module = "actuate._native")]
struct NativeSession {
    session: Session,
}
#[pymethods]
impl NativeSession {
    #[new]
    fn new(py: Python<'_>, options: String) -> PyResult<Self> {
        py.detach(move || parse_options(&options).and_then(Session::connect))
            .map(|session| Self { session })
            .map_err(|e| {
                PyRuntimeError::new_err(
                    serde_json::to_string(&e).expect("serializable native error"),
                )
            })
    }
    fn request(&self, py: Python<'_>, request: String) -> String {
        py.detach(|| self.session.request_json(&request))
    }
    fn close(&self, py: Python<'_>) -> String {
        py.detach(|| envelope(self.session.close().map(|_| serde_json::Value::Null)))
    }
}
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeSession>()
}
