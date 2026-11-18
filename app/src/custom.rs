//! Custom training schedules: weekly templates the user builds (or copies from a week in FM)
//! and assigns to coming weeks. The live writer (`live_training::run`) uses them instead of the
//! automatic planner for the weeks they are assigned to.
//!
//! A schedule has 7 days × 3 slots. A slot holds an FM27 session code, or `None` to keep whatever
//! FM has on that slot. Match and travel days always stay with FM, like in the automatic plan.
//! With `follow_match_rhythm` the days around a match (day before, day after, between two matches)
//! come from the automatic planner, so a schedule built for a Saturday match still works in a
//! week where the match is on Wednesday.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::planner::DayRole;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomSchedule {
    pub name: String,
    /// Monday … Sunday, each [session 1, session 2, extra].
    pub days: [[Option<u16>; 3]; 7],
    /// Days around matches follow the automatic planner's rhythm instead of this template.
    #[serde(default = "yes")]
    pub follow_match_rhythm: bool,
    /// The author's description (from an imported .fmf).
    #[serde(default)]
    pub description: String,
}

fn yes() -> bool {
    true
}

impl CustomSchedule {
    pub fn new(name: &str) -> Self {
        CustomSchedule { name: name.to_string(), days: [[None; 3]; 7], follow_match_rhythm: true, description: String::new() }
    }

    /// A schedule copied from a week as FM has it now (match and travel slots become "keep FM's").
    pub fn from_codes(name: &str, codes: &[[u16; 3]]) -> Self {
        let mut s = CustomSchedule::new(name);
        for (d, day) in codes.iter().take(7).enumerate() {
            for (i, c) in day.iter().enumerate() {
                s.days[d][i] = Some(*c).filter(|c| selectable(*c));
            }
        }
        s
    }
}

/// Which session codes a schedule may contain: every known session except Match and travel.
pub fn selectable(code: u16) -> bool {
    code != 500 && code != 402 && crate::model::session_by_code(code).is_some()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomTraining {
    #[serde(default)]
    pub schedules: Vec<CustomSchedule>,
    /// Week (Monday) → schedule name. Overrides `default`.
    #[serde(default)]
    pub assigned: BTreeMap<NaiveDate, String>,
    /// Schedule for every week without its own assignment (None = automatic planner).
    #[serde(default)]
    pub default: Option<String>,
    /// Repeating pattern: schedules used week after week in this order, then from the start again
    /// (C1, C2, C3, C4, C1, …). Overrides `default`; a week's own assignment overrides the pattern.
    #[serde(default)]
    pub pattern: Vec<String>,
    /// Monday of the week that gets the first entry of the pattern.
    #[serde(default)]
    pub pattern_start: Option<NaiveDate>,
    /// The pattern can be switched off without losing it.
    #[serde(default)]
    pub pattern_on: bool,
}

/// Where a week's schedule comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Assigned,
    /// Entry `n` (0-based) of the pattern.
    Pattern(usize),
    Default,
}

impl CustomTraining {
    pub fn get(&self, name: &str) -> Option<&CustomSchedule> {
        self.schedules.iter().find(|s| s.name == name)
    }

    /// The schedule for a week and whether it was assigned to that week explicitly.
    pub fn for_week(&self, monday: NaiveDate) -> Option<(&CustomSchedule, bool)> {
        self.source(monday).map(|(s, src)| (s, src == Source::Assigned))
    }

    /// The schedule for a week and where it comes from: the week's own choice, then the pattern,
    /// then the default. None = automatic planner.
    pub fn source(&self, monday: NaiveDate) -> Option<(&CustomSchedule, Source)> {
        if let Some(n) = self.assigned.get(&monday) {
            // an own choice that names no schedule = automatic planner for this week
            return self.get(n).map(|s| (s, Source::Assigned));
        }
        if let Some(i) = self.pattern_index(monday) {
            if let Some(s) = self.get(&self.pattern[i]) {
                return Some((s, Source::Pattern(i)));
            }
        }
        self.default.as_deref().and_then(|n| self.get(n)).map(|s| (s, Source::Default))
    }

