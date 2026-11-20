//! The look of the plots, chosen in Settings: the same colours on screen and in saved files.

use std::sync::RwLock;

pub type Rgb = [u8; 3];

pub const PALETTE_LEN: usize = 10;

/// A curve or band that always keeps its own colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Temperature,
    Incident,
    Transmitted,
    Diffuse,
    Total,
    SavedLines,
    HPlus,
    H0,
    H2,
    CoC,
    HePlus,
    RegionHii,
    RegionPdr,
    RegionMol,
    FrontIf,
    FrontDf,
    FrontCo,
}

pub const ROLE_COUNT: usize = 17;

/// Each role: the group it is listed in, its name, its key in the settings file.
pub const ROLES: [(Role, &str, &str, &str); ROLE_COUNT] = [
    (Role::Total, "Spectrum (SED)", "Total", "total"),
    (Role::Incident, "Spectrum (SED)", "Incident", "incident"),
    (Role::Transmitted, "Spectrum (SED)", "Transmitted", "transmitted"),
    (Role::Diffuse, "Spectrum (SED)", "Diffuse", "diffuse"),
    (Role::SavedLines, "Spectrum (SED)", "Saved lines", "saved_lines"),
    (Role::Temperature, "Structure", "Temperature", "temperature"),
    (Role::HPlus, "Structure", "H⁺", "hplus"),
    (Role::H0, "Structure", "H⁰", "h0"),
    (Role::H2, "Structure", "2H₂/H", "h2"),
    (Role::CoC, "Structure", "CO/C", "co"),
    (Role::HePlus, "Structure", "He⁺", "heplus"),
    (Role::RegionHii, "Regions and fronts", "H II region", "region_hii"),
    (Role::RegionPdr, "Regions and fronts", "PDR", "region_pdr"),
    (Role::RegionMol, "Regions and fronts", "Molecular region", "region_mol"),
    (Role::FrontIf, "Regions and fronts", "Ionisation front", "front_if"),
    (Role::FrontDf, "Regions and fronts", "Dissociation front", "front_df"),
    (Role::FrontCo, "Regions and fronts", "C/CO transition", "front_co"),
];

pub const ROLE_GROUPS: [&str; 3] = ["Spectrum (SED)", "Structure", "Regions and fronts"];

/// The colour of each role, in the order of `ROLES`, taken from a list of curve colours.
const fn from_palette(p: &[Rgb; PALETTE_LEN]) -> [Rgb; ROLE_COUNT] {
    [p[2], p[6], p[0], p[1], p[3], p[1], p[0], p[1], p[2], p[3], p[5], [90, 170, 230], [235, 175, 70], [165, 140, 215], [30, 110, 200], [210, 110, 20], [120, 90, 190]]
}

const INCLOUD: [Rgb; PALETTE_LEN] = [[31, 119, 180], [221, 90, 30], [40, 160, 90], [200, 80, 150], [225, 160, 20], [60, 170, 220], [110, 110, 120], [120, 90, 200], [120, 170, 40], [200, 60, 80]];

