//! `view`: one self-contained HTML file (no network, no external assets) with the report embedded as JSON.

use crate::report::Report;

const TEMPLATE: &str = include_str!("view.html");
const PLACEHOLDER: &str = "__PRDIFF_DATA__";

pub fn render(report: &Report) -> String {
    let json = serde_json::to_string(report).expect("report serialises");
    // Inside <script>, `<` could end the element (`</script>`) or open a comment; < is the same JSON string.
    let json = json.replace('<', "\\u003c");
    TEMPLATE.replacen(PLACEHOLDER, &json, 1)
}
