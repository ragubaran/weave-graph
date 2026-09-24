use std::collections::HashMap;
use std::path::Path;

use crate::Language;
use crate::model::ParsedFile;

/// How an edge's endpoints were resolved (P10.5) — the confidence
/// dimension `weave_graph_core::edge_confidence` classifies. Free-form
/// `&'static str` at the edge-kind layer already existed for `kind`
/// itself; this is the analogous "why" behind *that* classification,
/// not a second copy of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    /// A single candidate resolved within the caller's own file — the
    /// tightest, least ambiguous match this resolver can produce.
    SameFileExact,
    /// A single candidate anywhere in the whole indexed project.
    UniqueGlobalExact,
    /// Multiple same-named candidates existed; every one became its own
    /// edge (`CALLS_DYNAMIC`'s fan-out, or an unresolved structural
    /// reference with more than one same-named target) — an inherent
    /// ambiguity in the source, not a resolver shortcoming.
    AmbiguousHeuristic,
}

impl ResolutionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SameFileExact => "SAME_FILE_EXACT",
            Self::UniqueGlobalExact => "UNIQUE_GLOBAL_EXACT",
            Self::AmbiguousHeuristic => weave_graph_core::AMBIGUOUS_HEURISTIC,
        }
    }
}

/// A call or structural reference resolved to a concrete target symbol.
/// `kind` is one of the schema's free-form edge kinds:
/// `CALLS_EXACT`, `CALLS_DYNAMIC`, `IMPORTS`, `INHERITS`, `IMPLEMENTS`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEdge {
    pub source_moniker: String,
    pub target_moniker: String,
    pub kind: String,
    pub resolution_kind: ResolutionKind,
    pub extractor: Option<String>,
}

/// ID-based edge result used by storage ingestion to avoid cloning monikers.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEdgeIds {
    pub source_id: u32,
    pub target_id: u32,
    pub kind: String,
    pub resolution_kind: ResolutionKind,
    pub extractor: Option<String>,
}

fn file_of(moniker: &str) -> &str {
    moniker.split('#').next().unwrap_or(moniker)
}

/// Which language backend produced an edge, derived from the source
/// symbol's own file — the same `Language::from_path` dispatch the
/// extractors themselves already run on, not a second guess. `None`
/// when the path has no recognized extension (matches
/// `weave-graph-cli::verify`'s own "unresolvable language" handling).
fn extractor_of(moniker: &str) -> Option<String> {
    Language::from_path(Path::new(file_of(moniker)))
        .map(|lang| format!("{lang:?}").to_ascii_lowercase())
}

use std::rc::Rc;

/// String interner for mapping monikers and symbol names to 32-bit integer IDs.
/// Invariant 4: Peak memory must stay under 80MB for 500k symbols;
/// integer IDs cut candidate vector memory by over 60% compared to String.
#[derive(Debug, Default, Clone)]
pub struct StringInterner {
    strings: Vec<Rc<str>>,
    indices: HashMap<Rc<str>, u32>,
}

impl StringInterner {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.indices.get(s) {
            id
        } else {
            let id = self.strings.len() as u32;
            let rc: Rc<str> = s.into();
            self.strings.push(rc.clone());
            self.indices.insert(rc, id);
            id
        }
    }

    pub fn get_id(&self, s: &str) -> Option<u32> {
        self.indices.get(s).copied()
    }

    pub fn resolve(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }
}

/// Project-wide symbol table used to turn raw per-file references into edges.
/// Uses uint32 string interning internally to stay within the 80MB envelope.
#[derive(Debug, Default)]
pub struct ProjectIndex {
    interner: StringInterner,
    by_short_name: HashMap<u32, Vec<u32>>,
}

