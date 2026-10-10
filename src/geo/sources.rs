//! Where imagery and heights come from: tile servers given as URL templates (the built-in
//! ones and any the user adds in the preferences), some with the user's own key.

use serde::{Deserialize, Serialize};

use crate::i18n::tr;

/// How a source's tiles are fetched and read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Kind {
    /// Pictures at `{z}/{x}/{y}` (JPEG or PNG; WMTS in the Web Mercator tile grid too).
    #[default]
    Imagery,
    /// A WMS (or ArcGIS export): each Web Mercator tile asked for by its box, `{bbox}` in
    /// EPSG:3857 metres and `{width}` × `{height}` pixels.
    Wms,
    /// Google's Map Tiles API: a session is made first, its token goes into each request.
    Google,
    /// Heights as Terrarium PNGs (R * 256 + G + B / 256 - 32768 m).
    Terrarium,
}

/// One tile server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    /// `{z}`, `{x}`, `{y}`, `{key}` (and `{bbox}`, `{width}`, `{height}` for a WMS) are
    /// filled in.
    pub url: String,
    pub attribution: String,
    /// The finest zoom asked for.
    pub max_zoom: u8,
    #[serde(default)]
    pub kind: Kind,
    /// Works only with the user's own key.
    #[serde(default)]
    pub needs_key: bool,
    /// Where to get a key, or the source's terms.
    #[serde(default)]
    pub key_link: String,
    /// Where it has imagery: west, south, east, north (degrees; None: the whole world).
    #[serde(default)]
    pub bounds: Option<[f64; 4]>,
    /// Its pictures' own ground resolution (m a pixel; 0: as fine as its zoom says). A
    /// WMS draws any zoom, finer than this is only enlarged.
    #[serde(default)]
    pub resolution: f64,
    /// Pixels a side of each tile asked for (256; a WMS gives 512 at once).
    #[serde(default = "px256")]
    pub tile_px: u32,
    /// An official orthophoto of a country or state (preferred where it has the place).
    #[serde(default)]
    pub ortho: bool,
    /// Part of openMapper (not removable).
    #[serde(skip)]
    pub builtin: bool,
}

fn px256() -> u32 {
    256
}

pub const ESRI: &str = "Esri World Imagery";
pub const GOOGLE: &str = "Google satellite";
pub const TERRARIUM: &str = "AWS Terrain Tiles";
/// The cache folder of Google's sessions.
pub const GOOGLE_SESSION_DIR: &str = "google-sessions";

fn base(name: &str, url: &str, attribution: &str, max_zoom: u8) -> Source {
    Source { name: name.into(), url: url.into(), attribution: attribution.into(), max_zoom, kind: Kind::Imagery, needs_key: false, key_link: String::new(), bounds: None, resolution: 0.0, tile_px: 256, ortho: false, builtin: true }
}

/// An official orthophoto (each endpoint was tried: it answers without a key).
fn ortho(name: &str, url: &str, attribution: &str, terms: &str, bounds: [f64; 4], resolution: f64, wms: bool) -> Source {
    Source {
        kind: if wms { Kind::Wms } else { Kind::Imagery },
        key_link: terms.into(),
        bounds: Some(bounds),
        resolution,
        tile_px: if wms { 512 } else { 256 },
        ortho: true,
        ..base(name, url, attribution, if wms { 20 } else { 21 })
    }
}

fn wms(base_url: &str, layer: &str) -> String {
    format!("{base_url}?SERVICE=WMS&VERSION=1.1.1&REQUEST=GetMap&LAYERS={layer}&STYLES=&SRS=EPSG:3857&BBOX={{bbox}}&WIDTH={{width}}&HEIGHT={{height}}&FORMAT=image/jpeg")
}

