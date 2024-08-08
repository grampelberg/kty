use std::sync::Arc;

use k8s_openapi::api::core::v1::Pod;

pub struct Shell {
    pod: Arc<Pod>,
}

impl Shell {
    pub fn new(pod: Arc<Pod>) -> Self {
        Self { pod }
    }
}
