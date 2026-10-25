//! Deterministic, fixed-window controller for the opt-in mixed-mode LNS.
//!
//! The controller only learns whether a selected neighborhood produced a
//! strict accepted physical-delay improvement. It has no wall-clock input and
//! does not alter the move acceptance policy.

use std::collections::VecDeque;

const RECENT_WINDOW: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Action {
    /// 0 = region neighborhood, 1 = blocker neighborhood.
    pub op: u8,
    pub k: usize,
}

pub(super) struct Controller {
    actions: Vec<Action>,
    ever_tried: Vec<bool>,
    recent: VecDeque<(usize, bool)>,
}

impl Controller {
    pub(super) fn new(kmin: usize, kmax: usize) -> Self {
        assert!(kmin >= 2 && kmin <= kmax);
        let mut actions = Vec::with_capacity((kmax - kmin + 1) * 2);
        for k in kmin..=kmax {
            actions.push(Action { op: 0, k });
            actions.push(Action { op: 1, k });
        }
        Self {
            ever_tried: vec![false; actions.len()],
            actions,
            recent: VecDeque::with_capacity(RECENT_WINDOW),
        }
    }

    /// Choose an arm deterministically. Every arm is sampled once before
    /// recency-window UCB selection begins. Arms missing from the recent
    /// window are explored again, allowing adaptation after the search basin
    /// changes. Index order breaks exact score ties.
    pub(super) fn choose(&mut self) -> (usize, Action) {
        if let Some(i) = self.ever_tried.iter().position(|seen| !seen) {
            self.ever_tried[i] = true;
            return (i, self.actions[i]);
        }

        let mut pulls = vec![0usize; self.actions.len()];
        let mut wins = vec![0usize; self.actions.len()];
        for &(arm, reward) in &self.recent {
            pulls[arm] += 1;
            wins[arm] += if reward { 1 } else { 0 };
        }
        let total = self.recent.len();
        let mut best = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for i in 0..self.actions.len() {
            let score = if pulls[i] == 0 {
                f64::INFINITY
            } else {
                wins[i] as f64 / pulls[i] as f64
                    + (2.0 * ((total + 1) as f64).ln() / pulls[i] as f64).sqrt()
            };
            if score > best_score {
                best = i;
                best_score = score;
            }
        }
        (best, self.actions[best])
    }

    /// Record one completed action. Callers pass true only for a strict
    /// delay decrease returned by the existing accepted whole-tree move path.
    pub(super) fn observe(&mut self, arm: usize, strict_legal_improvement: bool) {
        assert!(arm < self.actions.len());
        if self.recent.len() == RECENT_WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back((arm, strict_legal_improvement));
    }

    #[cfg(test)]
    fn recent_counts(&self, arm: usize) -> (usize, usize) {
        let mut n = 0;
        let mut wins = 0;
        for &(a, reward) in &self.recent {
            if a == arm {
                n += 1;
                wins += if reward { 1 } else { 0 };
            }
        }
        (n, wins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_arm_order_explores_every_operator_size_once() {
        let mut c = Controller::new(2, 4);
        let picked: Vec<_> = (0..6).map(|_| c.choose().1).collect();
        assert_eq!(
            picked,
            vec![
                Action { op: 0, k: 2 },
                Action { op: 1, k: 2 },
                Action { op: 0, k: 3 },
                Action { op: 1, k: 3 },
                Action { op: 0, k: 4 },
                Action { op: 1, k: 4 },
            ]
        );
    }

    #[test]
    fn controller_is_deterministic_for_same_feedback() {
        fn trace() -> Vec<Action> {
            let mut c = Controller::new(2, 3);
            let mut out = Vec::new();
            for t in 0..40 {
                let (arm, action) = c.choose();
                out.push(action);
                // Authored controller-only feedback, not a route-quality test.
                c.observe(arm, (t < 12 && action.op == 0) || (t >= 12 && action.op == 1));
            }
            out
        }
        assert_eq!(trace(), trace());
    }

    #[test]
    fn strict_improvement_feedback_is_binary_and_recency_bounded() {
        let mut c = Controller::new(2, 2);
        let (a0, _) = c.choose();
        let (a1, _) = c.choose();
        c.observe(a0, true);
        c.observe(a1, false); // equal, rejected, uphill and inapplicable all map to false.
        assert_eq!(c.recent_counts(a0), (1, 1));
        assert_eq!(c.recent_counts(a1), (1, 0));
        for i in 0..RECENT_WINDOW {
            c.observe(i % 2, false);
        }
        assert_eq!(c.recent.len(), RECENT_WINDOW);
        assert_eq!(c.recent_counts(a0).0 + c.recent_counts(a1).0, RECENT_WINDOW);
    }

    #[test]
    fn window_forgets_stale_reward_after_basin_change() {
        let mut c = Controller::new(2, 2);
        let (a0, _) = c.choose();
        let (a1, _) = c.choose();
        c.observe(a0, true);
        c.observe(a1, false);
        for i in 0..RECENT_WINDOW {
            c.observe(a1, true);
        }
        assert_eq!(c.recent_counts(a0), (0, 0));
        assert_eq!(c.recent_counts(a1), (RECENT_WINDOW, RECENT_WINDOW));
    }

    #[test]
    fn recent_feedback_can_follow_a_changed_action_regime() {
        let mut c = Controller::new(2, 2);
        let (a0, _) = c.choose();
        let (a1, _) = c.choose();
        c.observe(a0, true);
        c.observe(a1, false);
        for _ in 0..(RECENT_WINDOW * 3) {
            let (arm, action) = c.choose();
            c.observe(arm, action.op == 1);
        }
        assert!(c.recent_counts(a1).0 > RECENT_WINDOW / 2);
    }
}
