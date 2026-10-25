use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::config::{ApiConfig, Rank};

pub struct ApiProblem {
    pub id: Option<String>,
    pub rank_display: Option<String>,
    pub description: Option<String>,
    pub sgf: String,
}

pub async fn fetch_problem(
    client: &reqwest::Client,
    api: &ApiConfig,
    key: &str,
    rank: Rank,
) -> Result<ApiProblem> {
    let mut req = client
        .get(&api.url)
        .query(&[
            (api.rank_param.as_str(), rank.display()),
            (api.user_id_param.as_str(), api.user_id.clone()),
        ])
        .header("x-rapidapi-key", key);
    if let Some(host) = &api.host {
        req = req.header("x-rapidapi-host", host);
    }

    let resp = req.send().await.context("API-Anfrage fehlgeschlagen")?;
    let status = resp.status();
    let body = resp.text().await.context("API-Antwort nicht lesbar")?;
    if !status.is_success() {
        bail!("API antwortete mit {status}: {body}");
    }

    let json: Value = serde_json::from_str(&body)
        .with_context(|| format!("API-Antwort ist kein JSON: {body}"))?;
    let obj = find_problem_object(&json)
        .ok_or_else(|| anyhow!("Antwort enthält kein Feld 'sgf_content': {body}"))?;

    let text = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_owned);
    Ok(ApiProblem {
        id: text("id"),
        rank_display: text("rank_display"),
        description: text("description"),
        sgf: text("sgf_content").expect("von find_problem_object geprüft"),
    })
}

/// Findet das Objekt mit `sgf_content`; toleriert Arrays und Wrapper wie `data`.
fn find_problem_object(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(map) => {
            if map.get("sgf_content").is_some_and(Value::is_string) {
                return Some(v);
            }
            map.values().find_map(find_problem_object)
        }
        Value::Array(items) => items.iter().find_map(find_problem_object),
        _ => None,
    }
}