/// The sources openMapper knows by itself.
pub fn builtin() -> Vec<Source> {
    let keyed = |name: &str, url: &str, attribution: &str, max_zoom: u8, kind: Kind, link: &str, tile_px: u32| Source { kind, needs_key: true, key_link: link.into(), tile_px, ..base(name, url, attribution, max_zoom) };
    vec![
        // (blankTile=false: above its coverage the server answers 404, not a grey picture)
        base(ESRI, "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}?blankTile=false", "Imagery © Esri, Maxar, Earthstar Geographics, and the GIS User Community", 21),
        keyed(GOOGLE, "https://tile.googleapis.com/v1/2dtiles/{z}/{x}/{y}?session={session}&key={key}", "Google", 22, Kind::Google, "https://console.cloud.google.com/apis/library/tile.googleapis.com", 256),
        keyed("Mapbox Satellite", "https://api.mapbox.com/v4/mapbox.satellite/{z}/{x}/{y}@2x.jpg90?access_token={key}", "© Mapbox © OpenStreetMap contributors © Maxar", 21, Kind::Imagery, "https://account.mapbox.com/access-tokens/", 512),
        keyed("MapTiler Satellite", "https://api.maptiler.com/tiles/satellite-v2/{z}/{x}/{y}.jpg?key={key}", "© MapTiler © OpenStreetMap contributors", 20, Kind::Imagery, "https://cloud.maptiler.com/account/keys/", 256),
        ortho("Berlin TrueDOP 2024", &wms("https://gdi.berlin.de/services/wms/truedop_2024", "truedop_2024"), "© Geoportal Berlin, TrueDOP 2024, dl-de/zero-2.0", "https://www.govdata.de/dl-de/zero-2-0", [13.08, 52.33, 13.77, 52.68], 0.2, true),
        ortho("Brandenburg DOP20", &wms("https://isk.geobasis-bb.de/mapproxy/dop20c/service/wms", "bebb_dop20c"), "© GeoBasis-DE/LGB, dl-de/by-2-0", "https://www.govdata.de/dl-de/by-2-0", [11.26, 51.35, 14.77, 53.56], 0.2, true),
        ortho("NRW DOP10", &wms("https://www.wms.nrw.de/geobasis/wms_nw_dop", "nw_dop_rgb"), "© Geobasis NRW, dl-de/zero-2.0", "https://www.govdata.de/dl-de/zero-2-0", [5.86, 50.32, 9.47, 52.53], 0.1, true),
        ortho("Bayern DOP40", &wms("https://geoservices.bayern.de/od/wms/dop/v1/dop40", "by_dop40c"), "© Bayerische Vermessungsverwaltung, CC BY 4.0", "https://creativecommons.org/licenses/by/4.0/", [8.97, 47.27, 13.84, 50.57], 0.4, true),
        ortho("Niedersachsen DOP20", &wms("https://opendata.lgln.niedersachsen.de/doorman/noauth/dop_wms", "ni_dop20"), "LGLN (2025), CC BY 4.0", "https://creativecommons.org/licenses/by/4.0/", [6.65, 51.29, 11.6, 53.9], 0.2, true),
        ortho("PDOK Luchtfoto (NL)", "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg", "Luchtfoto © Beeldmateriaal Nederland / PDOK, CC BY 4.0", "https://www.pdok.nl/", [3.2, 50.75, 7.22, 53.7], 0.08, false),
        ortho("swisstopo SWISSIMAGE", "https://wmts.geo.admin.ch/1.0.0/ch.swisstopo.swissimage/default/current/3857/{z}/{x}/{y}.jpeg", "© swisstopo", "https://www.geo.admin.ch/en/general-terms-of-use-fsdi", [5.95, 45.82, 10.49, 47.81], 0.1, false),
        ortho("basemap.at Orthofoto", "https://maps.wien.gv.at/basemap/bmaporthofoto30cm/normal/google3857/{z}/{y}/{x}.jpeg", "Datenquelle: basemap.at, CC BY 4.0", "https://basemap.at/", [9.53, 46.37, 17.16, 49.02], 0.29, false),
        ortho("IGN Ortho (France)", "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=ORTHOIMAGERY.ORTHOPHOTOS&STYLE=normal&TILEMATRIXSET=PM&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}&FORMAT=image/jpeg", "© IGN, Licence Ouverte Etalab 2.0", "https://www.etalab.gouv.fr/licence-ouverte-open-licence/", [-5.2, 41.3, 9.6, 51.1], 0.2, false),
        ortho("New York State Orthos", "https://orthos.its.ny.gov/arcgis/rest/services/wms/Latest/MapServer/export?bbox={bbox}&bboxSR=3857&imageSR=3857&size={width},{height}&format=jpg&f=image", "NYS ITS Geospatial Services", "https://gis.ny.gov/orthoimagery", [-79.77, 40.49, -71.85, 45.02], 0.15, true),
        Source { kind: Kind::Terrarium, key_link: "https://github.com/tilezen/joerd/blob/master/docs/attribution.md".into(), ..base(TERRARIUM, "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png", "Terrain: Mapzen Terrain Tiles on AWS Open Data (SRTM, USGS 3DEP, GMTED2010, ETOPO1, Copernicus EU-DEM and others)", 15) },
    ]
}

/// The user's keys and own sources (part of openMapper's preferences, kept only on this
/// computer).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoPrefs {
    /// Sources added by the user.
    #[serde(default)]
    pub sources: Vec<Source>,
    /// Keys by source name.
    #[serde(default)]
    pub keys: std::collections::BTreeMap<String, String>,
}

impl GeoPrefs {
    /// Every source: the built-in ones, then the user's.
    pub fn all(&self) -> Vec<Source> {
        let mut v = builtin();
        v.extend(self.sources.iter().filter(|s| !v.iter().any(|b| b.name == s.name)).cloned().collect::<Vec<_>>());
        v
    }

    pub fn key(&self, name: &str) -> Option<&str> {
        self.keys.get(name).map(|k| k.trim()).filter(|k| !k.is_empty())
    }

    /// The source can be used (it has a key if it needs one).
    pub fn usable(&self, s: &Source) -> bool {
        !s.needs_key || self.key(&s.name).is_some()
    }
}

