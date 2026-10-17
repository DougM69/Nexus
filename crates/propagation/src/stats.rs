//! Descriptive logbook geography — the continent / CQ-zone / DX-vs-domestic breakdowns the
//! frontend `StatsView` can't compute on its own. Each contact is placed where the awards count
//! it ([`counted_entity`], [`counted_zone`]): in the DXCC entity and CQ zone its record carries
//! (LoTW's DXCC first), else the ones cty.dat gives its call; the stored record carries no
//! continent, so that is its zone's. Pure over callsigns and what their records carry, plus the
//! operator's own call, so it unit-tests offline.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::awards::{counted_entity, counted_zone, RecordFields};
use crate::dxcc;
use crate::journey::{continent_of_zone, CONTINENTS};

/// QSOs worked on one WAC continent, plus how many distinct DXCC entities they span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinentTally {
    /// WAC code: NA / SA / EU / AS / OC / AF.
    pub continent: String,
    /// QSOs worked with stations on this continent.
    pub qsos: usize,
    /// Distinct DXCC entities worked on this continent.
    pub entities: usize,
}

/// QSOs worked in one CQ zone (1–40).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneTally {
    pub zone: u8,
    pub qsos: usize,
}

/// The geographic slice of the logbook: continent / CQ-zone / DX-vs-domestic. Everything is keyed
/// on the entity each QSO counts toward — a QSO in none (a call cty.dat can't place with no DXCC
/// number on its record, or a record's DXCC 0) is counted in `total` but in no breakdown, surfaced
/// honestly as `total - resolved`. This is descriptive ("who have I worked, geographically"), NOT
/// award credit: a WAE/CQ-only area counts as the entity it belongs to (Sicily as Italy), as the
/// awards count it, and a deleted entity as itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogStats {
    /// All QSOs scanned.
    pub total: usize,
    /// QSOs placed in a DXCC entity (the base for every breakdown).
    pub resolved: usize,
    /// Resolved QSOs with an entity different from the operator's own (entity-based DX).
    pub dx: usize,
    /// Resolved QSOs with the operator's own entity (domestic).
    pub domestic: usize,
    /// By WAC continent, in canonical NA→AF order (continents with no QSOs are omitted).
    pub by_continent: Vec<ContinentTally>,
    /// By CQ zone, ascending; only zones with at least one QSO.
    pub by_zone: Vec<ZoneTally>,
}

/// Roll a set of worked callsigns into the geographic stats, each by its call alone (a record
/// carrying no DXCC number or CQ zone), anchored on the operator's own call for the
/// DX-vs-domestic split (entity equality, matching the Awards engine). If the operator's own call
/// can't be resolved, every resolved QSO counts as DX (there's no home entity to be domestic
/// against).
pub fn compute_log_stats<S: AsRef<str>>(calls: &[S], my_call: &str) -> LogStats {
    let mut stats = LogStatsAccumulator::new(my_call);
    for call in calls {
        stats.add(call.as_ref(), RecordFields::default());
    }
    stats.summary()
}

