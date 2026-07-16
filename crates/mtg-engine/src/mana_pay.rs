//! Auto-tap mana payment: find a set of taps (plus floating pool mana and
//! Phyrexian life) that pays a cost. Most-constrained-first with bounded
//! backtracking; source counts are small so this is cheap.
//!
//! The plan records, per tap, exactly what enters the pool, so a dual land
//! assigned to its second color contributes that color and not its first.

use mtg_ir::{Color, ColorSet, HybridPip, ManaCost, ManaProduction};
use smallvec::SmallVec;

use crate::state::{GameState, ObjectId, Seat};

const W: u8 = 1 << 0;
const U: u8 = 1 << 1;
const B: u8 = 1 << 2;
const R: u8 = 1 << 3;
const G: u8 = 1 << 4;
const C: u8 = 1 << 5;

fn color_bit(c: Color) -> u8 {
    match c {
        Color::W => W,
        Color::U => U,
        Color::B => B,
        Color::R => R,
        Color::G => G,
    }
}

fn bit_to_index(bit: u8) -> usize {
    bit.trailing_zeros() as usize
}

#[derive(Debug, Clone, Default)]
pub struct TapPlan {
    /// (source, mana ability index, exact pool addition W U B R G C).
    pub taps: SmallVec<[(ObjectId, u8, [u8; 6]); 8]>,
    /// W U B R G C consumed from the already-floating pool.
    pub use_pool: [u16; 6],
    pub pay_life: u8,
}

/// One unit of potential mana. Units from the same (object, ability) share a
/// group: tapping once yields all of them.
#[derive(Debug, Clone, Copy)]
struct Unit {
    group: u16,
    options: u8,
}

#[derive(Debug, Clone, Copy)]
struct Group {
    obj: ObjectId,
    ability: u8,
    /// Fixed productions always add everything they make; choice
    /// productions add only the solver-assigned unit.
    fixed: bool,
}

fn production_units(p: &ManaProduction, units: &mut Vec<Unit>, group: u16) {
    match p {
        ManaProduction::Fixed { w, u, b, r, g, c } => {
            for (n, bit) in [(*w, W), (*u, U), (*b, B), (*r, R), (*g, G), (*c, C)] {
                for _ in 0..n {
                    units.push(Unit { group, options: bit });
                }
            }
        }
        ManaProduction::AnyOneOf(set) => units.push(Unit { group, options: set.bits() }),
        ManaProduction::AnyColor => units.push(Unit { group, options: W | U | B | R | G }),
        ManaProduction::Custom(_) => {}
    }
}

/// Collect the player's usable tap-for-mana sources.
fn collect(gs: &GameState, seat: Seat) -> (Vec<Group>, Vec<Unit>) {
    let mut groups = Vec::new();
    let mut units = Vec::new();
    for &id in &gs.player(seat).battlefield {
        let o = gs.obj(id);
        if o.tapped || o.token.is_some() {
            continue;
        }
        // Creatures with tap abilities respect summoning sickness.
        if o.sick && o.is_creature() {
            continue;
        }
        let cf = gs.db.compiled_face(o.card, o.face);
        for (i, ma) in cf.mana_abilities.iter().enumerate() {
            // The auto-solver only uses plain tap abilities; anything with
            // extra costs is left to explicit activation.
            if !ma.cost.tap_self
                || ma.cost.mana.is_some()
                || ma.cost.sac_self
                || ma.cost.pay_life > 0
            {
                continue;
            }
            let group = groups.len() as u16;
            groups.push(Group {
                obj: id,
                ability: i as u8,
                fixed: matches!(ma.produce, ManaProduction::Fixed { .. }),
            });
            production_units(&ma.produce, &mut units, group);
            break;
        }
    }
    (groups, units)
}

struct Solver<'a> {
    units: &'a [Unit],
    /// 0 = unused, otherwise the demand mask this unit paid.
    paid: Vec<u8>,
    nodes: u32,
}

impl<'a> Solver<'a> {
    fn solve(&mut self, demands: &mut Vec<u8>, generic: u32) -> bool {
        self.nodes += 1;
        if self.nodes > 50_000 {
            return false;
        }
        let pick = demands
            .iter()
            .enumerate()
            .min_by_key(|(_, &mask)| {
                self.units
                    .iter()
                    .zip(&self.paid)
                    .filter(|(u, paid)| **paid == 0 && u.options & mask != 0)
                    .count()
            })
            .map(|(i, _)| i);
        match pick {
            None => {
                let free = self.paid.iter().filter(|p| **p == 0).count() as u32;
                free >= generic
            }
            Some(i) => {
                let mask = demands.swap_remove(i);
                for ui in 0..self.units.len() {
                    if self.paid[ui] == 0 && self.units[ui].options & mask != 0 {
                        self.paid[ui] = mask;
                        if self.solve(demands, generic) {
                            return true;
                        }
                        self.paid[ui] = 0;
                    }
                }
                demands.push(mask);
                false
            }
        }
    }
}

