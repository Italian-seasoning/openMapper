//! Which transport modes a vehicle is for, as openOMSI decides it (`Vehicle::transport_modes`):
//! the `[bus_transport]`… keywords its `.bus` / `.ovh` names, else its shape: a rail vehicle
//! (`[type]` 2, `[boogies]`, `[contact_shoe]`, `[rail_body_osc]`) is the tram, metro or
//! railway train its `[rail_class]` says, a metro with a contact shoe, a railway train over
//! 45 t, else a tram; a vehicle with `[trolley_pole]` gear a bus and a trolleybus; anything else
//! a bus.

use std::collections::HashMap;

use super::{Mode, Modes};

/// A vehicle's modes and whether its file declares them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleModes {
    pub modes: Modes,
    pub declared: bool,
}

impl VehicleModes {
    /// Whether openOMSI runs it on a map made for `map` (a map that names none takes all).
    pub fn fits(self, map: Modes) -> bool {
        map.is_empty() || self.modes.iter().any(|m| map.contains(m))
    }
}

/// The modes of a vehicle file's text.
pub fn of_text(text: &str) -> VehicleModes {
    let mut declared = Modes::default();
    let (mut rail, mut shoe, mut trolley) = (false, false, false);
    let mut class: Option<Mode> = None;
    let mut mass_t = 0.0f64;
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    for (i, l) in lines.iter().enumerate() {
        let Some(k) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) else { continue };
        let next = || lines.get(i + 1).copied().unwrap_or("");
        match k.to_ascii_lowercase().as_str() {
            "type" => rail |= next().trim() == "2",
            "boogies" | "rail_body_osc" => rail = true,
            "contact_shoe" => {
                rail = true;
                shoe = true;
            }
            "rail_class" => class = Mode::from_name(next()).filter(|m| m.is_rail()),
            "trolley_pole" => trolley = true,
            "mass" => mass_t = next().replace(',', ".").trim().parse().unwrap_or(0.0),
            k => {
                if let Some(m) = Mode::from_keyword(k) {
                    declared.set(m, true);
                }
            }
        }
    }
    if !declared.is_empty() {
        return VehicleModes { modes: declared, declared: true };
    }
    let modes = if rail {
        Modes::of(&[class.unwrap_or(if shoe {
            Mode::Metro
        } else if mass_t > 45.0 {
            Mode::Railway
        } else {
            Mode::Tram
        })])
    } else if trolley {
        Modes::of(&[Mode::Bus, Mode::Trolleybus])
    } else {
        Modes::of(&[Mode::Bus])
    };
    VehicleModes { modes, declared: false }
}

/// The modes of the vehicle files a map names, each read once from the content roots (None:
/// not installed).
#[derive(Default)]
pub struct Known(HashMap<String, Option<VehicleModes>>);

impl Known {
    pub fn get(&mut self, rel: &str) -> Option<VehicleModes> {
        *self.0.entry(rel.trim().to_lowercase()).or_insert_with(|| {
            let (_, path) = omsi_cfg::find_in_roots(rel.trim())?;
            let bytes = std::fs::read(path).ok()?;
            Some(of_text(&omsi_cfg::decode_text(&bytes)))
        })
    }
}

/// The vehicle files of an AI group (its vehicles and its depot's type groups).
pub fn group_files(g: &omsi_map::AiGroup) -> Vec<String> {
    g.vehicles.iter().map(|v| v.file.clone()).chain(g.typgroups.iter().map(|t| t.file.clone())).filter(|f| !f.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_openomsi_judges_a_vehicle() {
        assert_eq!(of_text("[mass]\n12\n"), VehicleModes { modes: Modes::of(&[Mode::Bus]), declared: false });
        assert_eq!(of_text("[mass]\n18\n[boogies]\n1\n").modes, Modes::of(&[Mode::Tram]));
        assert_eq!(of_text("[mass]\n30\n[boogies]\n1\n[contact_shoe]\n1\n").modes, Modes::of(&[Mode::Metro]));
        assert_eq!(of_text("[mass]\n52\n[boogies]\n1\n").modes, Modes::of(&[Mode::Railway]));
        assert_eq!(of_text("[mass]\n52\n[boogies]\n1\n[rail_class]\ntram\n").modes, Modes::of(&[Mode::Tram]));
        assert_eq!(of_text("[mass]\n12\n[trolley_pole]\nside left\n").modes, Modes::of(&[Mode::Bus, Mode::Trolleybus]));
        let v = of_text("[mass]\n18\n[boogies]\n1\n[Railway_Transport]\n");
        assert_eq!(v, VehicleModes { modes: Modes::of(&[Mode::Railway]), declared: true });
        assert!(v.fits(Modes::default()) && !v.fits(Modes::of(&[Mode::Tram])));
    }
}
