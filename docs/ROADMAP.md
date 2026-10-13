# openMapper: road to 1.0

openMapper 0.1 already opens, edits and saves OMSI 2 maps (roads with automatic junctions,
objects, terrain, painting, tiles, game logic, import, test drive). What keeps it from being
a complete editor, and who works on what.

## Workstreams

Each stream has its own branch and owns its files. Shared files (`src/ui.rs`,
`src/editor.rs`, `src/app.rs`, `src/main.rs`, `src/i18n.rs`, `src/icons.rs`) are touched
only with small hooks: a new menu item, a new `Action`, a call into the stream's module. Big
pieces of code go into new files.

| Stream | Branch | Owns | Goal |
| --- | --- | --- | --- |
| A. Timetable | `feature/timetable` | `src/timetable/` (new), `src/ui_timetable.rs` (new) | Lines, trips and tours (`TTData`): stops picked from the map, the route found along the splines between them, times; written in OMSI 2's formats so the map can be driven in service, not only freely |
| B. Editing | `feature/editing` | `src/editor.rs` (selection, drag, spline edit), `src/clipboard.rs` (new) | Multi-select (Shift-click, box), copy / paste / duplicate, numeric position and height, moving road ends and reshaping a road after it is drawn, snapping |
| C. Quality | `feature/quality` | `src/render/`, `src/doc/`, `src/check.rs` (new) | Map check (missing files, unlinked splines, stops off a path, start points off a road), speed on big maps (Berlin-Spandau), safe saving (backup, atomic writes), tests |
| D. Integration | `main` | `README.md`, `docs/`, merging | Merges the streams, keeps the README true, overview map, release polish |

## Rules for everyone

* Every user-facing text goes through `tr("English", "Русский")`.
* Follow the code around you: short comments that say why, no dead code.
* Commit often on your branch with plain messages. **No AI mentions anywhere**: no
  `Co-Authored-By` trailers, no "Generated with", nothing in code comments.
* `cargo build --release` and `cargo test --release` pass before every commit.
* Never write into the OMSI 2 installation; maps and generated content go into openOMSI's
  content folder.
* Check visually with `OPENMAPPER_SHOT=out.png ./target/release/openmapper path/to/global.cfg`.
* When you add a shared hook (new `Action`, new menu entry), note it in the log below so the
  merge is easy.

## Hooks log

