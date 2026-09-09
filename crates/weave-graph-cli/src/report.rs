use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use weave_graph_core::{CommunityId, NodeId, Storage, louvain_communities};

use crate::provenance::{self, Provenance};

/// `plan.md` §1.3a's hard budget: no canvas emits more than this many
/// top-level nodes. Over-budget regions collapse into one node linking to
/// a sub-canvas holding the rest.
const NODE_BUDGET: usize = 200;

const NODE_WIDTH: i32 = 260;
const NODE_HEIGHT: i32 = 80;
const GRID_GAP: i32 = 40;

#[derive(Serialize)]
struct CanvasNode {
    id: String,
    #[serde(rename = "type")]
    node_type: &'static str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

#[derive(Serialize)]
struct CanvasEdge {
    id: String,
    #[serde(rename = "fromNode")]
    from_node: String,
    #[serde(rename = "toNode")]
    to_node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

#[derive(Serialize, Default)]
struct Canvas {
    nodes: Vec<CanvasNode>,
    edges: Vec<CanvasEdge>,
}

struct Module {
    id: CommunityId,
    label: String,
    files: Vec<String>,
}

pub(crate) struct ReportPaths {
    pub(crate) report_md: PathBuf,
    pub(crate) canvas_files: Vec<PathBuf>,
}

/// `weave report` (`plan.md` §1.3a): LOD 0 (repos), LOD 1 (architectural
/// modules via Louvain over the file-dependency graph, the default view),
/// and LOD 2 (one sub-canvas per module, file-level) — LOD 3 is
/// `weave export`, already wired in M1.6. Every artifact carries the same
/// provenance badge.
pub(crate) fn generate(
    root: &Path,
    out_dir: &Path,
    db_path: &Path,
    storage: &dyn Storage,
    doc_provenance_section: Option<&str>,
) -> Result<ReportPaths, Box<dyn std::error::Error>> {
    let nodes = storage.all_nodes()?;
    let edges = storage.all_edges()?;
    let provenance = provenance::current(root, db_path);

    let file_of: HashMap<NodeId, String> = nodes.iter().map(|n| (n.id, n.path.clone())).collect();
    let file_edges = aggregate_file_edges(&nodes, &edges, &file_of);
    let modules = build_modules(&nodes, &file_edges);

    fs::create_dir_all(out_dir)?;

    let repo_canvas_path = out_dir.join("weave-report.canvas");
    write_canvas(&repo_canvas_path, &repo_canvas(&nodes))?;

    let modules_canvas_path = out_dir.join("weave-modules.canvas");
    let (modules_canvas, overflow) = modules_canvas(&modules, &file_edges, out_dir);
    write_canvas(&modules_canvas_path, &modules_canvas)?;

    let mut canvas_files = vec![repo_canvas_path, modules_canvas_path];
    for (overflow_path, overflow_canvas) in overflow {
        write_canvas(&overflow_path, &overflow_canvas)?;
        canvas_files.push(overflow_path);
    }

    for module in &modules {
        let path = out_dir.join(format!("weave-module-{}.canvas", module.id));
        write_canvas(&path, &module_canvas(module, &file_edges))?;
        canvas_files.push(path);
    }

    let report_md = out_dir.join("WEAVE_REPORT.md");
    fs::write(
        &report_md,
        render_report_md(&provenance, &nodes, &modules, doc_provenance_section),
    )?;

    Ok(ReportPaths {
        report_md,
        canvas_files,
    })
}

/// One aggregated weight per unordered file pair — how many symbol-level
/// edges cross between the two files. Self-file edges are dropped: they
/// say nothing about *inter*-file architecture, which is what LOD 1/2 map.
fn aggregate_file_edges(
    nodes: &[weave_graph_core::Node],
    edges: &[weave_graph_core::Edge],
    file_of: &HashMap<NodeId, String>,
) -> HashMap<(String, String), f64> {
    let mut weights: HashMap<(String, String), f64> = HashMap::new();
    for edge in edges {
        let (Some(a), Some(b)) = (file_of.get(&edge.source_id), file_of.get(&edge.target_id))
        else {
            continue;
        };
        if a == b {
            continue;
        }
        let key = if a <= b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        };
        *weights.entry(key).or_insert(0.0) += 1.0;
    }
    let _ = nodes;
    weights
}

fn build_modules(
    nodes: &[weave_graph_core::Node],
    file_edges: &HashMap<(String, String), f64>,
) -> Vec<Module> {
    let mut files: Vec<String> = nodes.iter().map(|n| n.path.clone()).collect();
    files.sort();
    files.dedup();

    // Louvain wants integer node ids; map each distinct file path to one.
    let file_id: HashMap<&str, NodeId> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i as NodeId))
        .collect();
    let id_edges: Vec<(NodeId, NodeId, f64)> = file_edges
        .iter()
        .filter_map(|((a, b), w)| Some((*file_id.get(a.as_str())?, *file_id.get(b.as_str())?, *w)))
        .collect();
    let ids: Vec<NodeId> = (0..files.len() as NodeId).collect();
    let communities = louvain_communities(&ids, &id_edges);

    let mut by_module: HashMap<CommunityId, Vec<String>> = HashMap::new();
    for (i, file) in files.iter().enumerate() {
        let community = communities.get(&(i as NodeId)).copied().unwrap_or(0);
        by_module.entry(community).or_default().push(file.clone());
    }

    let mut modules: Vec<Module> = by_module
        .into_iter()
        .map(|(id, mut files)| {
            files.sort();
            let label = module_label(&files);
            Module { id, label, files }
        })
        .collect();
    modules.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.id.cmp(&b.id)));
    modules
}

