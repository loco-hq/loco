use std::sync::Arc;

use crate::server::AppState;

pub struct ProjectScope {
    pub user: String,
    pub project: String,
    pub state: Arc<AppState>,
}

impl ProjectScope {
    pub fn project_id(&self) -> String {
        format!("{}/{}", self.user, self.project)
    }
}