Add a line when you touch a shared file: stream, file, what.
* D · `src/ui.rs`: `Action::LookAt(DVec2)`, `UiState::minimap`, `Status::cam`, View menu "Overview map", minimap drawn in the central panel; welcome list maps a recent installation map to its content copy.
* D · `src/app.rs`: `Status::cam` filled, `Action::LookAt` handled, key `M` toggles the overview map.
* D · `src/main.rs`: `mod ui_minimap`.
* D · `src/ui.rs`: `Action::ToggleFavourite`, `Action::UsedAsset`, `UiState::{favourites, recent_assets}`; `catalogue`, `logic_panel`, `catalogue_of` take `actions`; favourites / recently used groups and a right-click menu in the catalogue.
* D · `src/install.rs`: `Prefs::{favourites, recent_assets}`.
* D · `src/app.rs`: `OPENMAPPER_TOOL` also takes `roads` and `objects`.
* D · `src/ui.rs`: `Action::MapPicture`, `Action::SetMapCamera`; `map_properties` takes `st` and `actions` and shows the map's picture.
* D · `src/app.rs`: `read_frame` (split from `save_shot`), `save_map_picture`, `[mapcam]` from the camera.
* D · `src/ui.rs`: `Action::SetScale`, "Interface size" in `lang_menu`; `src/install.rs`: `Prefs::ui_scale`; `src/app.rs`: zoom applied at start and on change.
* B · `src/ui.rs`: `Action::Copy / Cut / Paste / Duplicate / SelectAll` and Edit menu entries; `check`, `section`, `prop_row`, `TEXT_DIM` made `pub(crate)` for `ui_edit.rs`; properties: several selected go to `ui_edit::group_properties`, object X / Y are numbers (`Editor::put_object`), a spline's ends via `ui_edit::spline_ends` / `apply_ends` (the old Gradient row went); status bar: `ui_edit::snap_controls`; Select hint and Controls rows.
* B · `src/app.rs`: Ctrl+C / X / V / D / A push the new actions and they are handled; Shift / Ctrl + left press on empty ground starts `ed.select_box`, cursor moves update it, release calls `ed.finish_box`; `ui_edit::paint_box` after `ui::editor`; undo / redo use `ed.select(None)`.
* B · `src/main.rs`: `mod clipboard`, `mod reshape`, `mod selection`, `mod ui_edit`.
* B · `src/render/mod.rs`: `FrameExtras::selected_more` (a `HashSet<Sel>`), tinted like `selected` and part of the scene key.
* B · `src/doc/mod.rs`: `Sel` derives `Hash`.
* B · `Editor::selected` stays the one shown in the properties; the rest of a selection is `Editor::also` (meaningless while `selected` is None). Use `ed.select(..)` / `ed.selection()` rather than writing `selected` when it matters.
* A · `src/main.rs`: `mod timetable`.
* A · `src/doc/mod.rs`: `Document::timetable` (read with the map in `load`, empty in `new_map`), counted in `is_dirty`.
* A · `src/doc/save.rs`: `crate::timetable::save(doc)` after `global.cfg` (writes only the changed TTData files).
* A · `src/main.rs`: `mod ui_timetable`.
* A · `src/ui.rs`: `Action::Timetable`; Map menu "Lines and timetable…" (key `L`).
* A · `src/app.rs`: `App::tt` (the Lines window): drawn after `ui::editor`, its route laid into the frame's extras after `Editor::extras`, `before_save` in `App::save`, `Action::Timetable` and key `L` toggle it; `OPENMAPPER_TIMETABLE=<line>[:tours]` opens it on a line, and with `OPENMAPPER_DEMO` adds stops and a line to the demo map (checks without a person at the screen); `Action::Force` also drops the timetable's changes.
* C · `src/main.rs`: `mod check;` and `openmapper --check path/to/global.cfg` (checks a map without a window).
* C · `src/ui.rs`: `UiState::check` (the map check window), Map > Check map…, the window drawn at the end of `editor()`.
* C · `src/app.rs`: `OPENMAPPER_PROFILE` timings round the editor, the interface, acquiring the frame and submitting it; the camera turning by itself with `OPENMAPPER_PROFILE=orbit`; screenshot and profiling runs go on when the window gets no redraws; `OPENMAPPER_CHECK=1` opens the map check.
* C · `src/gpu.rs`: no vsync while profiling.
* C · `src/ui.rs`: `Action::RestoreBackup(PathBuf)`, File > Earlier versions… (`UiState::versions_open`, the window is `doc::safe::versions_window`).
* C · `src/app.rs`: autosave (`App::autosave`, ticked after the frame's actions, not while the left button is down or an edit is open), the offer at start (`App::recover`, `doc::autosave::offer`), the autosave cleared on save, close, opening or making another map and a normal quit; `Action::RestoreBackup` handled (guarded like Open); the save error says the changes are still there.
* C · `src/timetable/tests.rs`: `one_map()` takes the crate-wide `doc::one_map()` lock (the grid is global: Spandau's tests and the round trips of other maps must not run at once).
* A · `src/editor.rs`: `Editor::{pick_mode, picked}`: with pick mode on, a left click with the Select tool only notes what is under the cursor and where (the Lines window picks stops so: nothing is selected or dragged).
* A · `src/history.rs`: `Step::{timetable, at}`, `Snapshot` and `History::push_snapshot` (a window's edit given as the state before it; edits of one kind within 1.5 s are one step); `apply` swaps the timetable too (`Timetable::restored` has every file written again).
* A · `src/main.rs`: `mod traffic`, `mod ui_traffic`.
* A · `src/doc/mod.rs`: `Document::traffic` (`ailists.cfg`, unscheduled traffic, parked cars, people; read in `load`, from the template in `new_map`), counted in `is_dirty`.
* A · `src/doc/save.rs`: `crate::traffic::save(doc)` after the timetable's (only the changed files).
* A · `src/selection.rs` (tests' `flat_map`): `traffic: Default::default()`.
* A · `src/history.rs`: `Step::traffic`, `Snapshot::traffic`.
* A · `src/ui.rs`: `Action::Traffic`; Map menu "Traffic and AI…" (key `T`).
* A · `src/app.rs`: `App::traffic` (the Traffic and AI window) drawn after the Lines window, `Action::Traffic` and key `T`; `OPENMAPPER_TRAFFIC=day|groups|lists` opens it on a tab; `Action::Force` drops the traffic's changes too.
* B (round 2) · `src/app.rs`: `U` toggles the ruler; the Select tool's empty-ground press goes to the editor while the ruler is on; `ed.paste(&self.lib)`.
* B (round 2) · `src/ui.rs`: the properties panel is in a `ScrollArea`; an object hanging on another (`[attachObj]`) is shown through `selection::any_object` / `any_object_mut` (no X / Y, a "Hangs on" row); a road's properties end with `ui_edit::rows_section`; Controls row for `U`.
* B (round 2) · `src/main.rs`: `mod rows`.
* A · `src/main.rs`: `mod sample`, `mod ui_guide`, `mod ui_newmap`.
* A · `src/ui.rs`: the New map dialog moved to `src/ui_newmap.rs` (`UiState::new_map` is a `NewMapForm`; `Action::NewMap` carries `options`), `Action::Guide` and Help menu "Getting started".
* A · `src/install.rs`: `Prefs::guide_off`.
* A · `src/app.rs`: `new_map` takes the options (base ground, left-hand traffic, sample → `App::sample`), the New map dialog drawn from here (both screens), `App::guide` window and its "Show me" requests, `Action::Guide`, Play ticks the guide's test drive; `OPENMAPPER_DEMO=SAMPLE:<name>` (with `OPENMAPPER_LHT` keeping left) makes a sample map and saves it, `OPENMAPPER_NEWMAP` opens the dialog, `OPENMAPPER_GUIDE` the guide.
* A · `src/ui_timetable.rs`: `add_depot` is `pub(crate)` (the sample uses it).
* A · `src/doc/save.rs`: the timetable's and the traffic's files are planned as `safe::Op`s (`timetable::plan`, `traffic::plan`) and committed with the map's own, all or nothing (`saved` marks them done afterwards); the separate `extra` backup list for them is gone (their ops are backed up like the rest).
* B (round 3) · `src/ui.rs`: `Action::BuildRoad`; the Roads panel calls `ui_edit::road_options` (keep height).
* B (round 3) · `src/app.rs`: Enter pushes `Action::BuildRoad` (handled with `ed.confirm_road`); Shift sets `ed.straight_lock` and `ed.replan`; `ui_edit::road_entry` drawn after `ui::editor`.
* B (round 3) · `src/main.rs`: `mod draw`.
* B (round 3) · `src/main.rs`: `mod roundabout`. Roads panel options (roundabout, keep height) come from `ui_edit::road_options`.
* A · `src/main.rs`: `mod export`; `--export path/to/global.cfg out.zip` and `--import file` run without a window.
* A · `src/ui.rs`: `Action::Export`, File menu "Export map for sharing…".
* A · `src/app.rs`: `App::export` (saves, asks where, packs; toasts what went in and what is missing).
* A · `src/check.rs` (C): `missing_textures` is `pub(crate)` (the export test checks the imported map with it).
* A · `src/doc/mod.rs`, `src/doc/save.rs` (C): `Document::descriptions` (`global_<LANG>.dsc`, new `src/descriptions.rs`), read with the map, counted in `is_dirty`, planned with the save's ops; `descriptions: Default::default()` in the tests' and autosave's `Document` literals (`src/doc/tests.rs`, `src/doc/autosave.rs`, `src/check.rs`, `src/selection.rs`).
* A · `src/ui.rs`: `map_properties` shows `descriptions::editor` (a language picker above the description) in place of the plain description field.
* A · `src/history.rs`: `Snapshot::descriptions`.
* A · `src/main.rs`: `mod descriptions`.
* A · (no new shared hooks for Holidays.txt: it is part of the timetable, `Timetable::calendar`.)
* B (round 4) · `src/junction.rs`: `Options` (lights, greens, crossings; kept as a comment line in the junction's `.sco`), `make_with` (light program, `[use_traffic_light]` on the first piece of each way in, signal heads and poles with `[varparent]`, zebra stripes and footways), `children`, `remove_unused`; `make` calls `make_with` with no options.
* B (round 4) · `src/ui.rs`: an object's properties end with `ui_edit::junction_section` (for openMapper's junctions).
* B (round 4) · `src/app.rs`: after a successful save, `ed.clean_junction_files()` deletes this map's junction files no tile uses that were made before this session.

* C · `src/install.rs`: `Prefs::save` writes through `doc::safe::write_atomic` (a warning in the log when it fails).
* C · `src/gpu.rs`: `SAMPLES` went; `Gpu::samples` and `Gpu::set_samples` (MSAA chosen by the graphics quality; 1 draws the view straight into the frame).
* C · `src/install.rs`: `Prefs::graphics` ("low" / "medium" / "high", empty: high).
* C · `src/ui.rs`: `Action::SetGraphics(render::quality::Quality)`, View > Graphics quality.
* C · `src/app.rs`: the renderer made with the quality from the settings (`Renderer::new(&gpu, quality)`, `OPENMAPPER_GRAPHICS` overrides for checks); `Action::SetGraphics` makes the renderer again (thumbnails freed); `OPENMAPPER_PROFILE=gpu` times the view on a picture of its own (`Renderer::time_gpu`).
* Geodata · `Cargo.toml`: `ureq` 3 (rustls, no other features) for imagery, heights and place search; `image` also reads TIFF, BMP and WebP (own pictures).
* Geodata · `src/main.rs`: `mod geo`, `mod ui_geo`.
* Geodata · `src/install.rs`: `Prefs::geo` (`geo::sources::GeoPrefs`: the user's own tile servers and keys).
* Geodata · `src/render/mod.rs`: `pub mod imagery`, `Renderer::imagery` (per-tile textures with mipmaps); the tile bind group has the tile's imagery at binding 5 and its sampler at 6 (`tile_bind`, made again when the tile's imagery changes: `GpuTile::{views, imagery_rev}`), its opacity is `TileUniform::flags[1]`. A new renderer (graphics quality) starts empty: `Imagery::epoch` changes and `ui_geo` uploads again from disk.
* Geodata · `src/render/terrain.wgsl`: bindings 5 and 6 of group 1, the imagery mixed into the ground's colour by its alpha times `tile.flags.y`.
* Geodata · `src/ui.rs`: `Action::Geo`, Map menu "Satellite and terrain…" (key `I`), `UiState::geo` (`ui_geo::Bridge`), View menu `ui_geo::view_menu`, the Tiles panel ends with `ui_geo::tiles_panel`; the Tiles hint mentions clicking a tile.
* Geodata · `src/app.rs`: `App::geo` (`ui_geo::GeoUi`): `fill_bridge` before the interface, `window` after the Traffic window (its `look` becomes `Action::LookAt`), `take_bridge` after it, `feed` before `Renderer::sync`; key `I`, `Action::Geo`; a picture dropped with the Tiles tool on goes to `geo.drop_picture` first; the screenshot waits while `geo.busy()`; `OPENMAPPER_GEO=lat,lon[,load][,hide][,picture=<file>[,turn=<deg>]]`.
* Geodata · `src/editor.rs`: `Editor::picked_tile` (the tile clicked or added last with the Tiles tool).
* Geodata · `src/export.rs`: `plan` calls `geo::export_filter` (the map's `openMapper/imagery` stays out of shared zips unless the map's geo.json says `export_imagery`).
* Geodata (round 2) · `src/ui_minimap.rs`: `Minimap::imagery` (each tile's small picture, drawn over its square); `src/app.rs` fills it from `geo.minimap_tiles()` before the interface. `src/ui.rs`: Controls row for `I`.
* Geodata (round 2) · Google-sourced pictures are never written (map folder, backups, exports): `ImageryMeta::kept` false, held in memory and made again from the tile cache (Google's max-age) when the map opens.
* Review · `src/ui.rs`: `Action::SaveThen(Box<Action>)` (the unsaved-changes prompt's Save: the action follows only if the save worked); `src/app.rs`: `Action::Force` passes the guard by (`App::unguarded`) instead of clearing the dirty flags, `Action::Undo / Redo` call `Editor::undo / redo` (they stop the tool first).
* Review · `src/doc/save.rs`: `crate::timetable::follow_stop_tiles(doc)` before the timetable's plan (a stop object moved into another tile).
* Review · `src/history.rs`: `History::object_files` (the junction cleanup after a save keeps what undo and redo can bring back).
* D · `src/main.rs`: `mod texts`, `mod parallel`, `mod ui_find`, `mod ui_groups`; `Cargo.toml`: `omsi-content` (the `.oft` fonts).
* D · `src/assets.rs`: `ObjectModel::{texts, labels, editor_parts}`, `Part::texts`; a text texture is asked for as a texture under a path `texts::path` makes, drawn by `texts::draw` in the loader; `load_object` is `pub(crate)`.
* D · `src/render/mod.rs`: `Renderer::model_with_labels` (a model copy per text shown), `FrameExtras::hide` (trees, objects, splines), `hidden_layer`, `push_object(.., editor)`, the tiles' water kept among the spline meshes under the IDCode `WATER`.
* D · `src/ui.rs`: `UiState::{find, groups, history_open}`, `ObjInfo::labels`; Edit menu "Edit history…", "Groups…", "Find in map…" (Ctrl+F); View > Show; the Tiles panel's water; a spline's length, radius and "Select the whole road".
* D · `src/editor.rs`: `Editor::{shape_request, hide}`, `set_spline_cant`, `parallel_copy`, `replace_everywhere`, `even_selected_chain`, `select_chain` (a double click on a road), `grow_map`, `set_water`, `turn_objects`.
* D · `src/doc/mod.rs`: `TileDoc::water` (read and written with the tile's `.water`); `doc::save` is `pub(crate)` (`tile_text` and `utf16` for groups).
* D · `src/app.rs`: `OPENMAPPER_SELECT=x,y[,spline]`, `OPENMAPPER_HIDE`, `OPENMAPPER_FIND`, `OPENMAPPER_GROUPS`, `OPENMAPPER_HISTORY`; a screenshot run takes no input and does not take the focus; `Prefs::save` does nothing in one.
* D · `src/editor.rs`: `array` (copies in a row), `align_to_roads`, `replace_selected_objects`, `select_alike`; `camera::Camera::axes` is public (Alt + arrows nudge along the screen's nearest map axis).
* D · `src/app.rs`: `snapshot` (View > Save a picture of the view, F12, into `~/Pictures/openMapper`), `OPENMAPPER_SNAPSHOT`; `ui::map_stats` in the map's properties.
* D · `src/main.rs`: `mod ui_mapsettings`, `mod ui_calendar`; `src/ui.rs`: `Action::{MapSettings, Calendar}` (Map menu), `ObjInfo::{paths, attach_points}`; `src/app.rs`: `OPENMAPPER_SETTINGS[=calendar]`, `OPENMAPPER_LEVEL`.
* D · `src/traffic/mod.rs`: `Traffic::{calendar, timezone, registrations, has}` with `File::{Holidays, TimeZone, Registrations}` (written with the map, undone with the traffic).
* D · `src/editor.rs`: `set_rules`, `set_spline_extra` (skew, terrain align), `level_request` / `level_under_objects`, `attach_object`.
* D · `src/main.rs`: `mod signals`, `mod ui_signals`, `mod ui_chrono`, `mod chrono_edit`; `src/ui.rs`: `Action::{SignalRoutes, Chrono}` (Map menu); `src/app.rs`: signal pieces counted again before saving, `OPENMAPPER_SETTINGS=signals|chrono`.
* D · `src/traffic/mod.rs`: `Traffic::signal_routes` with `File::SignalRoutes`; `src/doc/mod.rs`: `Document::{chrono_root, show_chrono}`; `src/doc/save.rs`: `object`, `spline`, `header`, `copy_missing` are `pub(crate)` (chrono patches); `src/check.rs`: `Kind::SignalRouteBroken`.
* D · `src/app.rs`: `ChronoWindow::end_session` before saving and before closing or replacing the map; autosave waits while a scenario is edited on the map. `src/doc/safe.rs`: `own_path` (write targets inside a map's own folder; `resolve_path` may lead to another content root). `src/doc/mod.rs`: `show_chrono_without`.
* D · `src/main.rs`: `mod chrono_tt` (a scenario's timetable and AI lists sessions); `src/ui.rs`: `Action::{OpenLines, OpenTraffic}`; `src/timetable/mod.rs`: `renumber_tiles` is `pub(crate)`; `src/traffic/mod.rs`: the holidays are the timetable's only (`File::Holidays` went); `src/doc/mod.rs`: `one_map` gives back the plain grid when let go (tests).
* D · `src/main.rs`: `mod heightmap`; `src/ui.rs`: `UiState::heightmap` (its buttons at the end of the Terrain panel), `BG_FIELD` is `pub(crate)`; `src/editor.rs`: `Editor::flatten_fixed`; `src/app.rs`: `OPENMAPPER_TOOL` also takes `terrain` and `paint`, `OPENMAPPER_HEIGHTMAP=<picture>[,apply]`.
* D · `src/main.rs`: `mod autopaint`; `src/ui.rs`: `UiState::autopaint` (its rule at the end of the Paint panel); `src/app.rs`: `OPENMAPPER_AUTOPAINT=<layer>`.
* D · `src/editor.rs`: `reverse_selected`; `src/reshape.rs`: `reversed`; `src/ui.rs`: a spline's "Reverse direction" (`ui_edit::reverse_help`), also for several in `group_properties`.
* D · `src/render/mod.rs`: `pub mod daylight`, `Renderer::daytime` (the light of the view from it; `light_view` takes the sun, the shadow pass uses the globals' sun); `src/ui.rs`: `UiState::daytime`, View > Time of day; `src/app.rs`: the renderer's daytime set before `sync`, `OPENMAPPER_DAYTIME=<hour>`.
* D · `src/main.rs`: `mod onground`; `src/editor.rs`: `lay_selected_on_ground`; `src/ui.rs` / `src/ui_edit.rs`: "Lay on the ground" for a road and a selection (`ui_edit::lay_help`); `src/app.rs`: `OPENMAPPER_LAY=1`.
* D · `src/main.rs`: `mod osm`, `mod ui_osm`; `src/ui.rs`: `Action::Osm` (Map menu); `src/app.rs`: `App::osm` drawn after the Satellite window, its build request run after the interface (`Editor::build_osm` with the assets), `OPENMAPPER_OSM=<main type>[;<street type>]`, `OPENMAPPER_SAVE=1` (saved at the third frame); `src/editor.rs`: `build_road`'s body is `lay_road` (no step of its own), `build_osm`; `src/ui_geo.rs`: `GeoUi::georef` is `pub(crate)`; `src/geo/fetch.rs`: `overpass`.
* D · `src/main.rs`: `mod osm_lines`; `src/app.rs`: after the OSM roads and stops, the bus routes into the timetable (`osm_lines::add`, one snapshot step); `Editor::build_osm` takes the one-way type, `Editor::place_osm_stops`.
* D · `src/main.rs`: `mod osm_green`; `src/editor.rs`: `plant_osm`; `src/app.rs`: the OSM build plants the trees last.
* D · `src/assets.rs`: `Material::glow` (`[matl_nightmap]` / `[matl_lightmap]`); `src/render/mod.rs`: the material's binding 5 is its glow texture, `MatUniform::mode[3]` its kind, `Globals::shadow[2]` how dark it is (`Light::night`).
* D · `src/ui.rs`: `Action::SetSeason`, `UiState::season`, View > Season; `src/app.rs`: `set_season` (omsi-texture's season folder, the assets and models read again), `OPENMAPPER_SEASON`; `src/render/mod.rs`: the ground's textures looked for in the season's folders first.
* D · `src/main.rs`: `mod osm_buildings`; `src/editor.rs`: `place_osm_buildings`; `src/app.rs`: the OSM build puts the buildings before the trees; `OPENMAPPER_OSM`'s seventh field a building folder (the catalogue is read first).
* D · `src/history.rs`: `mark` and `merge_since` (the OSM build is one step). `src/main.rs`: `mod osm_water`; `src/osm_green.rs`: `rings` is `pub(crate)`; `src/app.rs`: the OSM build digs the water after the roads (they keep their heights over it).
* D · `src/editor.rs`: `Editor::{plant_area, plant_outline}`, `finish_plant_outline` (Enter via `confirm_road`, Esc and right-click take it back); `plant_osm` calls `plant_spots`; `src/app.rs`: `ed_has_pending` counts the outline, `OPENMAPPER_PLANT=<plant>;x,y;…[;open]`.
* D · `src/main.rs`: `mod ui_profile`; `src/ui_edit.rs`: `group_properties` takes `actions` (the height profile after Even slope); `src/ui.rs`: a single road's properties end with `ui_profile::profile_section`; `src/app.rs`: `OPENMAPPER_SELECT=x,y,chain`.
* D · `src/main.rs`: `mod ui_views`; `src/ui.rs`: `UiState::views`, View > Saved views…; `src/app.rs`: the window drawn after the Satellite window (`follow` the map's folder), `Ctrl(+Shift)+1…9`, `OPENMAPPER_VIEWS=<n>`.
* D · `src/main.rs`: `mod ui_palette`; `src/ui.rs`: `UiState::palette`, Edit > Search for a command…, drawn after the edit history, `SELECT` is `pub(crate)`; `src/app.rs`: `Ctrl+K` / `Ctrl+Shift+P`, `OPENMAPPER_PALETTE=<text>`.
* D · `src/install.rs`: `Prefs::last_views` (`remember_view`, `last_view`); `src/app.rs`: `remember_view` before another map opens, on Close and Quit; `after_open` goes there (not in screenshot runs).