impl Source {
    /// The request for one tile (`session`: Google's token).
    pub fn tile_url(&self, z: u8, x: u32, y: u32, key: Option<&str>, session: Option<&str>) -> String {
        let r = 20_037_508.342_789_244;
        let s = 2.0 * r / (1u64 << z) as f64;
        let bbox = format!("{:.3},{:.3},{:.3},{:.3}", -r + x as f64 * s, r - (y as f64 + 1.0) * s, -r + (x as f64 + 1.0) * s, r - y as f64 * s);
        let px = self.tile_px.to_string();
        self.url
            .replace("{z}", &z.to_string())
            .replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string())
            .replace("{bbox}", &bbox)
            .replace("{width}", &px)
            .replace("{height}", &px)
            .replace("{key}", key.unwrap_or(""))
            .replace("{session}", session.unwrap_or(""))
    }

    /// It may have imagery at (lat, lon).
    pub fn covers(&self, lat: f64, lon: f64) -> bool {
        self.bounds.map(|[w, s, e, n]| lon >= w && lon <= e && lat >= s && lat <= n).unwrap_or(true)
    }

    /// The finest zoom worth asking for at a latitude: its `max_zoom`, or for a source of
    /// a known resolution the zoom that just gives it (a finer one is only enlarged).
    pub fn useful_zoom(&self, lat: f64) -> u8 {
        if self.resolution <= 0.0 {
            return self.max_zoom;
        }
        let at0 = super::metres_per_pixel(lat, 0) * 256.0 / self.tile_px as f64;
        ((at0 / self.resolution).log2() + 0.7).floor().clamp(1.0, self.max_zoom as f64) as u8
    }

    /// Ground metres a pixel of its tiles of zoom `z` at a latitude (no finer than its own
    /// resolution).
    pub fn metres_per_pixel(&self, lat: f64, z: u8) -> f64 {
        (super::metres_per_pixel(lat, z) * 256.0 / self.tile_px as f64).max(self.resolution)
    }

    /// A folder name for the disk cache (no key in it).
    pub fn slug(&self) -> String {
        let s: String = self.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
        if s.is_empty() {
            "source".into()
        } else {
            s
        }
    }

    pub fn is_terrain(&self) -> bool {
        self.kind == Kind::Terrarium
    }

    /// A short note for the source list.
    pub fn note(&self) -> String {
        match (self.kind, self.needs_key) {
            (Kind::Terrarium, _) => tr("terrain heights, no key needed", "высоты рельефа, ключ не нужен").into(),
            (_, true) => tr("needs your own free key", "нужен ваш собственный бесплатный ключ").into(),
            _ => tr("no key needed", "ключ не нужен").into(),
        }
    }
}

/// A Google Map Tiles API session (`createSession`'s answer).
#[derive(Debug, Clone, PartialEq)]
pub struct GoogleSession {
    pub token: String,
    /// Seconds since 1970 when it runs out.
    pub expiry: u64,
}

/// Read `createSession`'s answer: `{"session": "...", "expiry": "1690000000", ...}`.
pub fn parse_google_session(body: &str) -> Result<GoogleSession, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if let Some(e) = v.get("error") {
        return Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("error").to_string());
    }
    let token = v.get("session").and_then(|s| s.as_str()).ok_or("no session in the answer")?.to_string();
    let expiry = match v.get("expiry") {
        Some(serde_json::Value::String(s)) => s.parse().map_err(|_| "a bad expiry")?,
        Some(serde_json::Value::Number(n)) => n.as_u64().ok_or("a bad expiry")?,
        _ => return Err("no expiry in the answer".into()),
    };
    Ok(GoogleSession { token, expiry })
}

/// What Google's viewport information says about an area: the copyright line to show and
/// the finest zoom it has there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Viewport {
    pub copyright: String,
    pub max_zoom: Option<u8>,
}

/// Read the viewport answer: `{"copyright": "...", "maxZoomRects": [{"maxZoom": 19, "north": ..}]}`.
/// The finest zoom is that of the rectangles holding the point (lat, lon).
pub fn parse_google_viewport(body: &str, lat: f64, lon: f64) -> Result<Viewport, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if let Some(e) = v.get("error") {
        return Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("error").to_string());
    }
    let copyright = v.get("copyright").and_then(|c| c.as_str()).unwrap_or("").to_string();
    let mut max_zoom: Option<u8> = None;
    for r in v.get("maxZoomRects").and_then(|r| r.as_array()).into_iter().flatten() {
        let f = |k: &str| r.get(k).and_then(|x| x.as_f64());
        let (Some(z), Some(n), Some(s), Some(e), Some(w)) = (f("maxZoom"), f("north"), f("south"), f("east"), f("west")) else { continue };
        if lat <= n && lat >= s && lon <= e && lon >= w {
            max_zoom = Some(max_zoom.unwrap_or(0).max(z as u8));
        }
    }
    Ok(Viewport { copyright, max_zoom })
}
