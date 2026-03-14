use std::sync::Arc;

use axum::{Router, response::Html, routing::get};

use crate::app_state::AppState;

const CONTROL_UI_HTML: &str = include_str!("../../templates/frontend/control_ui.html");

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(page))
}

async fn page() -> Html<&'static str> {
    Html(CONTROL_UI_HTML)
}

#[cfg(test)]
mod tests {
    use super::CONTROL_UI_HTML;

    #[test]
    fn control_ui_markup_contains_expected_sections() {
        assert!(CONTROL_UI_HTML.contains("Dawn Personal Workbench"));
        assert!(CONTROL_UI_HTML.contains("id=\"bootstrap-form\""));
        assert!(CONTROL_UI_HTML.contains("id=\"task-form\""));
        assert!(CONTROL_UI_HTML.contains("/api/gateway/identity/status"));
        assert!(CONTROL_UI_HTML.contains("/api/a2a/task"));
    }
}
