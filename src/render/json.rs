use crate::report::Report;

pub fn render(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("report serialises") + "\n"
}
