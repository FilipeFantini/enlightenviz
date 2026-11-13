//! The pages: the list of boards, and the shell of one board, plus the JSON
//! that tells the page what a board is.
//!
//! The shell holds no data and no inline script or style: the content security
//! policy forbids both. The page's script asks `/api/boards/{board}` for the
//! board's definition and then asks each query for its rows.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::assets::Assets;
use crate::board::{Board, Chart, Control, Engine, Palette, StatFormat};
use crate::param::ParamType;

/// Sent with every HTML response. Nothing may load from another host, no
/// script may be inline or built from text, and the page cannot be framed.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// What the server keeps of a board for its page.
pub struct Page {
    pub title: String,
    /// The libraries the board's charts need, in load order.
    libraries: Vec<&'static str>,
    /// The JSON of `GET /api/boards/{board}`, written once at start.
    pub definition: String,
}

impl Page {
    pub fn new(board: &Board) -> Result<Page, String> {
        let mut libraries: Vec<&'static str> = Vec::new();
        for chart in &board.charts {
            let needs: &[&'static str] = match chart.spec() {
                Some((Engine::VegaLite, _)) => &["vega.min.js", "vega-lite.min.js"],
                Some((Engine::Echarts, _)) => &["echarts.min.js"],
                None => &[],
            };
            for name in needs {
                if !libraries.contains(name) {
                    libraries.push(name);
                }
            }
        }
        Ok(Page {
            title: board.title.clone(),
            libraries,
            definition: definition(board)?,
        })
    }
}

/// Text that goes into HTML, escaped for text and for double-quoted
/// attributes.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn head(title: &str, assets: &Assets) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"color-scheme\" content=\"light dark\">\n\
         <link rel=\"icon\" href=\"data:,\">\n\
         <title>{}</title>\n\
         <link rel=\"stylesheet\" href=\"{}\">\n\
         <script src=\"{}\"></script>\n",
        escape(title),
        assets.path("page.css"),
        assets.path("theme-init.js"),
    )
}

/// The list of boards: id and title, linked.
pub fn index(pages: &BTreeMap<String, Page>, assets: &Assets) -> String {
    let mut html = head("enlightenviz", assets);
    html.push_str("</head>\n<body>\n<main class=\"index\">\n<h1>Boards</h1>\n<ul>\n");
    for (id, page) in pages {
        html.push_str(&format!(
            "<li><a href=\"/boards/{}\">{}</a></li>\n",
            escape(id),
            escape(&page.title)
        ));
    }
    html.push_str("</ul>\n</main>\n</body>\n</html>\n");
    html
}

/// The shell of a board: the scripts that draw it, and an empty container.
pub fn board(page: &Page, assets: &Assets) -> String {
    let mut html = head(&page.title, assets);
    for library in &page.libraries {
        html.push_str(&format!(
            "<script defer src=\"{}\"></script>\n",
            assets.path(library)
        ));
    }
    html.push_str(&format!(
        "<script type=\"module\" src=\"{}\"></script>\n</head>\n<body>\n\
         <div id=\"app\"><p class=\"loading\">Loading the board...</p></div>\n\
         <noscript><p class=\"loading\">This page needs JavaScript to draw the charts.</p></noscript>\n\
         </body>\n</html>\n",
        assets.path("app.js")
    ));
    html
}

#[derive(Serialize)]
struct ParamView<'a> {
    #[serde(rename = "type")]
    ty: ParamType,
    default: Option<&'a Value>,
}

#[derive(Serialize)]
struct FilterView<'a> {
    label: &'a str,
    param: &'a str,
    control: Control,
    /// The type of the parameter the filter drives.
    #[serde(rename = "type")]
    ty: ParamType,
    default: Option<&'a Value>,
    options: Option<Vec<Value>>,
    options_from: Option<Value>,
}

#[derive(Serialize)]
struct StatView<'a> {
    label: &'a str,
    query: &'a str,
    value: &'a str,
    format: Option<StatFormat>,
    hero: bool,
}

#[derive(Serialize)]
struct ChartView<'a> {
    title: &'a str,
    query: &'a str,
    engine: Engine,
    palette: Palette,
    spec: &'a Map<String, Value>,
}

#[derive(Serialize)]
struct QueryView<'a> {
    params: BTreeMap<&'a str, ParamView<'a>>,
}

#[derive(Serialize)]
struct Definition<'a> {
    id: &'a str,
    title: &'a str,
    text: Option<&'a str>,
    filters: Vec<FilterView<'a>>,
    stats: Vec<StatView<'a>>,
    charts: Vec<ChartView<'a>>,
    queries: BTreeMap<&'a str, QueryView<'a>>,
}

fn chart_view(chart: &Chart) -> Result<ChartView<'_>, String> {
    let (engine, spec) = chart
        .spec()
        .ok_or_else(|| format!("the chart `{}` has no single specification", chart.title))?;
    Ok(ChartView {
        title: &chart.title,
        query: &chart.query,
        engine,
        palette: chart.palette.unwrap_or(Palette::Categorical),
        spec,
    })
}

/// The board as the page needs it. A board that reaches here has passed
/// validation, so every filter's parameter is declared by some query and every
/// chart has one specification; if not, that is an error here, not a page that
/// silently lacks a filter or a chart.
fn definition(board: &Board) -> Result<String, String> {
    let queries = board
        .queries
        .iter()
        .map(|(name, query)| {
            let params = query
                .params
                .iter()
                .map(|(p, param)| {
                    (
                        p.as_str(),
                        ParamView {
                            ty: param.ty,
                            default: param.default.as_ref(),
                        },
                    )
                })
                .collect();
            (name.as_str(), QueryView { params })
        })
        .collect();
    let filters = board
        .filters
        .iter()
        .map(|filter| {
            let param = board
                .queries
                .values()
                .find_map(|q| q.params.get(&filter.param))
                .ok_or_else(|| {
                    format!(
                        "the filter `{}` drives a parameter no query declares",
                        filter.label
                    )
                })?;
            Ok(FilterView {
                label: &filter.label,
                param: &filter.param,
                control: filter.control,
                ty: param.ty,
                default: param.default.as_ref(),
                options: filter.options.as_ref().map(|list| {
                    list.iter()
                        .map(|o| serde_json::json!({"value": o.value, "label": o.label}))
                        .collect()
                }),
                options_from: filter.options_from.as_ref().map(
                    |o| serde_json::json!({"query": o.query, "value": o.value, "label": o.label}),
                ),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stats = board
        .stats
        .iter()
        .map(|s| StatView {
            label: &s.label,
            query: &s.query,
            value: &s.value,
            format: s.format,
            hero: s.hero,
        })
        .collect();
    serde_json::to_string(&Definition {
        id: &board.board,
        title: &board.title,
        text: board.text.as_deref(),
        filters,
        stats,
        charts: board
            .charts
            .iter()
            .map(chart_view)
            .collect::<Result<Vec<_>, String>>()?,
        queries,
    })
    .map_err(|e| format!("the board's definition cannot be written: {e}"))
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn html_is_escaped_for_text_and_attributes() {
        assert_eq!(
            escape(r#"<b a="1" b='2'>&"#),
            "&lt;b a=&quot;1&quot; b=&#39;2&#39;&gt;&amp;"
        );
    }
}