impl ProjectIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, file: &ParsedFile) {
        for symbol in &file.symbols {
            let short_name = symbol.symbol.rsplit("::").next().unwrap_or(&symbol.symbol);
            self.add_symbol(&symbol.moniker, short_name);
        }
    }

    pub fn add_symbol(&mut self, moniker: &str, short_name: &str) -> u32 {
        let name_id = self.interner.intern(short_name);
        let moniker_id = self.interner.intern(moniker);
        self.by_short_name
            .entry(name_id)
            .or_default()
            .push(moniker_id);
        moniker_id
    }

    pub fn get_moniker_id(&self, moniker: &str) -> Option<u32> {
        self.interner.get_id(moniker)
    }

    fn candidates(&self, short_name: &str) -> &[u32] {
        self.interner
            .get_id(short_name)
            .and_then(|id| self.by_short_name.get(&id).map(Vec::as_slice))
            .unwrap_or(&[])
    }

    fn resolve_one(
        &self,
        from_moniker: &str,
        short_name: &str,
        force_dynamic: bool,
    ) -> Vec<(String, &'static str, ResolutionKind)> {
        let candidates = self.candidates(short_name);
        if candidates.is_empty() {
            return Vec::new();
        }
        if !force_dynamic {
            let caller_file = file_of(from_moniker);
            let same_file: Vec<u32> = candidates
                .iter()
                .copied()
                .filter(|&m_id| file_of(self.interner.resolve(m_id)) == caller_file)
                .collect();
            if same_file.len() == 1 {
                return vec![(
                    self.interner.resolve(same_file[0]).to_string(),
                    "CALLS_EXACT",
                    ResolutionKind::SameFileExact,
                )];
            }
            if candidates.len() == 1 {
                return vec![(
                    self.interner.resolve(candidates[0]).to_string(),
                    "CALLS_EXACT",
                    ResolutionKind::UniqueGlobalExact,
                )];
            }
        }
        candidates
            .iter()
            .map(|&m_id| {
                (
                    self.interner.resolve(m_id).to_string(),
                    "CALLS_DYNAMIC",
                    ResolutionKind::AmbiguousHeuristic,
                )
            })
            .collect()
    }

    pub fn resolve(&self, file: &ParsedFile) -> (Vec<ResolvedEdge>, Vec<String>) {
        let (ids, unresolved) = self.resolve_ids(file);
        let edges = ids
            .into_iter()
            .map(|edge| ResolvedEdge {
                source_moniker: self.interner.resolve(edge.source_id).to_string(),
                target_moniker: self.interner.resolve(edge.target_id).to_string(),
                kind: edge.kind,
                resolution_kind: edge.resolution_kind,
                extractor: edge.extractor,
            })
            .collect();
        (edges, unresolved)
    }

    /// Resolve references while retaining interned IDs for memory-bounded ingestion.
    pub fn resolve_ids(&self, file: &ParsedFile) -> (Vec<ResolvedEdgeIds>, Vec<String>) {
        let mut edges = Vec::new();
        let mut unresolved = Vec::new();

        for call in &file.calls {
            let resolved =
                self.resolve_one(&call.caller_moniker, &call.callee_name, call.is_member_call);
            if resolved.is_empty() {
                unresolved.push(call.callee_name.clone());
            }
            let source_id = self.interner.get_id(&call.caller_moniker);
            let extractor = extractor_of(&call.caller_moniker);
            for (target, kind, resolution_kind) in resolved {
                let Some(source_id) = source_id else { continue };
                let Some(target_id) = self.interner.get_id(&target) else {
                    continue;
                };
                edges.push(ResolvedEdgeIds {
                    source_id,
                    target_id,
                    kind: kind.to_string(),
                    resolution_kind,
                    extractor: extractor.clone(),
                });
            }
        }

        for structural in &file.structural_edges {
            let candidates = self.candidates(&structural.target_name);
            if candidates.is_empty() {
                unresolved.push(structural.target_name.clone());
            }
            // Every one of several same-named candidates is an inherent
            // ambiguity (the reference alone can't disambiguate further),
            // same as `CALLS_DYNAMIC`'s own fan-out — never presented as
            // certain just because a structural edge kind sounds definite.
            let resolution_kind = if candidates.len() == 1 {
                ResolutionKind::UniqueGlobalExact
            } else {
                ResolutionKind::AmbiguousHeuristic
            };
            let extractor = extractor_of(&structural.source_moniker);
            for &candidate_id in candidates {
                let Some(source_id) = self.interner.get_id(&structural.source_moniker) else {
                    continue;
                };
                edges.push(ResolvedEdgeIds {
                    source_id,
                    target_id: candidate_id,
                    kind: structural.kind.as_str().to_string(),
                    resolution_kind,
                    extractor: extractor.clone(),
                });
            }
        }

        // Deduplicate unresolved references.
        unresolved.sort();
        unresolved.dedup();
        (edges, unresolved)
    }

    /// Only the references `self` alone can't resolve — retried against
    /// `fallback`'s index so the `federation` feature can match a
    /// reference in one repo to a symbol exported by a different,
    /// independently-indexed repo. Same by-short-name precision ceiling
    /// as `resolve`: a same-named-but-unrelated symbol can still match.
    pub fn resolve_cross_repo(
        &self,
        file: &ParsedFile,
        fallback: &ProjectIndex,
    ) -> Vec<ResolvedEdge> {
        let mut edges = Vec::new();

        for call in &file.calls {
            if !self.candidates(&call.callee_name).is_empty() {
                continue;
            }
            let extractor = extractor_of(&call.caller_moniker);
            for (target, kind, _) in
                fallback.resolve_one(&call.caller_moniker, &call.callee_name, call.is_member_call)
            {
                edges.push(ResolvedEdge {
                    source_moniker: call.caller_moniker.clone(),
                    target_moniker: target,
                    kind: kind.to_string(),
                    // Cross-repo matching is by-short-name only across an
                    // independently-indexed project — even a single
                    // candidate carries more uncertainty than a same-repo
                    // unique match (this function's own doc comment: "a
                    // same-named-but-unrelated symbol can still match").
                    resolution_kind: ResolutionKind::AmbiguousHeuristic,
                    extractor: extractor.clone(),
                });
            }
        }

        for structural in &file.structural_edges {
            if !self.candidates(&structural.target_name).is_empty() {
                continue;
            }
            let extractor = extractor_of(&structural.source_moniker);
            for &candidate_id in fallback.candidates(&structural.target_name) {
                edges.push(ResolvedEdge {
                    source_moniker: structural.source_moniker.clone(),
                    target_moniker: fallback.interner.resolve(candidate_id).to_string(),
                    kind: structural.kind.as_str().to_string(),
                    resolution_kind: ResolutionKind::AmbiguousHeuristic,
                    extractor: extractor.clone(),
                });
            }
        }

        edges
    }
}

#[cfg(test)]
mod tests;
