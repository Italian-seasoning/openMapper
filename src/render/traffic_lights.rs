//! What a signal head shows: the light program of the crossing it takes its state from
//! (`[varparent]`), run by the editor's clock as OMSI 2's stock `ampel1.osc` runs it - the
//! phase `TrafficLightPhase` turned into the red, yellow and green lamps of its lenses.

use omsi_scenery::TrafficLightPhase;

use crate::editable::Lamp;

/// The lamps lit at one `[phase]`: 0..2 red, 3..5 red and yellow, 6..8 green, 9..11 yellow,
/// anything else (12 and up, or below 0) dark - the stock script's own rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lit {
    lit: [bool; 3],
}

impl Lit {
    pub fn of_state(state: i32) -> Lit {
        let mut lit = [false; 3];
        // (red, yellow, green: the order of the lenses' variables)
        lit[0] = (0..=5).contains(&state);
        lit[1] = (3..=5).contains(&state) || (9..=11).contains(&state);
        lit[2] = (6..=8).contains(&state);
        Lit { lit }
    }

    pub fn shows(self, lamp: Lamp) -> bool {
        self.lit[match lamp {
            Lamp::Red => 0,
            Lamp::Yellow => 1,
            Lamp::Green => 2,
        }]
    }

    pub fn is_dark(self) -> bool {
        !self.lit.iter().any(|l| *l)
    }
}

/// The phase an object with no crossing light shows: Omsi.exe binds a signal head's
/// `TrafficLightPhase` to a shared dummy variable when its `[varparent]` leads nowhere, and
/// that variable reads 0 - red, not "no light" (omsi-sim's `UNLINKED_PHASE`).
pub const UNLINKED_STATE: i32 = 0;

