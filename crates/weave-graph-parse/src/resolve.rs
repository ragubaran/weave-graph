use std::collections::HashMap;

use crate::model::ParsedFile;

/// A call or structural reference resolved to a concrete target symbol.
/// `kind` is one of the schema's free-form edge kinds (`plan.md` §1.2):
/// `CALLS_EXACT`, `CALLS_DYNAMIC`, `IMPORTS`, `INHERITS`, `IMPLEMENTS`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEdge {
    pub source_moniker: String,
    pub target_moniker: String,
    pub kind: String,
}

fn file_of(moniker: &str) -> &str {
    moniker.split('#').next().unwrap_or(moniker)
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
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

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
    ) -> Vec<(String, &'static str)> {
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
                )];
            }
            if candidates.len() == 1 {
                return vec![(
                    self.interner.resolve(candidates[0]).to_string(),
                    "CALLS_EXACT",
                )];
            }
        }
        candidates
            .iter()
            .map(|&m_id| (self.interner.resolve(m_id).to_string(), "CALLS_DYNAMIC"))
            .collect()
    }

    pub fn resolve(&self, file: &ParsedFile) -> (Vec<ResolvedEdge>, Vec<String>) {
        let mut edges = Vec::new();
        let mut unresolved = Vec::new();

        for call in &file.calls {
            let resolved =
                self.resolve_one(&call.caller_moniker, &call.callee_name, call.is_member_call);
            if resolved.is_empty() {
                unresolved.push(call.callee_name.clone());
            }
            for (target, kind) in resolved {
                edges.push(ResolvedEdge {
                    source_moniker: call.caller_moniker.clone(),
                    target_moniker: target,
                    kind: kind.to_string(),
                });
            }
        }

        for structural in &file.structural_edges {
            let candidates = self.candidates(&structural.target_name);
            if candidates.is_empty() {
                unresolved.push(structural.target_name.clone());
            }
            for &candidate_id in candidates {
                edges.push(ResolvedEdge {
                    source_moniker: structural.source_moniker.clone(),
                    target_moniker: self.interner.resolve(candidate_id).to_string(),
                    kind: structural.kind.as_str().to_string(),
                });
            }
        }

        // Deduplicate unresolved references.
        unresolved.sort();
        unresolved.dedup();
        (edges, unresolved)
    }

    /// Only the references `self` alone can't resolve (an empty candidate
    /// set here) — retried against `fallback`'s index before being given
    /// up on. `resolve`'s own single-repo behavior is untouched by this;
    /// this exists for the `federation` feature (`impl.md` M2.1) to let
    /// one repo's otherwise-unresolved reference match a symbol actually
    /// exported by a *different*, independently-indexed repo. Same
    /// precision ceiling as `resolve` itself: by short name, not
    /// type-aware — a same-named-but-unrelated symbol in `fallback` can
    /// still match, exactly as within one repo today.
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
            for (target, kind) in
                fallback.resolve_one(&call.caller_moniker, &call.callee_name, call.is_member_call)
            {
                edges.push(ResolvedEdge {
                    source_moniker: call.caller_moniker.clone(),
                    target_moniker: target,
                    kind: kind.to_string(),
                });
            }
        }

        for structural in &file.structural_edges {
            if !self.candidates(&structural.target_name).is_empty() {
                continue;
            }
            for &candidate_id in fallback.candidates(&structural.target_name) {
                edges.push(ResolvedEdge {
                    source_moniker: structural.source_moniker.clone(),
                    target_moniker: fallback.interner.resolve(candidate_id).to_string(),
                    kind: structural.kind.as_str().to_string(),
                });
            }
        }

        edges
    }
}

#[cfg(test)]
mod tests;