/// Compute a payment plan for a cost with X bound. None when unpayable.
pub fn solve(gs: &GameState, seat: Seat, cost: &ManaCost, x: u32) -> Option<TapPlan> {
    solve_with_delta(gs, seat, cost, x, 0)
}

/// Like solve, with a generic-cost adjustment (cost reductions, commander
/// tax). The adjusted generic never goes below zero.
pub fn solve_with_delta(
    gs: &GameState,
    seat: Seat,
    cost: &ManaCost,
    x: u32,
    generic_delta: i32,
) -> Option<TapPlan> {
    let mut plan = TapPlan::default();
    let mut pool_left = gs.player(seat).mana.pips;

    let mut demands: Vec<u8> = Vec::new();
    let mut generic = adjusted_generic(cost, x, generic_delta);

    for c in Color::ALL {
        let mut need = cost.pips[c.index()] as u16;
        let pi = c.index();
        let from_pool = need.min(pool_left[pi]);
        pool_left[pi] -= from_pool;
        plan.use_pool[pi] += from_pool;
        need -= from_pool;
        for _ in 0..need {
            demands.push(color_bit(c));
        }
    }
    {
        let mut need = cost.colorless as u16;
        let from_pool = need.min(pool_left[5]);
        pool_left[5] -= from_pool;
        plan.use_pool[5] += from_pool;
        need -= from_pool;
        for _ in 0..need {
            demands.push(C);
        }
    }
    for p in &cost.phyrexian {
        let mask = color_bit(p.0) | p.1.map(color_bit).unwrap_or(0);
        let mut paid = false;
        for c in Color::ALL {
            if mask & color_bit(c) != 0 && pool_left[c.index()] > 0 {
                pool_left[c.index()] -= 1;
                plan.use_pool[c.index()] += 1;
                paid = true;
                break;
            }
        }
        if !paid {
            demands.push(mask | 0x80); // High bit: 2-life fallback allowed.
        }
    }
    for h in &cost.hybrid {
        match h {
            HybridPip::Colors(a, b) => {
                let mask = color_bit(*a) | color_bit(*b);
                let mut paid = false;
                for c in [a, b] {
                    if pool_left[c.index()] > 0 {
                        pool_left[c.index()] -= 1;
                        plan.use_pool[c.index()] += 1;
                        paid = true;
                        break;
                    }
                }
                if !paid {
                    demands.push(mask);
                }
            }
            HybridPip::TwoOr(c) => {
                if pool_left[c.index()] > 0 {
                    pool_left[c.index()] -= 1;
                    plan.use_pool[c.index()] += 1;
                } else {
                    demands.push(color_bit(*c) | 0x40); // Generic+2 fallback.
                }
            }
        }
    }
    for pi in [5usize, 0, 1, 2, 3, 4] {
        let take = (generic.min(u16::MAX as u32) as u16).min(pool_left[pi]);
        pool_left[pi] -= take;
        plan.use_pool[pi] += take;
        generic -= take as u32;
        if generic == 0 {
            break;
        }
    }

    let (groups, units) = collect(gs, seat);

    // Expand fallback-marked demands into concrete attempts, mana-first.
    let mut attempts: Vec<(Vec<u8>, u32, u8)> = vec![(Vec::new(), 0, 0)];
    for &d in &demands {
        let mut next = Vec::new();
        for (ds, eg, el) in attempts {
            if d & 0x80 != 0 {
                let mut a = ds.clone();
                a.push(d & 0x3f);
                next.push((a, eg, el));
                next.push((ds, eg, el + 2));
            } else if d & 0x40 != 0 {
                let mut a = ds.clone();
                a.push(d & 0x3f);
                next.push((a, eg, el));
                next.push((ds, eg + 2, el));
            } else {
                let mut a = ds;
                a.push(d);
                next.push((a, eg, el));
            }
        }
        attempts = next;
        if attempts.len() > 64 {
            attempts.truncate(64);
        }
    }

    for (mut ds, extra_generic, extra_life) in attempts {
        if extra_life > 0 && gs.player(seat).life <= extra_life as i32 {
            continue;
        }
        let mut solver = Solver { units: &units, paid: vec![0; units.len()], nodes: 0 };
        if !solver.solve(&mut ds, generic + extra_generic) {
            continue;
        }
        // Cover generic with leftover units, preferring groups already
        // being tapped (their extra units are free mana).
        let mut need = generic + extra_generic;
        let mut group_used: Vec<bool> = vec![false; groups.len()];
        for (ui, p) in solver.paid.iter().enumerate() {
            if *p != 0 {
                group_used[units[ui].group as usize] = true;
            }
        }
        for pass in 0..2 {
            for (ui, u) in units.iter().enumerate() {
                if need == 0 {
                    break;
                }
                if solver.paid[ui] != 0 {
                    continue;
                }
                let in_used_group = group_used[u.group as usize];
                if (pass == 0 && in_used_group) || (pass == 1 && !in_used_group) {
                    solver.paid[ui] = u.options;
                    group_used[u.group as usize] = true;
                    need -= 1;
                }
            }
        }
        if need > 0 {
            continue;
        }
        // Build per-tap pool additions from the assignment.
        for (gi, used) in group_used.iter().enumerate() {
            if !*used {
                continue;
            }
            let g = groups[gi];
            let mut add = [0u8; 6];
            for (ui, u) in units.iter().enumerate() {
                if u.group as usize != gi {
                    continue;
                }
                if g.fixed {
                    // Fixed sources physically add every unit they make.
                    add[bit_to_index(u.options)] += 1;
                } else if solver.paid[ui] != 0 {
                    let chosen = u.options & solver.paid[ui];
                    let bit = 1u8 << chosen.trailing_zeros();
                    add[bit_to_index(bit)] += 1;
                }
            }
            plan.taps.push((g.obj, g.ability, add));
        }
        plan.pay_life = extra_life;
        return Some(plan);
    }
    None
}

