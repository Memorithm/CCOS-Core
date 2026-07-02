//! Causal Flash — a bounded, deterministic causal-cone context filter.
//!
//! At 10k+ nodes, recomputing a *global* structural signal (eigenvector
//! centrality, a full PageRank sweep) every tick is wasteful when the agent is
//! only working on a handful of nodes. Causal Flash instead computes relevance
//! on a **local cone** rooted at the active frontier — the graph analogue of
//! restricting attention to a window, but with an exact, non-probabilistic
//! definition rather than an approximation of the global measure.
//!
//! ## What it is (and the honest boundary)
//!
//! This is **not** flash-attention's exact tiling of a global op — a fixed
//! depth-`n` neighbourhood is a *truncation* of global centrality, so we do not
//! pretend to reproduce it. Instead the relevance we compute is **exactly** the
//! decayed dependency mass reachable *within the cone* — a well-defined
//! projection of the graph onto the cone, with no randomness and no reliance on
//! the rest of the graph. Completeness is reported honestly: the returned window
//! carries a [`CausalWindow::complete`] flag that is `true` **iff** the
//! dependency closure closed before the horizon (nothing was cut).
//!
//! ## Edge convention
//!
//! CCOS edges read `A → B` = *A depends on B* (see [`crate::memory`]). So a
//! node's **out-edges are its dependencies** (what it needs — required for the
//! LLM to *understand* it) and its **in-edges are its callers / dependents**
//! (what breaks if it changes — required for *impact*). Causal Flash follows
//! out-edges to build the dependency cone and adds a one-hop in-edge ring for
//! impact.
//!
//! ## Determinism (`replay == live`)
//!
//! Every step uses a total order on [`NodeId`]: seeds, frontier expansion, and
//! the emitted summary are all sorted, so the output never depends on
//! `HashMap` iteration order. The function is a pure read-only fold over the
//! resident graph — nothing is stored, so snapshots are untouched and a replay
//! reproduces the same window byte-for-byte.

use std::collections::{BTreeMap, BTreeSet};

use crate::memory::{EdgeType, MemoryGraph, NodeId, NodeState};

/// Knobs for [`MemoryGraph::causal_flash_window`]. All read-only; nothing here
/// is persisted.
#[derive(Debug, Clone)]
pub struct CausalFlashConfig {
    /// Maximum dependency depth `n` from the seed frontier.
    pub horizon: usize,
    /// Also seed from low-trust nodes (a poisoned/suspect node and its cone are
    /// often exactly what the agent must review), not just `Working`.
    pub include_low_trust_seeds: bool,
    /// Seed a node when `include_low_trust_seeds` and `trust < trust_threshold`.
    pub trust_threshold: f64,
    /// Per-hop distance decay in `(0, 1]`; a dependency `k` hops away
    /// contributes `decay^k · (edge weights)` to relevance.
    pub decay: f64,
    /// Add the one-hop in-edge ring (callers/dependents) for impact context.
    pub include_callers: bool,
    /// Token budget: cap the summary at this many nodes. Truncation removes
    /// **callers only** (impact context), lowest-relevance first — the
    /// dependency closure is never dropped, so structural correctness is
    /// preserved. `None` = unbounded.
    pub max_nodes: Option<usize>,
}

impl Default for CausalFlashConfig {
    fn default() -> Self {
        Self {
            horizon: 3,
            include_low_trust_seeds: false,
            trust_threshold: 0.5,
            decay: 0.5,
            include_callers: true,
            max_nodes: None,
        }
    }
}

/// Why a node is in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalRole {
    /// A member of the active frontier (the query root).
    Seed,
    /// Reached by following dependency (out-) edges from the seeds.
    Dependency,
    /// A one-hop caller/dependent (in-edge) of a cone node — impact context.
    Caller,
}

/// One entry of the summary.
#[derive(Debug, Clone)]
pub struct CausalWindowNode {
    pub id: NodeId,
    pub state: NodeState,
    pub trust: f64,
    pub role: CausalRole,
    /// Dependency distance from the nearest seed (`0` for seeds and for callers
    /// attached to a seed).
    pub depth: usize,
    /// Decayed dependency mass reachable within the cone — the locally-exact
    /// relevance.
    pub relevance: f64,
}

