//! Deterministic integer maximum-closure solver for binary route-column choices.
//! A bounded branch-and-cut wrapper resolves selected-column incompatibilities.
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub(super) struct ClosureResult {
    pub selected: Vec<bool>,
    pub flow_calls: usize,
    pub complete: bool,
    pub overflow: bool,
}

#[derive(Clone, Copy)]
struct Edge { to: usize, rev: usize, cap: u128 }
struct Dinic { graph: Vec<Vec<Edge>>, level: Vec<i32>, next: Vec<usize> }
impl Dinic {
    fn new(n: usize) -> Self { Self { graph: vec![Vec::new(); n], level: vec![-1; n], next: vec![0; n] } }
    fn add_edge(&mut self, from: usize, to: usize, cap: u128) {
        let fr = self.graph[to].len();
        let tr = self.graph[from].len();
        self.graph[from].push(Edge { to, rev: fr, cap });
        self.graph[to].push(Edge { to: from, rev: tr, cap: 0 });
    }
    fn levels(&mut self, source: usize, sink: usize) -> bool {
        self.level.fill(-1);
        self.level[source] = 0;
        let mut q = VecDeque::from([source]);
        while let Some(v) = q.pop_front() {
            for e in &self.graph[v] {
                if e.cap > 0 && self.level[e.to] < 0 {
                    self.level[e.to] = self.level[v] + 1;
                    q.push_back(e.to);
                }
            }
        }
        self.level[sink] >= 0
    }
    fn send(&mut self, v: usize, sink: usize, pushed: u128) -> Option<u128> {
        if pushed == 0 || v == sink { return Some(pushed); }
        while self.next[v] < self.graph[v].len() {
            let i = self.next[v];
            let to = self.graph[v][i].to;
            let rev = self.graph[v][i].rev;
            let cap = self.graph[v][i].cap;
            if cap > 0 && self.level[to] == self.level[v] + 1 {
                let sent = self.send(to, sink, pushed.min(cap))?;
                if sent > 0 {
                    let reverse = self.graph[to][rev].cap.checked_add(sent)?;
                    self.graph[v][i].cap -= sent;
                    self.graph[to][rev].cap = reverse;
                    return Some(sent);
                }
            }
            self.next[v] += 1;
        }
        Some(0)
    }
    fn max_flow(&mut self, source: usize, sink: usize) -> Option<u128> {
        let mut flow = 0u128;
        while self.levels(source, sink) {
            self.next.fill(0);
            loop {
                let sent = self.send(source, sink, u128::MAX)?;
                if sent == 0 { break; }
                flow = flow.checked_add(sent)?;
            }
        }
        Some(flow)
    }
    fn source_side(&self, source: usize) -> Vec<bool> {
        let mut seen = vec![false; self.graph.len()];
        seen[source] = true;
        let mut q = VecDeque::from([source]);
        while let Some(v) = q.pop_front() {
            for e in &self.graph[v] {
                if e.cap > 0 && !seen[e.to] { seen[e.to] = true; q.push_back(e.to); }
            }
        }
        seen
    }
}

fn closure_cut(
    deltas: &[i128], implications: &[(usize, usize)], excluded: &[bool], inf: u128,
) -> Option<Vec<bool>> {
    let n = deltas.len();
    let source = n;
    let sink = n + 1;
    let mut flow = Dinic::new(n + 2);
    for (i, &delta) in deltas.iter().enumerate() {
        if delta < 0 { flow.add_edge(source, i, (-delta) as u128); }
        else if delta > 0 { flow.add_edge(i, sink, delta as u128); }
        if excluded[i] { flow.add_edge(i, sink, inf); }
    }
    for &(from, to) in implications { flow.add_edge(from, to, inf); }
    flow.max_flow(source, sink)?;
    let side = flow.source_side(source);
    Some(side[..n].to_vec())
}