/// The state a light shows `t` seconds into its crossing's cycle: the `[phase]` whose seconds
/// `t` falls in. The cycle is its `[traffic_lights_group]`, else the phases' own sum; a last
/// phase of 0 s holds to the cycle's end, as does whatever the phases leave of a cycle longer
/// than they are. 12: dark (no program, or none yet).
pub fn state_at(phases: &[TrafficLightPhase], cycle: Option<f32>, t: f32) -> i32 {
    const DARK: i32 = 12;
    let Some(last) = phases.last() else { return DARK };
    let sum: f32 = phases.iter().map(|p| p.duration.max(0.0)).sum();
    let period = cycle.filter(|c| c.is_finite() && *c > 0.0).unwrap_or(sum);
    if !(period > 0.0) || !t.is_finite() {
        return DARK;
    }
    let x = t.rem_euclid(period);
    let mut at = 0.0;
    for p in phases {
        let d = p.duration.max(0.0);
        if x < at + d {
            return p.state;
        }
        at += d;
    }
    // (what the phases leave of the cycle: the last one still shows)
    last.state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phases(list: &[(i32, f32)]) -> Vec<TrafficLightPhase> {
        list.iter().map(|&(state, duration)| TrafficLightPhase { state, duration }).collect()
    }

    #[test]
    fn a_phase_lights_the_lamps_the_stock_script_lights() {
        let lit = |s| Lit::of_state(s);
        assert!(lit(0).shows(Lamp::Red) && !lit(0).shows(Lamp::Yellow) && !lit(0).shows(Lamp::Green));
        assert!(lit(2).shows(Lamp::Red));
        // (red and yellow together, before the green)
        assert!(lit(3).shows(Lamp::Red) && lit(3).shows(Lamp::Yellow) && !lit(3).shows(Lamp::Green));
        assert!(lit(5).shows(Lamp::Red) && lit(5).shows(Lamp::Yellow));
        assert!(lit(6).shows(Lamp::Green) && !lit(6).shows(Lamp::Red));
        assert!(lit(8).shows(Lamp::Green));
        // (yellow alone, after the green)
        assert!(lit(9).shows(Lamp::Yellow) && lit(11).shows(Lamp::Yellow) && !lit(9).shows(Lamp::Green));
        // (off: nothing lit)
        for s in [12, 13, 99, -1] {
            assert!(lit(s).is_dark(), "{s}");
        }
        assert!(!lit(6).is_dark());
    }

    #[test]
    fn the_phase_follows_the_cycle_and_comes_round() {
        let p = phases(&[(0, 2.0), (6, 3.0), (9, 1.0)]);
        assert_eq!(state_at(&p, Some(6.0), 0.0), 0);
        assert_eq!(state_at(&p, Some(6.0), 1.9), 0);
        assert_eq!(state_at(&p, Some(6.0), 2.0), 6);
        assert_eq!(state_at(&p, Some(6.0), 4.9), 6);
        assert_eq!(state_at(&p, Some(6.0), 5.0), 9);
        // (the cycle's end starts it again)
        assert_eq!(state_at(&p, Some(6.0), 6.0), 0);
        assert_eq!(state_at(&p, Some(6.0), 6.5), 0);
        // (without a `[traffic_lights_group]` the phases' sum is the cycle)
        assert_eq!(state_at(&p, None, 5.5), 9);
        assert_eq!(state_at(&p, None, 6.5), 0);
        // (a light with no phases is dark)
        assert_eq!(state_at(&[], Some(30.0), 1.0), 12);
        assert_eq!(state_at(&p, Some(0.0), 1.0), 0, "a cycle of nothing is the sum");
    }

    #[test]
    fn a_head_with_no_crossing_shows_red() {
        // (an unlinked head's `TrafficLightPhase` reads a shared dummy 0 in Omsi.exe: red,
        // not "no light")
        assert_eq!(UNLINKED_STATE, 0);
        let lit = Lit::of_state(UNLINKED_STATE);
        assert!(lit.shows(Lamp::Red));
        assert!(!lit.shows(Lamp::Yellow) && !lit.shows(Lamp::Green));
        assert!(!lit.is_dark());
    }

    #[test]
    fn a_last_phase_of_no_seconds_lasts_to_the_cycle_s_end() {
        // (as openMapper's own junctions write it: red-yellow, green, yellow, then 0 s of red)
        let p = phases(&[(3, 2.0), (6, 20.0), (9, 3.0), (0, 0.0)]);
        assert_eq!(state_at(&p, Some(48.0), 24.9), 9);
        assert_eq!(state_at(&p, Some(48.0), 25.0), 0);
        assert_eq!(state_at(&p, Some(48.0), 47.9), 0);
        assert_eq!(state_at(&p, Some(48.0), 48.0), 3, "and comes round");
    }

    #[test]
    fn a_faster_speed_shortens_the_cycle_without_changing_the_program() {
        use crate::render::daylight::seconds_after;
        // (red for 40 s, then green: an 82 s cycle)
        let p = phases(&[(0, 40.0), (6, 42.0)]);
        // (at 1x and at 5x the same program time shows the same lamp: only the editor time
        // it takes to reach it changes)
        assert_eq!(state_at(&p, Some(82.0), 50.0), 6);
        assert_eq!(state_at(&p, Some(82.0), seconds_after(0.0, 10.0, 5.0)), 6);
        assert_eq!(state_at(&p, Some(82.0), seconds_after(0.0, 50.0, 1.0)), 6);
        // (the whole 82 s cycle takes 16.4 s of the editor at 5x, then starts again)
        assert_eq!(state_at(&p, Some(82.0), seconds_after(0.0, 16.0, 5.0)), 6);
        assert_eq!(state_at(&p, Some(82.0), seconds_after(0.0, 16.4, 5.0)), 0);
        // (1x leaves the cycle on the game's own seconds)
        assert_eq!(state_at(&p, Some(82.0), seconds_after(0.0, 40.0, 1.0)), 6);
        assert_eq!(state_at(&p, Some(82.0), seconds_after(0.0, 10.0, 1.0)), 0);
    }
}