/// The high-density causal summary: a dependency-distance-layered node list
/// plus honest completeness metadata.
#[derive(Debug, Clone)]
pub struct CausalWindow {
    /// Nodes ordered for reading: seeds first, then dependencies by increasing
    /// distance and decreasing relevance, then callers — a topological layering
    /// rooted at the working set. `NodeId` breaks every tie.
    pub nodes: Vec<CausalWindowNode>,
    /// `true` iff the dependency closure closed before the horizon — i.e. no
    /// dependency was cut. Callers dropped for the token budget do NOT clear
    /// this (they are impact context, not correctness).
    pub complete: bool,
    /// How many nodes were cut: dependencies beyond the horizon plus any callers
    /// dropped for `max_nodes`.
    pub omitted: usize,
    /// Number of seed nodes the window was rooted at.
    pub seed_count: usize,
}

/// Edge types that constitute a *directional dependency* for the cone. Matches
/// the "downstream dependency" set walked by failure/`do()` propagation; the
/// symmetric `RelatedTo` and the belief edges (`Supports`/`Contradicts`) carry
/// different semantics and are excluded.
fn is_dependency_edge(t: &EdgeType) -> bool {
    matches!(
        t,
        EdgeType::DependsOn
            | EdgeType::Contains
            | EdgeType::References
            | EdgeType::Causes
            | EdgeType::Calls
            | EdgeType::DataFlow
    )
}

