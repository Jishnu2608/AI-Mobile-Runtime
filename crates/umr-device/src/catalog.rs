//! DEV-002 / DEV-008: the device catalogue — one profile per screen-size class (master build §12).
//!
//! Profiles emulate **size, not brand**: generic names, stock Android (Google APIs image), no
//! manufacturer frames. Foldable geometry and hinge/posture values follow the generic foldable
//! definitions Google ships with the SDK (`sdklib` `devices.xml`: "7.6\" Fold-in with outer
//! display", "6.7\" Horizontal Fold-in"), which the emulator is tested with.

use serde::{Deserialize, Serialize};

/// Foldable posture (values match the emulator's `Posture.PostureValue` and
/// `hw.sensor.posture_list` indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Posture {
    Closed = 1,
    HalfOpened = 2,
    Opened = 3,
    Flipped = 4,
    Tent = 5,
}

impl Posture {
    #[must_use]
    pub fn index(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "closed" | "folded" => Self::Closed,
            "half-opened" | "half-open" | "tabletop" => Self::HalfOpened,
            "opened" | "open" | "unfolded" => Self::Opened,
            "flipped" => Self::Flipped,
            "tent" => Self::Tent,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FoldKind {
    /// Opens like a book (vertical hinge); folded = narrow outer screen.
    Book,
    /// Clamshell (horizontal hinge).
    Flip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fold {
    pub kind: FoldKind,
    /// `hw.sensor.hinge.type`: 0 horizontal, 1 vertical.
    pub hinge_type: u8,
    /// `hw.sensor.hinge.areas` (x-y-width-height).
    pub hinge_area: &'static str,
    /// Display region shown when closed (x, y, width, height); `None` = no outer screen.
    pub folded_region: Option<(u32, u32, u32, u32)>,
    pub postures: &'static [Posture],
    /// `hw.sensor.hinge_angles_posture_definitions` (one range per posture).
    pub posture_angles: &'static str,
    pub hinge_range: &'static str,
    pub hinge_default: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Profile {
    pub id: &'static str,
    pub class: &'static str,
    pub diagonal_in: f64,
    pub width: u32,
    pub height: u32,
    pub density: u32,
    pub ram_mb: u32,
    pub data_gb: u32,
    pub default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fold: Option<Fold>,
}

const BOOK: &[Posture] = &[Posture::Closed, Posture::HalfOpened, Posture::Opened];

/// The MVP catalogue (master build §12; DEC-006: ≥ 4 GB guest RAM).
pub const PROFILES: &[Profile] = &[
    Profile {
        id: "compact",
        class: "Compact phone",
        diagonal_in: 6.1,
        width: 1080,
        height: 2340,
        density: 420,
        ram_mb: 4096,
        data_gb: 6,
        default: false,
        fold: None,
    },
    Profile {
        id: "standard",
        class: "Standard phone",
        diagonal_in: 6.3,
        width: 1080,
        height: 2400,
        density: 420,
        ram_mb: 4096,
        data_gb: 6,
        default: true,
        fold: None,
    },
    Profile {
        id: "large",
        class: "Large phone",
        diagonal_in: 6.8,
        width: 1440,
        height: 3120,
        density: 500,
        ram_mb: 4096,
        data_gb: 6,
        default: false,
        fold: None,
    },
    Profile {
        id: "fold",
        class: "Book foldable",
        diagonal_in: 7.6,
        width: 1768,
        height: 2208,
        density: 420,
        ram_mb: 4096,
        data_gb: 6,
        default: false,
        fold: Some(Fold {
            kind: FoldKind::Book,
            hinge_type: 1,
            hinge_area: "884-0-1-2208",
            folded_region: Some((0, 0, 884, 2208)),
            postures: BOOK,
            posture_angles: "0-30, 30-150, 150-180",
            hinge_range: "0-180",
            hinge_default: 180,
        }),
    },
    Profile {
        id: "flip",
        class: "Flip foldable",
        diagonal_in: 6.7,
        width: 1080,
        height: 2636,
        density: 440,
        ram_mb: 4096,
        data_gb: 6,
        default: false,
        fold: Some(Fold {
            kind: FoldKind::Flip,
            hinge_type: 0,
            hinge_area: "0-1318-1080-1",
            folded_region: None,
            postures: BOOK,
            posture_angles: "0-30, 30-150, 150-180",
            hinge_range: "0-180",
            hinge_default: 180,
        }),
    },
    Profile {
        id: "tablet",
        class: "Small tablet",
        diagonal_in: 11.0,
        width: 1600,
        height: 2560,
        density: 280,
        ram_mb: 4096,
        data_gb: 8,
        default: false,
        fold: None,
    },
];

#[must_use]
pub fn find(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.id == id)
}

#[must_use]
pub fn default_profile() -> &'static Profile {
    PROFILES.iter().find(|p| p.default).unwrap_or(&PROFILES[0])
}

/// Catalogue rules (§12): unique ids, no duplicate resolution + density, exactly one default,
/// foldables declare postures that match their angle ranges.
#[must_use]
pub fn validate(profiles: &[Profile]) -> Vec<String> {
    let mut e = Vec::new();
    for (i, a) in profiles.iter().enumerate() {
        for b in &profiles[i + 1..] {
            if a.id == b.id {
                e.push(format!("duplicate id {}", a.id));
            }
            if (a.width, a.height, a.density) == (b.width, b.height, b.density) {
                e.push(format!(
                    "{} and {} have the same resolution and density",
                    a.id, b.id
                ));
            }
        }
        if let Some(f) = &a.fold {
            if f.posture_angles.split(',').count() != f.postures.len() {
                e.push(format!("{}: one angle range per posture needed", a.id));
            }
        }
        if a.ram_mb < 4096 {
            e.push(format!("{}: guest RAM below 4 GB (DEC-006)", a.id));
        }
    }
    if profiles.iter().filter(|p| p.default).count() != 1 {
        e.push("exactly one default profile needed".into());
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_follows_the_rules() {
        assert_eq!(validate(PROFILES), Vec::<String>::new());
        assert_eq!(PROFILES.len(), 6);
        assert_eq!(default_profile().id, "standard");
        assert!(find("fold")
            .unwrap()
            .fold
            .as_ref()
            .unwrap()
            .folded_region
            .is_some());
        assert!(find("nope").is_none());
        let mut dup = PROFILES.to_vec();
        dup[1].width = 1080;
        dup[1].height = 2340; // same as compact
        dup[0].default = true;
        let errs = validate(&dup);
        assert!(
            errs.iter().any(|e| e.contains("same resolution")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("exactly one default")),
            "{errs:?}"
        );
    }

    #[test]
    fn postures_map_to_emulator_values() {
        assert_eq!(Posture::Closed.index(), 1);
        assert_eq!(Posture::HalfOpened.index(), 2);
        assert_eq!(Posture::Opened.index(), 3);
        assert_eq!(Posture::parse("tabletop"), Some(Posture::HalfOpened));
        assert_eq!(Posture::parse("unfolded"), Some(Posture::Opened));
        assert_eq!(Posture::parse("sideways"), None);
    }
}