/// Names a module after the directory most of its files share — falls
/// back to a bare id when the files don't agree on one (mixed top-level
/// dirs, or single-file modules with no meaningful shared prefix).
fn module_label(files: &[String]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for f in files {
        let dir = f.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if !dir.is_empty() {
            *counts.entry(dir).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| "(root)".to_string())
}

fn grid_position(index: usize, columns: usize) -> (i32, i32) {
    let col = (index % columns) as i32;
    let row = (index / columns) as i32;
    (
        col * (NODE_WIDTH + GRID_GAP),
        row * (NODE_HEIGHT + GRID_GAP),
    )
}

fn columns_for(count: usize) -> usize {
    (count as f64).sqrt().ceil().max(1.0) as usize
}

/// LOD 0: one node per distinct `repo_id` — today always exactly one
/// (`"local"`, single-repo), since cross-repo federation isn't built yet.
fn repo_canvas(nodes: &[weave_graph_core::Node]) -> Canvas {
    let mut repo_ids: Vec<&str> = nodes.iter().map(|n| n.repo_id.as_str()).collect();
    repo_ids.sort();
    repo_ids.dedup();
    let columns = columns_for(repo_ids.len().max(1));
    let canvas_nodes = repo_ids
        .iter()
        .enumerate()
        .map(|(i, repo_id)| {
            let (x, y) = grid_position(i, columns);
            CanvasNode {
                id: format!("repo-{repo_id}"),
                node_type: "text",
                x,
                y,
                width: NODE_WIDTH,
                height: NODE_HEIGHT,
                text: Some(format!("# {repo_id}")),
                file: None,
                label: None,
            }
        })
        .collect();
    Canvas {
        nodes: canvas_nodes,
        edges: Vec::new(),
    }
}

/// LOD 1: one node per module, budget-capped — modules beyond the top
/// `NODE_BUDGET - 1` collapse into one `file`-type node linking to an
/// overflow sub-canvas (Obsidian follows `file` nodes; that's the
/// "drill-down link" `plan.md` §1.3a calls for).
fn modules_canvas(
    modules: &[Module],
    file_edges: &HashMap<(String, String), f64>,
    out_dir: &Path,
) -> (Canvas, Vec<(PathBuf, Canvas)>) {
    let (visible, overflow) = if modules.len() > NODE_BUDGET {
        modules.split_at(NODE_BUDGET - 1)
    } else {
        (modules, &[][..])
    };

    let columns = columns_for(visible.len() + usize::from(!overflow.is_empty()));
    let mut nodes: Vec<CanvasNode> = visible
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let (x, y) = grid_position(i, columns);
            CanvasNode {
                id: format!("module-{}", m.id),
                node_type: "text",
                x,
                y,
                width: NODE_WIDTH,
                height: NODE_HEIGHT,
                text: Some(format!("{} ({} files)", m.label, m.files.len())),
                file: None,
                label: None,
            }
        })
        .collect();

    let module_index: HashMap<CommunityId, usize> =
        visible.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
    let file_module: HashMap<&str, CommunityId> = visible
        .iter()
        .flat_map(|m| m.files.iter().map(move |f| (f.as_str(), m.id)))
        .collect();

    let mut inter_module_weight: HashMap<(CommunityId, CommunityId), f64> = HashMap::new();
    for ((a, b), w) in file_edges {
        let (Some(&ma), Some(&mb)) = (file_module.get(a.as_str()), file_module.get(b.as_str()))
        else {
            continue;
        };
        if ma == mb {
            continue;
        }
        let key = if ma <= mb { (ma, mb) } else { (mb, ma) };
        *inter_module_weight.entry(key).or_insert(0.0) += w;
    }

    let edges: Vec<CanvasEdge> = inter_module_weight
        .into_iter()
        .filter_map(|((a, b), w)| {
            let _ = module_index.get(&a)?;
            let _ = module_index.get(&b)?;
            Some(CanvasEdge {
                id: format!("edge-{a}-{b}"),
                from_node: format!("module-{a}"),
                to_node: format!("module-{b}"),
                label: Some(format!("{w:.0}")),
            })
        })
        .collect();

    let mut overflow_canvases = Vec::new();
    if !overflow.is_empty() {
        let overflow_path = out_dir.join("weave-modules-overflow.canvas");
        let overflow_columns = columns_for(overflow.len());
        let overflow_nodes = overflow
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let (x, y) = grid_position(i, overflow_columns);
                CanvasNode {
                    id: format!("module-{}", m.id),
                    node_type: "text",
                    x,
                    y,
                    width: NODE_WIDTH,
                    height: NODE_HEIGHT,
                    text: Some(format!("{} ({} files)", m.label, m.files.len())),
                    file: None,
                    label: None,
                }
            })
            .collect();

        let (x, y) = grid_position(visible.len(), columns);
        nodes.push(CanvasNode {
            id: "modules-overflow".to_string(),
            node_type: "file",
            x,
            y,
            width: NODE_WIDTH,
            height: NODE_HEIGHT,
            text: None,
            file: Some("weave-modules-overflow.canvas".to_string()),
            label: Some(format!("+{} more modules", overflow.len())),
        });

        overflow_canvases.push((
            overflow_path,
            Canvas {
                nodes: overflow_nodes,
                edges: Vec::new(),
            },
        ));
    }

    (Canvas { nodes, edges }, overflow_canvases)
}