/// The same geographic fold, usable across short station-log read batches.
/// State holds counts and resolved entity sets, never a copy of the contact list.
#[derive(Default)]
pub struct LogStatsAccumulator {
    my_entity: Option<&'static str>,
    total: usize,
    resolved: usize,
    dx: usize,
    domestic: usize,
    cont_qsos: HashMap<&'static str, usize>,
    cont_entities: HashMap<&'static str, HashSet<&'static str>>,
    zone_qsos: HashMap<u8, usize>,
}
impl LogStatsAccumulator {
    /// Home is the entity a contact from `my_call` counts toward, so the two compare alike (an
    /// IT9 operator is home in Italy).
    pub fn new(my_call: &str) -> Self {
        Self {
            my_entity: counted_entity(dxcc::resolve(my_call).as_ref(), &RecordFields::default())
                .map(|c| c.name),
            ..Self::default()
        }
    }
    /// Count one contact: its call, and what its record carries beyond it.
    pub fn add(&mut self, call: &str, rec: RecordFields) {
        self.total += 1;
        let resolved = dxcc::resolve(call);
        let Some(entity) = counted_entity(resolved.as_ref(), &rec) else {
            return;
        };
        self.resolved += 1;
        if self.my_entity.is_some() && Some(entity.name) == self.my_entity {
            self.domestic += 1;
        } else {
            self.dx += 1;
        }
        let Some(zone) = counted_zone(resolved.as_ref(), &rec) else {
            return;
        };
        if let Some(cont) = continent_of_zone(zone) {
            *self.cont_qsos.entry(cont).or_default() += 1;
            self.cont_entities
                .entry(cont)
                .or_default()
                .insert(entity.name);
        }
        *self.zone_qsos.entry(zone).or_default() += 1;
    }
    pub fn summary(&self) -> LogStats {
        let by_continent = CONTINENTS
            .iter()
            .filter_map(|&cont| {
                let qsos = *self.cont_qsos.get(cont)?;
                Some(ContinentTally {
                    continent: cont.to_string(),
                    qsos,
                    entities: self.cont_entities.get(cont).map_or(0, HashSet::len),
                })
            })
            .collect();
        let mut by_zone: Vec<ZoneTally> = self
            .zone_qsos
            .iter()
            .map(|(&zone, &qsos)| ZoneTally { zone, qsos })
            .collect();
        by_zone.sort_by_key(|z| z.zone);
        LogStats {
            total: self.total,
            resolved: self.resolved,
            dx: self.dx,
            domestic: self.domestic,
            by_continent,
            by_zone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_log_is_all_zeros() {
        let s = compute_log_stats::<&str>(&[], "W1AW");
        assert_eq!(s, LogStats::default());
    }

    #[test]
    fn splits_dx_from_domestic_by_entity() {
        // Operator in the US: W/K/N calls are domestic; JA/DL/G are DX.
        let calls = ["W1AW", "K9XYZ", "JA1ABC", "DL1XX", "G3ZZZ"];
        let s = compute_log_stats(&calls, "KD9TAW");
        assert_eq!(s.total, 5);
        assert_eq!(s.resolved, 5);
        assert_eq!(s.domestic, 2); // W1AW + K9XYZ
        assert_eq!(s.dx, 3); // JA + DL + G
    }

    #[test]
    fn groups_by_continent_and_zone() {
        // W = NA (zones 3–5), JA = AS (zone 25), DL = EU (zone 14).
        let calls = ["W1AW", "K6XX", "JA1ABC", "DL1XX"];
        let s = compute_log_stats(&calls, "KD9TAW");
        let na = s.by_continent.iter().find(|c| c.continent == "NA").unwrap();
        assert_eq!(na.qsos, 2);
        assert_eq!(na.entities, 1); // both are "United States"
        assert!(s
            .by_continent
            .iter()
            .any(|c| c.continent == "AS" && c.qsos == 1));
        assert!(s
            .by_continent
            .iter()
            .any(|c| c.continent == "EU" && c.qsos == 1));
        // continents come back in canonical order, no empties
        let order: Vec<&str> = s
            .by_continent
            .iter()
            .map(|c| c.continent.as_str())
            .collect();
        assert_eq!(order, ["NA", "EU", "AS"]);
        // zones ascending, each present once
        assert!(s.by_zone.windows(2).all(|w| w[0].zone < w[1].zone));
        assert!(s.by_zone.iter().any(|z| z.zone == 14 && z.qsos == 1)); // DL
    }

    #[test]
    fn unresolvable_calls_count_in_total_only() {
        // An empty call resolves to nothing (the prefix resolver places almost any letter-led
        // string, so a blank is the clean "unresolvable" case). It still counts toward `total`.
        let calls = ["W1AW", ""];
        let s = compute_log_stats(&calls, "KD9TAW");
        assert_eq!(s.total, 2);
        assert_eq!(s.resolved, 1); // only W1AW places
        assert_eq!(s.domestic + s.dx, s.resolved);
    }

    #[test]
    fn unresolvable_operator_call_makes_everything_dx() {
        let calls = ["W1AW", "JA1ABC"];
        let s = compute_log_stats(&calls, "");
        assert_eq!(s.resolved, 2);
        assert_eq!(s.domestic, 0);
        assert_eq!(s.dx, 2);
    }
}