/// Maximize negative delta subject to implications and pairwise incompatibilities.
/// Incompatibility is resolved exactly by branching x_i=0 or x_j=0. `flow_cap`
/// bounds min-cut calls, not wall time; on cap the best feasible set seen so far
/// is returned and `complete` is false.
pub(super) fn solve_binary_projection(
    deltas: &[i128],
    implications: &[(usize, usize)],
    conflicts: &[(usize, usize)],
    forced_false: &[bool],
    flow_cap: usize,
) -> ClosureResult {
    assert_eq!(forced_false.len(), deltas.len());
    assert!(implications.iter().all(|&(a, b)| a < deltas.len() && b < deltas.len()));
    assert!(conflicts.iter().all(|&(a, b)| a < deltas.len() && b < deltas.len() && a != b));
    let mut sum_abs = 0u128;
    for &d in deltas {
        let Some(abs) = d.checked_abs().map(|v| v as u128) else {
            return ClosureResult { selected: vec![false; deltas.len()], flow_calls: 0, complete: false, overflow: true };
        };
        let Some(next) = sum_abs.checked_add(abs) else {
            return ClosureResult { selected: vec![false; deltas.len()], flow_calls: 0, complete: false, overflow: true };
        };
        sum_abs = next;
    }
    let Some(inf) = sum_abs.checked_add(1) else {
        return ClosureResult { selected: vec![false; deltas.len()], flow_calls: 0, complete: false, overflow: true };
    };
    let mut sorted_implications = implications.to_vec();
    sorted_implications.sort_unstable();
    sorted_implications.dedup();
    let mut sorted_conflicts = conflicts.to_vec();
    for pair in &mut sorted_conflicts { if pair.0 > pair.1 { *pair = (pair.1, pair.0); } }
    sorted_conflicts.sort_unstable();
    sorted_conflicts.dedup();
    let mut best = vec![false; deltas.len()];
    let mut best_benefit = 0i128;
    let mut calls = 0usize;
    let mut complete = true;
    let mut overflow = false;
    fn visit(
        deltas: &[i128], implications: &[(usize, usize)], conflicts: &[(usize, usize)],
        excluded: &mut [bool], inf: u128, cap: usize, calls: &mut usize,
        complete: &mut bool, overflow: &mut bool, best: &mut Vec<bool>, best_benefit: &mut i128,
    ) {
        if *calls >= cap { *complete = false; return; }
        *calls += 1;
        let Some(selected) = closure_cut(deltas, implications, excluded, inf) else {
            *complete = false; *overflow = true; return;
        };
        let mut benefit = 0i128;
        for (i, &yes) in selected.iter().enumerate() {
            if yes { benefit = match benefit.checked_sub(deltas[i]) { Some(v) => v, None => { *complete = false; *overflow = true; return; } }; }
        }
        // The relaxed closure objective is an upper bound for this branch.
        if benefit <= *best_benefit { return; }
        if let Some(&(a, b)) = conflicts.iter().find(|&&(a, b)| selected[a] && selected[b]) {
            excluded[a] = true;
            visit(deltas, implications, conflicts, excluded, inf, cap, calls, complete, overflow, best, best_benefit);
            excluded[a] = false;
            excluded[b] = true;
            visit(deltas, implications, conflicts, excluded, inf, cap, calls, complete, overflow, best, best_benefit);
            excluded[b] = false;
        } else {
            *best_benefit = benefit;
            *best = selected;
        }
    }
    let mut excluded = forced_false.to_vec();
    visit(deltas, &sorted_implications, &sorted_conflicts, &mut excluded, inf, flow_cap, &mut calls, &mut complete, &mut overflow, &mut best, &mut best_benefit);
    ClosureResult { selected: best, flow_calls: calls, complete, overflow }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn brute(d: &[i128], imp: &[(usize, usize)], conf: &[(usize, usize)], forced: &[bool]) -> (i128, Vec<bool>) {
        let mut best = 0i128;
        let mut best_bits = vec![false; d.len()];
        for mask in 0usize..(1usize << d.len()) {
            let bits: Vec<bool> = (0..d.len()).map(|i| mask & (1 << i) != 0).collect();
            if (0..d.len()).any(|i| forced[i] && bits[i]) || imp.iter().any(|&(a,b)| bits[a] && !bits[b]) || conf.iter().any(|&(a,b)| bits[a] && bits[b]) { continue; }
            let benefit = (0..d.len()).filter(|&i| bits[i]).map(|i| -d[i]).sum::<i128>();
            if benefit > best { best = benefit; best_bits = bits; }
        }
        (best, best_bits)
    }
    fn check(d: &[i128], imp: &[(usize,usize)], conf: &[(usize,usize)], forced: &[bool]) {
        let expected = brute(d, imp, conf, forced).0;
        let got = solve_binary_projection(d, imp, conf, forced, 10000);
        assert!(got.complete && !got.overflow);
        let actual = got.selected.iter().enumerate().filter(|(_,yes)| **yes).map(|(i,_)| -d[i]).sum::<i128>();
        assert_eq!(actual, expected);
        assert!(!imp.iter().any(|&(a,b)| got.selected[a] && !got.selected[b]));
        assert!(!conf.iter().any(|&(a,b)| got.selected[a] && got.selected[b]));
        assert!(!(0..d.len()).any(|i| forced[i] && got.selected[i]));
    }
    #[test]
    fn implication_cycle_selects_together_against_bruteforce() { check(&[-5, 2], &[(0,1),(1,0)], &[], &[false,false]); }
    #[test]
    fn weighted_65_cycle_selects_the_entire_closed_set() {
        let n = 65;
        let deltas = vec![-1i128; n];
        let implications = (0..n).map(|i| (i, (i + 1) % n)).collect::<Vec<_>>();
        let result = solve_binary_projection(&deltas, &implications, &[], &vec![false; n], 10);
        assert!(result.complete && !result.overflow);
        assert!(result.selected.iter().all(|&x| x));
        assert_eq!(result.selected.len(), n);
    }
    #[test]
    fn expensive_blocker_can_make_apparently_good_move_unprofitable() { check(&[-10, 12], &[(0,1)], &[], &[false,false]); }
    #[test]
    fn new_new_incompatibility_branches_and_matches_exhaustive_oracle() { check(&[-9,-6,-4], &[(0,2)], &[(0,1),(1,2)], &[false,false,false]); }
    #[test]
    fn forced_zero_and_negative_delta_constraints_match_oracle() { check(&[-8,-7,1], &[(0,1)], &[], &[false,true,false]); }
    #[test]
    fn flow_call_cap_returns_only_a_feasible_incumbent_and_marks_incomplete() {
        let got = solve_binary_projection(&[-9,-8], &[], &[(0,1)], &[false,false], 1);
        assert!(!got.complete);
        assert!(got.flow_calls <= 1);
        assert!(!(got.selected[0] && got.selected[1]));
    }
    #[test]
    fn binary_selector_handles_many_small_deterministic_instances() {
        for n in 1..=7usize {
            let mut state = 0x9e3779b97f4a7c15u64 ^ n as u64;
            for _ in 0..40 {
                let mut d = Vec::new();
                for _ in 0..n { state ^= state << 7; state ^= state >> 9; d.push((state % 13) as i128 - 6); }
                let mut imp = Vec::new(); let mut conf = Vec::new(); let mut forced = vec![false; n];
                for i in 0..n { for j in i+1..n { state ^= state << 7; state ^= state >> 9; if state % 11 == 0 { imp.push((i,j)); } if state % 13 == 0 { conf.push((i,j)); } } if state % 17 == 0 { forced[i] = true; } }
                check(&d, &imp, &conf, &forced);
            }
        }
    }
}