fn adjusted_generic(cost: &ManaCost, x: u32, generic_delta: i32) -> u32 {
    (cost.generic as i64 + cost.snow as i64 + (cost.x_count as i64) * x as i64 + generic_delta as i64)
        .max(0) as u32
}

/// Execute a plan: tap the sources, add the recorded production to the
/// pool, then remove the cost. Returns false if the pool cannot cover the
/// cost, which indicates a solver bug; callers treat that as illegal.
pub fn execute(gs: &mut GameState, seat: Seat, plan: &TapPlan, cost: &ManaCost, x: u32) -> bool {
    execute_with_delta(gs, seat, plan, cost, x, 0)
}

pub fn execute_with_delta(
    gs: &mut GameState,
    seat: Seat,
    plan: &TapPlan,
    cost: &ManaCost,
    x: u32,
    generic_delta: i32,
) -> bool {
    for &(obj, _ability, add) in &plan.taps {
        if gs.obj(obj).tapped {
            return false;
        }
        gs.obj_mut(obj).tapped = true;
        let pool = &mut gs.player_mut(seat).mana.pips;
        for (i, n) in add.iter().enumerate() {
            pool[i] += *n as u16;
        }
    }
    if plan.pay_life > 0 {
        gs.player_mut(seat).life -= plan.pay_life as i32;
    }

    let pool = &mut gs.player_mut(seat).mana.pips;
    let mut ok = true;
    fn take(pool: &mut [u16; 6], idx: usize, n: u16) -> u16 {
        let got = n.min(pool[idx]);
        pool[idx] -= got;
        got
    }
    for c in Color::ALL {
        let need = cost.pips[c.index()] as u16;
        if take(pool, c.index(), need) < need {
            ok = false;
        }
    }
    if take(pool, 5, cost.colorless as u16) < cost.colorless as u16 {
        ok = false;
    }
    let mut life_credit = plan.pay_life;
    for p in &cost.phyrexian {
        let mut paid = false;
        for c in [Some(p.0), p.1].into_iter().flatten() {
            if pool[c.index()] > 0 {
                pool[c.index()] -= 1;
                paid = true;
                break;
            }
        }
        if !paid && life_credit >= 2 {
            life_credit -= 2;
            paid = true;
        }
        if !paid {
            ok = false;
        }
    }
    let mut generic = adjusted_generic(cost, x, generic_delta);
    for h in &cost.hybrid {
        match h {
            HybridPip::Colors(a, b) => {
                let mut paid = false;
                for c in [a, b] {
                    if pool[c.index()] > 0 {
                        pool[c.index()] -= 1;
                        paid = true;
                        break;
                    }
                }
                if !paid {
                    ok = false;
                }
            }
            HybridPip::TwoOr(c) => {
                if pool[c.index()] > 0 {
                    pool[c.index()] -= 1;
                } else {
                    generic += 2;
                }
            }
        }
    }
    for pi in [5usize, 0, 1, 2, 3, 4] {
        let t = take(pool, pi, generic.min(u16::MAX as u32) as u16);
        generic -= t as u32;
        if generic == 0 {
            break;
        }
    }
    if generic > 0 {
        ok = false;
    }
    ok
}

/// Add a production to a player's pool with a caller-chosen color for
/// choice productions (used by explicit mana ability activation).
pub fn add_production(gs: &mut GameState, seat: Seat, p: &ManaProduction, choice: Option<Color>) {
    let pool = &mut gs.player_mut(seat).mana.pips;
    match p {
        ManaProduction::Fixed { w, u, b, r, g, c } => {
            pool[0] += *w as u16;
            pool[1] += *u as u16;
            pool[2] += *b as u16;
            pool[3] += *r as u16;
            pool[4] += *g as u16;
            pool[5] += *c as u16;
        }
        ManaProduction::AnyOneOf(set) => {
            let c = choice
                .filter(|c| set.contains(c.set()))
                .or_else(|| set.colors().next());
            if let Some(c) = c {
                pool[c.index()] += 1;
            } else if set.contains(ColorSet::C) {
                pool[5] += 1;
            }
        }
        ManaProduction::AnyColor => {
            pool[choice.unwrap_or(Color::W).index()] += 1;
        }
        ManaProduction::Custom(_) => {}
    }
}
