# openMapper

**A simple map editor for [openOMSI](https://openomsi.org/).** Draw roads with a couple of
clicks, place buildings and trees from a visual catalogue, shape and paint the ground, and
drive your map in openOMSI with a single button. It's built for anyone to pick up, not just
experienced map makers.

> openMapper needs an installed **OMSI 2** (Steam or retail): maps are built from its
> content. The original folder is never written to. Maps and imported assets go into
> openOMSI's content folder, which the game reads before the installation.

## What it can do

| Tool | Key | What it does |
| --- | --- | --- |
| Select | 1 | click an object or road · drag to move · `R` turn · `Del` delete |
| Roads | 2 | click the start, then the end: the arc builds itself. A road continues smoothly from a road end it starts at, joins another end with an S-curve or bend (biarc), is cut into OMSI splines and linked with its neighbours. It can shape the ground under itself. |
| Junctions | — | automatic: a road drawn into another's middle or across it cuts both and fills the gap with a junction object of its own (asphalt plate, `[path]` lanes for straight on, right and left, footpaths round the corners), written as ordinary OMSI content into `Sceneryobjects\openMapper\<map>` |
| Objects | 3 | catalogue as the installation's folder tree with thumbnails; classes read from the files themselves (roads, tracks, footpaths, decoration, technical AI/helper splines; trees, stops, junction pieces, technical helpers), search, turning towards the nearest road |
| Game logic | 4 | start points (`[entrypoint]`), bus stops, car parks (one by one or in a row along the kerb), AI paths for cars and people drawn like roads and snapped to lane ends, sound sources and other editor-only helpers |
| Terrain | 5 | raise / lower / smooth / flatten brush |
| Paint | 6 | painting with the map's ground textures (OMSI `[groundtex]` masks) |
| Tiles | 7 | add a 300 × 300 m tile next to the map, remove one with right-click |

* **Open any map**: OMSI 2's own (saved as a copy into the content folder) and mod maps.
* **New map** from OMSI 2's own template (`template/NewMap`): it's ready to drive right away.
* **Test drive**: saves and launches openOMSI on this map.
* **Map picture** (`picture.jpg`, shown where maps are chosen) taken from the view, and the
  view the map opens with (`[mapcam]`), both in the map's properties.
* **Favourites and recently used** at the top of the catalogue: right-click an entry to star
  it.
* **Overview map** in the corner of the view (`M`): tiles, roads, tracks and the camera;
  click or drag on it to move there, double-click for a larger one.
* **Asset import**: drop onto the window or use the button —
  * a mod (a folder or `.zip` laid out like OMSI 2) is merged into the content folder;
  * a folder of `.sco` / `.sli` files becomes a folder under `Sceneryobjects` / `Splines`;
  * a 3D model (`.obj` from Blender and others, `.x`, `.o3d`) becomes a new object: a `.sco` is
    written for it, the textures are copied, and so are their **PBR maps** (`_nn`, `_orm`,
    `_rr`, `_mm`, `_aa` …, the way openOMSI finds them).
* **PBR** in the viewport: normal, roughness, metalness and AO maps are found by
  openOMSI's rules (`omsi-texture`) and drawn with the same derivative-based tangent frame.
* Undo / redo for every action, two languages (English / Russian).

## Controls

* **Mouse:** right drag turns and tilts the camera, wheel zooms toward the cursor, middle drag
  or left drag on empty ground (Select tool) moves the map.
* **Trackpad:** two fingers move the map, pinch zooms, twist turns; Option + two fingers or
  Option + drag turns and tilts.
* **Keyboard:** `W A S D` / arrows move, `Q / E` turn, `1–7` tools, `M` overview map, `R` turn object, `Del`,
  `F` go to selection, `[ ]` brush size, `G` tile grid, `Ctrl+Z`, `Ctrl+Shift+Z`, `Ctrl+S`,
  `F5` test drive.

## How it's built

Rust, written from scratch for openOMSI:

* **wgpu** from the org's fork [`wgpu-openomsi`](https://github.com/openOMSI-Project/wgpu-openomsi)
  (the same revision openOMSI uses), Metal / Vulkan / Direct3D 12 / GL;
* the interface is [`egui-openomsi`](https://github.com/openOMSI-org/egui-openomsi), laid out
  like a classic desktop editor: menu bar, toolbar, docked panels (tool/catalogue on the left,
  properties on the right), status bar with help and cursor coordinates;
* icons are Google's [Material Symbols](https://fonts.google.com/icons) (Apache 2.0), stored in
  the repository: the full font is in `assets/fonts`, and the binary embeds a ~10 KB subset of
  the icons it uses (`tools/subset-icons.py`, add a name there to get a new icon). Nothing is
  downloaded at run time;
* reading and building content uses openOMSI's own crates: `omsi-map` (global.cfg, tiles,
  terrain), `omsi-scenery` (.sco/.sli), `omsi-model`, `omsi-o3d`, `omsi-geometry` (spline
  meshes exactly as in the game), `omsi-texture` (DDS/BMP/TGA, PBR), `omsi-cfg` (paths, VFS).

Saving writes OMSI 2's format: UTF-16 with CR LF, tiles at `[version] 14`,
`.map.terrain`, A8 DDS painting masks `texture/map/tile_x_y.map.N.dds`. Tests check
that the written tiles read back through openOMSI's own parser.

```bash
cargo run --release
```

`openmapper path/to/global.cfg` opens a map straight away. `OPENMAPPER_SHOT=out.png` saves
a picture of the window and exits (for checks without a person at the screen).

## License

MIT