impl MemoryGraph {
    /// Compute the [`CausalWindow`] for the current active frontier under `cfg`.
    ///
    /// Cost is `O(E)` to index the resident edges once plus `O(cone)` to
    /// traverse — never the `O(iterations · E)` of a global eigenvector sweep,
    /// which is the whole point at scale.
    pub fn causal_flash_window(&self, cfg: &CausalFlashConfig) -> CausalWindow {
        let decay = cfg.decay.clamp(f64::MIN_POSITIVE, 1.0);

        // Forward (dependency) and reverse (caller) adjacency, restricted to
        // dependency edge types. Sorted for deterministic expansion.
        let mut fwd: BTreeMap<&NodeId, Vec<(&NodeId, f64)>> = BTreeMap::new();
        let mut rev: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
        for e in self.edges() {
            if !is_dependency_edge(&e.edge_type) {
                continue;
            }
            fwd.entry(&e.source)
                .or_default()
                .push((&e.target, e.weight));
            rev.entry(&e.target).or_default().push(&e.source);
        }
        for v in fwd.values_mut() {
            v.sort_by(|a, b| a.0.cmp(b.0));
        }
        for v in rev.values_mut() {
            v.sort();
        }

        // Seeds: Working nodes (∪ low-trust when enabled), never Orphan.
        // BTreeSet keyed by id ⇒ deterministic seed order.
        let mut seeds: BTreeSet<&NodeId> = BTreeSet::new();
        for (id, node) in self.node_entries() {
            if node.state == NodeState::Orphan {
                continue;
            }
            let is_working = node.state == NodeState::Working;
            let is_low_trust = cfg.include_low_trust_seeds && node.trust < cfg.trust_threshold;
            if is_working || is_low_trust {
                seeds.insert(id);
            }
        }
        let seed_count = seeds.len();

        // depth + accumulated relevance per cone node; role tracked separately.
        let mut depth: BTreeMap<&NodeId, usize> = BTreeMap::new();
        let mut relevance: BTreeMap<&NodeId, f64> = BTreeMap::new();
        for s in &seeds {
            depth.insert(s, 0);
            relevance.insert(s, 1.0);
        }

        // Layered BFS along dependency edges, bounded by the horizon.
        let mut frontier: Vec<&NodeId> = seeds.iter().copied().collect();
        let mut cut_deps: BTreeSet<&NodeId> = BTreeSet::new();
        for d in 1..=cfg.horizon {
            let mut next: BTreeSet<&NodeId> = BTreeSet::new();
            for &parent in &frontier {
                let prel = relevance[parent];
                if let Some(children) = fwd.get(parent) {
                    for &(child, w) in children {
                        // Orphan (dead/unreachable) nodes are excluded from the
                        // structural cone, exactly as they are from centrality.
                        if self.node(child).map(|x| x.state) == Some(NodeState::Orphan) {
                            continue;
                        }
                        let contrib = prel * decay * w;
                        *relevance.entry(child).or_insert(0.0) += contrib;
                        if !depth.contains_key(child) {
                            depth.insert(child, d);
                            next.insert(child);
                        }
                    }
                }
            }
            frontier = next.into_iter().collect();
            if frontier.is_empty() {
                break; // closure reached a fixpoint before the horizon
            }
        }
        // Anything still on the frontier after the last layer is a cut dependency.
        if cfg.horizon > 0 {
            for &parent in &frontier {
                if let Some(children) = fwd.get(parent) {
                    for &(child, _) in children {
                        // An excluded Orphan is not an omitted dependency.
                        if self.node(child).map(|x| x.state) == Some(NodeState::Orphan) {
                            continue;
                        }
                        if !depth.contains_key(child) {
                            cut_deps.insert(child);
                        }
                    }
                }
            }
        }
        let complete = cut_deps.is_empty();

        // Assemble cone nodes (seed / dependency), skipping any id not resident.
        let mut out: Vec<CausalWindowNode> = Vec::new();
        for (&id, &d) in &depth {
            let Some(node) = self.node(id) else {
                continue;
            };
            let role = if seeds.contains(&id) {
                CausalRole::Seed
            } else {
                CausalRole::Dependency
            };
            out.push(CausalWindowNode {
                id: id.clone(),
                state: node.state,
                trust: node.trust,
                role,
                depth: d,
                relevance: relevance.get(id).copied().unwrap_or(0.0),
            });
        }

        // One-hop caller ring (impact), for cone nodes with in-edges. Deterministic
        // via the sorted reverse adjacency and a BTreeSet of already-included ids.
        let cone_ids: BTreeSet<&NodeId> = depth.keys().copied().collect();
        let mut caller_added: BTreeSet<NodeId> = BTreeSet::new();
        if cfg.include_callers {
            for &cone in &cone_ids {
                if let Some(callers) = rev.get(cone) {
                    let base = relevance.get(cone).copied().unwrap_or(0.0);
                    for &caller in callers {
                        if cone_ids.contains(&caller) || caller_added.contains(caller) {
                            continue;
                        }
                        let Some(node) = self.node(caller) else {
                            continue;
                        };
                        if node.state == NodeState::Orphan {
                            continue;
                        }
                        caller_added.insert(caller.clone());
                        out.push(CausalWindowNode {
                            id: caller.clone(),
                            state: node.state,
                            trust: node.trust,
                            role: CausalRole::Caller,
                            depth: depth[cone],
                            relevance: base * decay,
                        });
                    }
                }
            }
        }

        // Reading order: (role rank, depth asc, relevance desc, id asc) — seeds
        // first, then nearest/most-relevant dependencies, then callers.
        fn role_rank(r: CausalRole) -> u8 {
            match r {
                CausalRole::Seed => 0,
                CausalRole::Dependency => 1,
                CausalRole::Caller => 2,
            }
        }
        out.sort_by(|a, b| {
            role_rank(a.role)
                .cmp(&role_rank(b.role))
                .then(a.depth.cmp(&b.depth))
                .then(b.relevance.total_cmp(&a.relevance))
                .then(a.id.cmp(&b.id))
        });

        let mut omitted = cut_deps.len();

        // Token budget: drop callers (lowest relevance first) until within
        // `max_nodes`. Dependencies are never dropped.
        if let Some(cap) = cfg.max_nodes {
            if out.len() > cap {
                // Indices of callers, ordered by ascending relevance then id, are
                // the drop candidates.
                let mut callers: Vec<usize> = out
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.role == CausalRole::Caller)
                    .map(|(i, _)| i)
                    .collect();
                callers.sort_by(|&i, &j| {
                    out[i]
                        .relevance
                        .total_cmp(&out[j].relevance)
                        .then(out[i].id.cmp(&out[j].id))
                });
                let need_to_drop = out.len() - cap;
                let drop: BTreeSet<usize> = callers.into_iter().take(need_to_drop).collect();
                if !drop.is_empty() {
                    omitted += drop.len();
                    let mut kept = Vec::with_capacity(out.len() - drop.len());
                    for (i, n) in out.into_iter().enumerate() {
                        if !drop.contains(&i) {
                            kept.push(n);
                        }
                    }
                    out = kept;
                }
            }
        }

