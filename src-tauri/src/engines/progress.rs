//! Measured progress for the current engine operation, never a predicted installation total.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Progress {
    pub text: String,
    pub fraction: Option<f32>,
    pub stage: Option<u64>,
    pub stages: Option<u64>,
    pub done: Option<f64>,
    pub total: Option<f64>,
    pub unit: String,
    pub estimated: bool,
    pub eta_seconds: Option<f64>,
    pub elapsed_seconds: Option<f64>,
}
impl Progress {
    pub fn message(text: impl Into<String>) -> Self { Self { text:text.into(), ..Default::default() } }
}