/// LOD 2: one sub-canvas per module, one node per file in it, edges from
/// the same file-pair weights restricted to pairs inside this module.
fn module_canvas(module: &Module, file_edges: &HashMap<(String, String), f64>) -> Canvas {
    let files_in_module: std::collections::HashSet<&str> =
        module.files.iter().map(String::as_str).collect();
    let columns = columns_for(module.files.len().max(1));
    let node_id = |f: &str| format!("file-{}", f.replace(['/', '.'], "_"));

    let nodes = module
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let (x, y) = grid_position(i, columns);
            CanvasNode {
                id: node_id(f),
                node_type: "text",
                x,
                y,
                width: NODE_WIDTH,
                height: NODE_HEIGHT,
                text: Some(f.clone()),
                file: None,
                label: None,
            }
        })
        .collect();

    let edges = file_edges
        .iter()
        .filter(|((a, b), _)| {
            files_in_module.contains(a.as_str()) && files_in_module.contains(b.as_str())
        })
        .map(|((a, b), w)| CanvasEdge {
            id: format!("edge-{}-{}", node_id(a), node_id(b)),
            from_node: node_id(a),
            to_node: node_id(b),
            label: Some(format!("{w:.0}")),
        })
        .collect();

    Canvas { nodes, edges }
}

fn write_canvas(path: &Path, canvas: &Canvas) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, serde_json::to_string_pretty(canvas)?)?;
    Ok(())
}

fn render_report_md(
    provenance: &Provenance,
    nodes: &[weave_graph_core::Node],
    modules: &[Module],
    doc_provenance_section: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# Weave Graph Report\n\n");
    for line in provenance.badge_lines() {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str(&format!("\nTotal symbols: {}\n\n", nodes.len()));
    out.push_str("## LOD 1 — Architectural Modules\n\n");
    out.push_str("See `weave-modules.canvas` for the linked view.\n\n");
    for module in modules {
        out.push_str(&format!(
            "- **{}** ({} files) — see `weave-module-{}.canvas`\n",
            module.label,
            module.files.len(),
            module.id
        ));
    }
    out.push_str(
        "\n## On-Demand Symbol View\n\nRun `weave export --symbol <name> --depth 2` for LOD 3.\n",
    );
    // `None` leaves the report byte-identical to a default build's —
    // M2.3 renders doc-link provenance only when present.
    if let Some(section) = doc_provenance_section {
        out.push_str(&format!("\n{section}"));
    }
    out
}

#[cfg(test)]
mod tests;