        CausalWindow {
            nodes: out,
            complete,
            omitted,
            seed_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::NodeType;

    fn n(g: &mut MemoryGraph, id: &str) {
        g.upsert_node(id.into(), id.into(), "x".into(), NodeType::Module);
    }
    fn dep(g: &mut MemoryGraph, a: &str, b: &str) {
        // a depends on b (a → b).
        g.add_edge(a.into(), b.into(), 1.0, EdgeType::DependsOn);
    }

    // Seed = Working; the dependency cone follows out-edges; Orphan excluded.
    #[test]
    fn cone_follows_dependencies_and_excludes_orphans() {
        let mut g = MemoryGraph::new(0.2, usize::MAX);
        for id in ["w", "dep1", "dep2", "orphan", "unrelated"] {
            n(&mut g, id);
        }
        dep(&mut g, "w", "dep1");
        dep(&mut g, "dep1", "dep2");
        dep(&mut g, "w", "orphan");
        g.set_node_state(&"w".into(), NodeState::Working);
        g.set_node_state(&"orphan".into(), NodeState::Orphan);

        let win = g.causal_flash_window(&CausalFlashConfig::default());
        let ids: Vec<&str> = win.nodes.iter().map(|x| x.id.0.as_str()).collect();
        assert!(ids.contains(&"w"), "seed present");
        assert!(
            ids.contains(&"dep1") && ids.contains(&"dep2"),
            "deps reached"
        );
        assert!(
            !ids.contains(&"orphan"),
            "orphan excluded even as a dependency"
        );
        assert!(!ids.contains(&"unrelated"), "disconnected node excluded");
        assert_eq!(win.seed_count, 1);
        assert!(win.complete, "closure fits inside default horizon 3");
    }

    // Horizon truncation is reported: a dependency one hop past `n` is cut and
    // `complete` goes false with a matching `omitted`.
    #[test]
    fn horizon_bounds_the_cone_and_reports_incompleteness() {
        let mut g = MemoryGraph::new(0.2, usize::MAX);
        for id in ["w", "a", "b", "c"] {
            n(&mut g, id);
        }
        dep(&mut g, "w", "a");
        dep(&mut g, "a", "b");
        dep(&mut g, "b", "c");
        g.set_node_state(&"w".into(), NodeState::Working);

        let cfg = CausalFlashConfig {
            horizon: 2,
            include_callers: false,
            ..Default::default()
        };
        let win = g.causal_flash_window(&cfg);
        let ids: Vec<&str> = win.nodes.iter().map(|x| x.id.0.as_str()).collect();
        assert!(ids.contains(&"a") && ids.contains(&"b"), "within horizon");
        assert!(!ids.contains(&"c"), "c is at depth 3 > horizon 2");
        assert!(!win.complete, "closure was cut at the horizon");
        assert_eq!(win.omitted, 1, "exactly c was omitted");
    }

    // Relevance decays with dependency distance.
    #[test]
    fn relevance_decays_with_depth() {
        let mut g = MemoryGraph::new(0.2, usize::MAX);
        for id in ["w", "a", "b"] {
            n(&mut g, id);
        }
        dep(&mut g, "w", "a");
        dep(&mut g, "a", "b");
        g.set_node_state(&"w".into(), NodeState::Working);
        let win = g.causal_flash_window(&CausalFlashConfig {
            include_callers: false,
            ..Default::default()
        });
        let rel = |id: &str| win.nodes.iter().find(|x| x.id.0 == id).unwrap().relevance;
        assert!(rel("w") > rel("a") && rel("a") > rel("b"), "monotone decay");
    }

    // In-edges surface as Caller-role impact context.
    #[test]
    fn callers_appear_as_impact_context() {
        let mut g = MemoryGraph::new(0.2, usize::MAX);
        for id in ["caller", "w", "dep"] {
            n(&mut g, id);
        }
        dep(&mut g, "caller", "w"); // caller depends on w  ⇒ w's in-edge
        dep(&mut g, "w", "dep");
        g.set_node_state(&"w".into(), NodeState::Working);

        let win = g.causal_flash_window(&CausalFlashConfig::default());
        let caller = win.nodes.iter().find(|x| x.id.0 == "caller");
        assert!(caller.is_some(), "caller included");
        assert_eq!(caller.unwrap().role, CausalRole::Caller);
    }

    // Deterministic and insertion-order independent: two graphs built in
    // different orders yield byte-identical windows.
    #[test]
    fn window_is_deterministic_regardless_of_insertion_order() {
        let build = |order: &[&str]| {
            let mut g = MemoryGraph::new(0.2, usize::MAX);
            for id in order {
                n(&mut g, id);
            }
            dep(&mut g, "w", "a");
            dep(&mut g, "w", "b");
            dep(&mut g, "a", "c");
            g.set_node_state(&"w".into(), NodeState::Working);
            g.causal_flash_window(&CausalFlashConfig::default())
        };
        let w1 = build(&["w", "a", "b", "c"]);
        let w2 = build(&["c", "b", "a", "w"]);
        let ids1: Vec<(&str, usize)> = w1
            .nodes
            .iter()
            .map(|x| (x.id.0.as_str(), x.depth))
            .collect();
        let ids2: Vec<(&str, usize)> = w2
            .nodes
            .iter()
            .map(|x| (x.id.0.as_str(), x.depth))
            .collect();
        assert_eq!(ids1, ids2, "order-independent, deterministic emission");
    }

    // The token budget drops callers only — the dependency closure is preserved.
    #[test]
    fn budget_truncates_callers_not_dependencies() {
        let mut g = MemoryGraph::new(0.2, usize::MAX);
        for id in ["w", "d1", "d2", "c1", "c2", "c3"] {
            n(&mut g, id);
        }
        dep(&mut g, "w", "d1");
        dep(&mut g, "d1", "d2");
        // three callers of w
        dep(&mut g, "c1", "w");
        dep(&mut g, "c2", "w");
        dep(&mut g, "c3", "w");
        g.set_node_state(&"w".into(), NodeState::Working);

        let cfg = CausalFlashConfig {
            max_nodes: Some(3),
            ..Default::default()
        };
        let win = g.causal_flash_window(&cfg);
        assert!(win.nodes.len() <= 3, "within budget");
        // The full dependency closure (w, d1, d2) must survive.
        for keep in ["w", "d1", "d2"] {
            assert!(
                win.nodes.iter().any(|x| x.id.0 == keep),
                "dependency {keep} must not be dropped"
            );
        }
        assert!(win.omitted >= 1, "some callers were dropped for the budget");
        assert!(
            win.nodes.iter().all(|x| x.role != CausalRole::Caller) || win.nodes.len() == 3,
            "callers are the only drop candidates"
        );
    }

    // Low-trust seeding brings a suspect node (and its cone) into view even when
    // it is not Working.
    #[test]
    fn low_trust_seeding_is_opt_in() {
        let mut g = MemoryGraph::new(0.2, usize::MAX);
        for id in ["poison", "used"] {
            n(&mut g, id);
        }
        dep(&mut g, "poison", "used");
        if let Some(node) = g.node_mut(&"poison".into()) {
            node.trust = 0.1;
        }

        let off = g.causal_flash_window(&CausalFlashConfig::default());
        assert_eq!(
            off.seed_count, 0,
            "no Working, no low-trust seeding ⇒ empty"
        );

        let on = g.causal_flash_window(&CausalFlashConfig {
            include_low_trust_seeds: true,
            trust_threshold: 0.5,
            ..Default::default()
        });
        assert!(on.nodes.iter().any(|x| x.id.0 == "poison"), "poison seeded");
        assert!(
            on.nodes.iter().any(|x| x.id.0 == "used"),
            "its cone included"
        );
    }
}