/// Ready-made sets of colours.
pub const PRESETS: [(&str, [Rgb; PALETTE_LEN]); 5] = [
    ("InCloud", INCLOUD),
    ("Colour-blind safe", [[0, 114, 178], [213, 94, 0], [0, 158, 115], [204, 121, 167], [230, 159, 0], [86, 180, 233], [120, 120, 120], [170, 68, 153], [153, 153, 51], [136, 34, 85]]),
    ("Classic", [[31, 119, 180], [255, 127, 14], [44, 160, 44], [214, 39, 40], [148, 103, 189], [140, 86, 75], [127, 127, 127], [227, 119, 194], [188, 189, 34], [23, 190, 207]]),
    ("Soft", [[92, 141, 186], [214, 126, 96], [111, 170, 130], [178, 123, 168], [212, 175, 96], [120, 180, 200], [140, 140, 148], [142, 128, 190], [158, 178, 106], [196, 112, 122]]),
    ("Viridis", [[68, 1, 84], [253, 231, 37], [33, 145, 140], [72, 40, 120], [94, 201, 98], [49, 104, 142], [120, 120, 120], [180, 222, 44], [62, 74, 137], [31, 158, 137]]),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Theme {
    /// colours of the curves that have no colour of their own, in order
    pub palette: [Rgb; PALETTE_LEN],
    pub roles: [Rgb; ROLE_COUNT],
    /// thickness of the curves, 1 = normal
    pub width: f32,
    /// opacity of the shaded regions
    pub band: f32,
    /// a saved line is drawn on the SED if it adds this share to the continuum below it
    pub line_min: f64,
    /// write the names of the saved lines on the SED
    pub line_names: bool,
    /// background of the plots: 0 same as the app, 1 white, 2 black, 3 `bg`
    pub bg_mode: u8,
    pub bg: Rgb,
    /// saved figures get the same background (otherwise they are on white paper)
    pub export_bg: bool,
    /// fonts: 0 InCloud's own, 1 DejaVu Sans, 2 DejaVu Serif
    pub ui_font: u8,
    /// size of the interface text, 1 = normal
    pub ui_text: f32,
    pub plot_font: u8,
    /// size of the text of the plots on screen, 1 = normal
    pub plot_text: f32,
}

pub const FONTS: [&str; 3] = ["InCloud (Ubuntu)", "DejaVu Sans", "DejaVu Serif"];

impl Theme {
    pub const DEFAULT: Theme = Theme { palette: INCLOUD, roles: from_palette(&INCLOUD), width: 1.0, band: 0.12, line_min: 0.1, line_names: true, bg_mode: 0, bg: [255, 255, 255], export_bg: false, ui_font: 0, ui_text: 1.0, plot_font: 0, plot_text: 1.0 };

    /// The background chosen for the plots, if it isn't the app's own.
    pub fn plot_bg(&self) -> Option<Rgb> {
        match self.bg_mode {
            1 => Some([255, 255, 255]),
            2 => Some([0, 0, 0]),
            3 => Some(self.bg),
            _ => None,
        }
    }

    /// Take the colours of a ready-made set.
    pub fn use_palette(&mut self, p: [Rgb; PALETTE_LEN]) {
        self.palette = p;
        self.roles = from_palette(&p);
    }

    pub fn colour(&self, r: Role) -> Rgb {
        self.roles[index(r)]
    }

    pub fn colour_mut(&mut self, r: Role) -> &mut Rgb {
        &mut self.roles[index(r)]
    }

    /// The lines of the settings file.
    pub fn to_text(&self) -> String {
        let list: Vec<String> = self.palette.iter().map(|c| hex(*c)).collect();
        let mut s = format!("plot_palette={}\nplot_width={}\nplot_band={}\nplot_line_min={}\nplot_line_names={}\nplot_bg_mode={}\nplot_bg={}\nplot_export_bg={}\nui_font={}\nui_text={}\nplot_font={}\nplot_text={}\n", list.join(","), self.width, self.band, self.line_min, self.line_names, self.bg_mode, hex(self.bg), self.export_bg, self.ui_font, self.ui_text, self.plot_font, self.plot_text);
        for (i, (_, _, _, key)) in ROLES.iter().enumerate() {
            s.push_str(&format!("plot_colour_{key}={}\n", hex(self.roles[i])));
        }
        s
    }

    /// Read one line of the settings file.
    pub fn read(&mut self, key: &str, value: &str) {
        match key {
            "plot_palette" => {
                for (i, c) in value.split(',').filter_map(parse_hex).take(PALETTE_LEN).enumerate() {
                    self.palette[i] = c;
                }
            }
            "plot_width" => self.width = value.parse().unwrap_or(self.width).clamp(0.4, 3.0),
            "plot_band" => self.band = value.parse().unwrap_or(self.band).clamp(0.0, 0.6),
            "plot_line_min" => self.line_min = value.parse().unwrap_or(self.line_min).clamp(0.0, 10.0),
            "plot_line_names" => self.line_names = value == "true",
            "plot_bg_mode" => self.bg_mode = value.parse::<u8>().unwrap_or(0).min(3),
            "plot_bg" => self.bg = parse_hex(value).unwrap_or(self.bg),
            "plot_export_bg" => self.export_bg = value == "true",
            "ui_font" => self.ui_font = value.parse::<u8>().unwrap_or(0).min(2),
            "ui_text" => self.ui_text = value.parse().unwrap_or(1.0f32).clamp(0.8, 1.5),
            "plot_font" => self.plot_font = value.parse::<u8>().unwrap_or(0).min(2),
            "plot_text" => self.plot_text = value.parse().unwrap_or(1.0f32).clamp(0.7, 1.8),
            k => {
                let Some(name) = k.strip_prefix("plot_colour_") else { return };
                if let (Some(i), Some(c)) = (ROLES.iter().position(|r| r.3 == name), parse_hex(value)) {
                    self.roles[i] = c;
                }
            }
        }
    }
}

fn index(r: Role) -> usize {
    ROLES.iter().position(|x| x.0 == r).unwrap_or(0)
}

static THEME: RwLock<Theme> = RwLock::new(Theme::DEFAULT);

pub fn get() -> Theme {
    THEME.read().map(|t| *t).unwrap_or(Theme::DEFAULT)
}

pub fn set(t: Theme) {
    if let Ok(mut g) = THEME.write() {
        *g = t;
    }
}

/// The colour of the curve number `i`.
pub fn pal(i: usize) -> Rgb {
    get().palette[i % PALETTE_LEN]
}

pub fn role(r: Role) -> Rgb {
    get().colour(r)
}

pub fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<Rgb> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    Some([u8::from_str_radix(&t[0..2], 16).ok()?, u8::from_str_radix(&t[2..4], 16).ok()?, u8::from_str_radix(&t[4..6], 16).ok()?])
}