    /// Which entry of the pattern a week gets (None: no pattern, or the week is before its start).
    pub fn pattern_index(&self, monday: NaiveDate) -> Option<usize> {
        let start = self.pattern_start?;
        if !self.pattern_on || self.pattern.is_empty() || monday < start {
            return None;
        }
        let weeks = (monday - start).num_days().div_euclid(7) as usize;
        Some(weeks % self.pattern.len())
    }

    /// Assign a schedule (None = automatic planner) to `count` weeks from `first` on.
    pub fn assign(&mut self, first: NaiveDate, count: u32, name: Option<&str>) {
        for w in 0..count {
            let monday = first + chrono::Duration::weeks(w as i64);
            match name {
                Some(n) => {
                    self.assigned.insert(monday, n.to_string());
                }
                None => {
                    self.assigned.remove(&monday);
                }
            }
        }
    }

    /// Drop assignments of weeks that are over.
    pub fn prune(&mut self, today: NaiveDate) {
        self.assigned.retain(|monday, _| *monday + chrono::Duration::days(6) > today);
    }

    pub fn rename(&mut self, old: &str, new: &str) {
        for s in self.schedules.iter_mut().filter(|s| s.name == old) {
            s.name = new.to_string();
        }
        for v in self.assigned.values_mut().filter(|v| *v == old) {
            *v = new.to_string();
        }
        if self.default.as_deref() == Some(old) {
            self.default = Some(new.to_string());
        }
        for v in self.pattern.iter_mut().filter(|v| *v == old) {
            *v = new.to_string();
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.schedules.retain(|s| s.name != name);
        self.assigned.retain(|_, v| v != name);
        if self.default.as_deref() == Some(name) {
            self.default = None;
        }
        self.pattern.retain(|v| v != name);
    }

    /// A name that is not in use yet ("Custom 1", "Custom 2", …).
    pub fn free_name(&self, base: &str) -> String {
        (1..).map(|i| format!("{base} {i}")).find(|n| self.get(n).is_none()).unwrap_or_default()
    }
}

/// The week as it should be in FM with this schedule.
///
/// * `current`: what FM has now; `auto`: what the automatic planner would set;
/// * `roles`: the planner's day roles (match rhythm); `editable`: days the app may change.
///
/// Slots on days that are not editable stay as they are, `None` slots keep FM's session.
pub fn apply(s: &CustomSchedule, current: &[[u16; 3]], auto: &[[u16; 3]], roles: &[DayRole], editable: &[bool; 7]) -> Vec<[u16; 3]> {
    let mut out = current.to_vec();
    for d in 0..7.min(out.len()) {
        if !editable[d] {
            continue;
        }
        let around_match = matches!(roles.get(d), Some(DayRole::Match | DayRole::PreMatch | DayRole::PostMatch | DayRole::PostAndPreMatch));
        if s.follow_match_rhythm && around_match {
            if let Some(a) = auto.get(d) {
                out[d] = *a;
            }
            continue;
        }
        for i in 0..3 {
            if let Some(c) = s.days[d][i].filter(|c| selectable(*c)) {
                out[d][i] = c;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2027, 3, 1).unwrap() + chrono::Duration::days(n as i64)
    }

    #[test]
    fn assignment_and_default() {
        let mut c = CustomTraining::default();
        c.schedules.push(CustomSchedule::new("A"));
        c.schedules.push(CustomSchedule::new("B"));
        let mon = day(0);
        assert!(c.for_week(mon).is_none());
        c.default = Some("A".into());
        assert_eq!(c.for_week(mon).map(|(s, e)| (s.name.as_str(), e)), Some(("A", false)));
        c.assign(mon, 3, Some("B"));
        assert_eq!(c.for_week(day(14)).map(|(s, e)| (s.name.as_str(), e)), Some(("B", true)));
        assert_eq!(c.for_week(day(21)).map(|(s, _)| s.name.as_str()), Some("A"));
        c.rename("B", "C");
        assert_eq!(c.assigned.get(&day(7)).map(String::as_str), Some("C"));
        c.remove("A");
        assert!(c.default.is_none());
        c.prune(day(10));
        assert_eq!(c.assigned.keys().copied().collect::<Vec<_>>(), vec![day(7), day(14)]);
        c.assign(day(7), 1, None);
        assert_eq!(c.assigned.len(), 1);
    }

    #[test]
    fn repeating_pattern() {
        let mut c = CustomTraining::default();
        for n in ["C1", "C2", "C3", "C4", "D"] {
            c.schedules.push(CustomSchedule::new(n));
        }
        c.default = Some("D".into());
        c.pattern = ["C1", "C2", "C3", "C4"].map(String::from).to_vec();
        c.pattern_start = Some(day(7));
        let name = |c: &CustomTraining, w: u32| c.source(day(7 * w)).map(|(s, src)| (s.name.clone(), src));
        assert_eq!(name(&c, 1).unwrap().0, "D", "pattern off");
        c.pattern_on = true;
        assert_eq!(name(&c, 0).unwrap().0, "D", "before the start");
        let got: Vec<String> = (1..=9).map(|w| name(&c, w).unwrap().0).collect();
        assert_eq!(got, ["C1", "C2", "C3", "C4", "C1", "C2", "C3", "C4", "C1"]);
        assert_eq!(name(&c, 3).unwrap().1, Source::Pattern(2));
        c.assign(day(14), 1, Some("D"));
        assert_eq!(name(&c, 2), Some(("D".into(), Source::Assigned)), "a week's own choice wins");
        c.assigned.insert(day(21), String::new());
        assert_eq!(name(&c, 3), None, "automatic for this week only");
        c.rename("C2", "Two");
        assert_eq!(c.pattern[1], "Two");
        c.remove("C3");
        assert_eq!(c.pattern, ["C1", "Two", "C4"]);
    }

    #[test]
    fn apply_respects_fm_days_and_match_rhythm() {
        let mut s = CustomSchedule::new("X");
        for d in 0..7 {
            s.days[d] = [Some(0), Some(200), None];
        }
        s.days[2][1] = Some(500); // a match code in a schedule is never written
        let current = vec![[300, 300, 300], [1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4], [0, 500, 0], [5, 5, 5]];
        let auto = vec![[9, 9, 9]; 7];
        let roles = [DayRole::Training, DayRole::Training, DayRole::Training, DayRole::Training, DayRole::PreMatch, DayRole::Match, DayRole::PostMatch];
        let editable = [false, true, true, true, true, false, true];
        let out = apply(&s, &current, &auto, &roles, &editable);
        assert_eq!(out[0], [300, 300, 300], "not editable (past)");
        assert_eq!(out[1], [0, 200, 1], "None keeps FM's");
        assert_eq!(out[2], [0, 2, 2], "match code ignored");
        assert_eq!(out[4], [9, 9, 9], "day before the match from the planner");
        assert_eq!(out[5], [0, 500, 0], "match day stays");
        assert_eq!(out[6], [9, 9, 9], "day after the match from the planner");
        s.follow_match_rhythm = false;
        let out = apply(&s, &current, &auto, &roles, &editable);
        assert_eq!(out[4], [0, 200, 4]);
        assert_eq!(out[6], [0, 200, 5]);
    }

    #[test]
    fn copy_from_fm_week() {
        let s = CustomSchedule::from_codes("FM", &[[0, 500, 0], [402, 1, 200], [1, 2, 3], [0; 3], [0; 3], [0; 3], [999, 0, 0]]);
        assert_eq!(s.days[0], [Some(0), None, Some(0)]);
        assert_eq!(s.days[1], [None, Some(1), Some(200)]);
        assert_eq!(s.days[6][0], None, "unknown code");
    }
}
