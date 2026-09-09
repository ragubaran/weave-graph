//! `weave ask` (`impl.md` M2.4.2, `suges-slm.md` §2.1): natural-language
//! query routing for a human at a terminal. The grounding invariant
//! (§1.2, non-negotiable): the router only selects tools and
//! parameters; every symbol is validated against the index before
//! dispatch — an invented name is reported not-found, corrected on a
//! near-miss, never passed through as though the graph confirmed it.

use std::path::Path;
use std::time::Instant;

use weave_graph_core::{Node, NodeId, Storage};

use crate::slm::{self, IntentRouter, RoutedCall, RouterError};
use crate::{config, query};

const DEFAULT_MODEL: &str = "qwen2.5-coder-0.5b";

/// `slm.model` comes from the repo's own `.weave/config.toml` — the one
/// location M1.5 keeps fixed even under `[storage] home`/`WEAVE_HOME`
/// relocation, so no data-dir resolution is needed to read it.
pub(crate) fn configured_model(root: &Path) -> String {
    config::get_key(&root.join(".weave").join("config.toml"), "slm.model")
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// Grounds a routed symbol against the index: exact match first, then
/// case-insensitive, then a unique substring hit as the near-miss
/// correction (§5.4). Ambiguous or missing names return the closest
/// candidates for the not-found message — never a silent pass-through.
fn ground_symbol(nodes: &[Node], name: &str) -> Result<NodeId, (String, Vec<String>)> {
    if let Some(node) = nodes.iter().find(|n| n.symbol == name) {
        return Ok(node.id);
    }
    if let Some(node) = nodes.iter().find(|n| n.symbol.to_lowercase() == name.to_lowercase()) {
        return Ok(node.id);
    }
    let lower = name.to_lowercase();
    let hits: Vec<&Node> = nodes.iter().filter(|n| n.symbol.to_lowercase().contains(&lower)).collect();
    if hits.len() == 1 {
        return Ok(hits[0].id);
    }
    let closest: Vec<String> = nodes
        .iter()
        .filter(|n| n.symbol.to_lowercase().contains(&lower))
        .take(3)
        .map(|n| n.symbol.clone())
        .collect();
    Err((name.to_string(), closest))
}

/// Validates and corrects a routed call against the index, returning
/// the call that will actually execute (which may differ from the
/// routed one on a near-miss — always shown in the transparency line).
fn ground_call(nodes: &[Node], call: &RoutedCall) -> Result<RoutedCall, String> {
    let symbol = ground_symbol(nodes, &call.symbol)
        .map(|id| nodes.iter().find(|n| n.id == id).map(|n| n.symbol.clone()).unwrap_or_else(|| call.symbol.clone()))
        .map_err(|(name, closest)| {
            if closest.is_empty() {
                format!("symbol not found in index: {name}")
            } else {
                format!("symbol not found in index: {name} (closest: {})", closest.join(", "))
            }
        })?;
    let second = match &call.second {
        Some(name) => Some(
            ground_symbol(nodes, name)
                .map_err(|(name, closest)| {
                    if closest.is_empty() {
                        format!("symbol not found in index: {name}")
                    } else {
                        format!("symbol not found in index: {name} (closest: {})", closest.join(", "))
                    }
                })?,
        ),
        None => None,
    };
    let second = second.map(|id| nodes.iter().find(|n| n.id == id).map(|n| n.symbol.clone()).unwrap_or_default());
    Ok(RoutedCall {
        tool: call.tool.clone(),
        symbol,
        second,
    })
}

pub(crate) fn cmd_ask(
    root: &Path,
    question: &str,
    dry_run: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = crate::open_storage_for_read(root)?;
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
    let symbols: Vec<String> = nodes.iter().map(|n| n.symbol.clone()).collect();

    let model = configured_model(root);
    let router = slm::select_router(&model);
    let started = Instant::now();
    // Graceful degradation (§4.2): an unavailable model falls back to
    // the deterministic router and the output says so.
    let (routed, degraded) = match router.route(question, &symbols) {
        Ok(call) => (call, false),
        Err(RouterError::Unavailable(_)) => (
            slm::FuzzyRouter
                .route(question, &symbols)
                .map_err(|e| e.to_string())?,
            true,
        ),
        Err(e) => return Err(e.to_string().into()),
    };
    let route_ms = started.elapsed().as_secs_f64() * 1000.0;
    let grounded = ground_call(&nodes, &routed)?;
    finish(
        &storage,
        &grounded,
        &routed,
        route_ms,
        &model,
        degraded,
        dry_run,
        json,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish(
    storage: &dyn Storage,
    grounded: &RoutedCall,
    routed: &RoutedCall,
    route_ms: f64,
    model: &str,
    degraded: bool,
    dry_run: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let degraded_note = if degraded { " (model unavailable — deterministic fallback)" } else { "" };
    let corrected = if grounded == routed { "" } else { " (corrected)" };
    if json {
        let mut value = serde_json::json!({
            "routed": {
                "tool": grounded.tool,
                "symbol": grounded.symbol,
                "second": grounded.second,
                "expression": grounded.expression(),
                "router": model,
                "route_ms": route_ms,
            },
        });
        if !dry_run {
            let result = query::run(storage, &grounded.expression())
                .map_err(|e| format!("grounded call failed: {e}"))?;
            value["result"] = serde_json::Value::String(result);
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!(
        "routed: {} [route {route_ms:.1}ms]{degraded_note}",
        grounded.expression()
    );
    if !corrected.is_empty() {
        println!("corrected from: {}{corrected}", routed.expression());
    }
    if dry_run {
        return Ok(());
    }
    let result = query::run(storage, &grounded.expression())
        .map_err(|e| format!("grounded call failed: {e}"))?;
    println!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests;
