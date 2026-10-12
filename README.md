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
| Select | 1 | click an object or road (double-click a road: all of it, every piece up to where it branches or ends; All like it in the properties: every one of its type) · `Shift`+click adds or takes away · `Shift`/`Ctrl` + drag on empty ground selects with a box · drag the selection to move it all · `R` turns it round its middle · `Del` deletes it · `Ctrl+C / X / V / D` copy, cut, paste at the cursor, duplicate (links inside the copy kept, rows of objects and things hanging on them come along) · several selected: move, raise and turn by numbers, give every road one type or every object the catalogue's one (also for one object: Replace with), one even slope along a chain of roads (a bridge, a ramp), objects turned each a random way, all like one, or each the way the nearest road runs beside it (also the road button by an object's rotation) |
| Road shape | - | select a road: drag its end dots to move an end (a road joined there follows, tangent at the joint, its far end kept; an end at a junction stays), drag the middle dot to bend it. Heights, gradients and cant (banking) at both ends in the properties, the joined road following; length and radius (straight, left, right) by numbers, the road joined at its end carried along, tangent; the ends' skew (a slanted cut: fences meeting at a corner) and the ground cut away under it (tunnels, underpasses: OMSI's terrain align) |
| Roundabouts | - | Roads tool option: click the centre, then the ring (or drag out to it). A one-way ring of the chosen road type, anticlockwise where traffic keeps right and clockwise where it keeps left; the roads that ran into the circle are cut back and joined to the ring with junctions |
| Drawing by numbers | - | each road is two clicks (going on from the end just built is an option); while a road is drawn: type its length, radius or heading in the fields at the view's corner (empty ones follow the cursor), `Shift` draws a straight, `Enter` builds it. "Keep the height" draws bridges and embankments: an even gradient, a rise typed for the end, the ground left alone (pillars: objects along the road) |
| Junction lights and crossings | - | select one of openMapper's junctions: "Traffic lights" writes OMSI's light program into it (main road and side roads, greens editable) with the installation's own signal heads on poles bound to it, the AI stopping at red; zebra crossings per road, with a footway across. A road drawn into a plate, or a road end dropped on one, becomes another arm of it |
| Junction moves | - | a junction (or any piece with spline helpers) moved, turned or raised carries the ends of its roads along, tangent, the road before a short arm bending with it; the ground follows its plate; openMapper's own junction plates are made again for their roads, and deleting an arm leaves the others joined (two left: a smooth joint) |
| Objects along a road | - | select a road or a chain: put the object chosen in the catalogue every so many metres beside it (one or both sides, margins, height, turn, leaning), as OMSI spline attachment rows that follow the road; the road's properties list its rows to change or remove |
| Parallel copy | - | select a road, a track or a chain: copy it alongside to the left or right at a distance (a second track, the other carriageway); bends stay exact arcs about the same centre, the copies linked as the originals |
| Ruler | U | status bar: click two points for the distance, the height between them and the gradient |
| Snapping | - | status bar: `Grid` (0.5–10 m) for placing, moving, road ends and reshaping; `15°` for headings |
| Roads | 2 | click the start, then the end: the arc builds itself. A road continues smoothly from a road end it starts at, joins another end with an S-curve or bend (biarc), is cut into OMSI splines and linked with its neighbours. It can shape the ground under itself. |
| Junctions | - | automatic: a road drawn into another's middle or across it cuts both and fills the gap with a junction object of its own (asphalt plate, `[path]` lanes for straight on, right and left, footpaths round the corners), written as ordinary OMSI content into `Sceneryobjects\openMapper\<map>` |
| Assets | B | a panel like Unity's Project window: folders of objects and splines, a grid of pictures with search, kind filter and size; an asset's facts, place or build with it, favourite, PBR, show in the file manager, delete (content folder only, into `openMapper\bin`); double-click to use, drag a picture into the view to place it |
| Switches | 2 | a track drawn off or into another's middle lays OMSI 2's own EW190 switch (left or right, for its rail type): the main track is cut out under it, the new track joins its diverging heel |
| Plants | 3 | Objects tool: "Plant brush" sows the chosen tree (or a mix: right-click entries, "Add to the plant mix") with a spacing and random turns; "Clear" takes trees away |
| Lining up | 3 | buildings face the nearest road with their front at a set distance from its edge (Shift: put freely); junction and crossing pieces (spline helpers) click onto the road ends near them |
| AI paths | 4 | Game logic > AI paths > "Edit existing": pick a path, drag its ends and middle, Ctrl+click on it adds a point, Del deletes it; lanes are drawn thin with small arrows |
| Road types | - | Roads tool > "Road types and junctions…": a road maker (lanes each way, lane width, middle strip, footways, kerb, textures) writing an ordinary OMSI spline, and per road type how its junctions are made (corners, lights, zebras, or no junction at all). Tracks branch off one another as switches and cross roads on the level |
| PBR maker | - | an object's "Create PBR" (or Tools > PBR maps for a picture): normal, roughness, metal and occlusion maps made from a texture with a live preview, written as openOMSI reads them (`_nn`, `_orm`); then "Configure PBR" to tune them again or delete them |
| Objects | 3 | catalogue as the installation's folder tree with thumbnails; classes read from the files themselves (roads, tracks, footpaths, decoration, technical AI/helper splines; trees, stops, junction pieces, technical helpers), search, turning towards the nearest road |
| Game logic | 4 | start points (`[entrypoint]`), bus stops, car parks (one by one or in a row along the kerb), AI paths for cars and people drawn like roads and snapped to lane ends, sound sources and other editor-only helpers |
| Terrain | 5 | raise / lower / smooth / flatten / roughen brush (rolls and dips of a natural field); Level ground in an object's properties makes the ground under its outline level with its foot (a house on a slope) |
| Paint | 6 | painting with the map's ground textures (OMSI `[groundtex]` masks) |
| Tiles | 7 | add a 300 × 300 m tile next to the map (or a whole ring of them round it with one button), remove one with right-click; a tile clicked gets water (OMSI's `[water]`, a surface at a height, seen where the ground lies below it: lower the ground for a lake or a river) |

* **Lines and timetable** (Map menu or `L`): lines, their trips and tours, written as OMSI
  2's `TTData` (`.ttl`, `.ttp`, `.ttr`, `StnLinks.cfg`, `Busstops.cfg`). Pick the stops on the
  map or from a list; the route along the roads is found by itself (drawn blue, each problem
  named), "Make the way back" turns a trip round, departures fill themselves from a first and a
  last time with a break, and a map without a depot gets one so AI buses drive the tours. The
  files of a stock map stay as they are until its timetable is edited.
* **Traffic and AI** (Map menu or `T`): the road and passenger density over the day as a
  curve with draggable points, car groups and depots of `ailists.cfg` with vehicles picked
  from the installation, which groups drive about on their own, parked cars and people. A group
  the map lacks can be copied from OMSI 2's own maps.
* Timetable and traffic edits are part of undo / redo; trips have times at each stop, spread
  by distance or fixed by hand.
* **Export map for sharing** (File menu, or `openmapper --export global.cfg out.zip`): a zip
  laid out like OMSI 2 with the map and every file of the content folder it uses (generated
  junctions, imported models with their textures and PBR maps, mod assets), never the
  installation's own files; a `readme.txt` lists what it needs. Dropping the zip on openMapper
  (or `openmapper --import file`) installs it.
* **Descriptions in several languages** (`global_<LANG>.dsc`) in the map's properties, and
  **holidays and school holidays** (`Holidays.txt`) in the Lines window.
* **Satellite and terrain** (Map menu or `I`): a map of the world in the window (drag, wheel, search a
  place by name), "Place the map here" and a click put the map's middle tile there (north turned
  by any angle); a `[worldcoordinates]` map lies on the Earth by itself. Then imagery for all tiles,
  the selected one, or each new tile by itself, and the terrain heights with it or on their own
  (relative to the middle tile, raised, smoothed; one undo step; the tiles' shared edges match).
  Each tile's picture is cut from the source exactly to the tile at the source's own detail (up to
  4096 px), so tiles meet without a seam, and lies on the ground in the 3D view; the overlay is on
  or off and as opaque as wanted, as a whole (also View menu) and per tile (also in the Tiles tool).
  The sources and how sharp they are at the map's place are listed, the sharpest picked: Esri World
  Imagery (no key), official orthophotos with no key (Berlin, Brandenburg, NRW, Bavaria, Lower
  Saxony, the Netherlands, Switzerland, Austria, France, New York State), and with the user's own
  key Google (Map Tiles API), Mapbox or MapTiler; any XYZ, WMTS or WMS server can be added. Heights
  come from the AWS Terrain Tiles (Terrarium), place search from OpenStreetMap's Nominatim. Each
  source's attribution is shown. Downloads are cached in `~/.openomsi/cache/openmapper/tiles`
  and never fetched twice; the map keeps its pictures in `openMapper/` in its folder (the game
  ignores it), left out of shared exports unless asked for.
* **Own pictures** on tiles (a scan, a screenshot, an old plan), no place on the Earth needed:
  "Load image file…" for a tile (or drop the picture on the window with the Tiles tool and a tile
  clicked), or one picture over a rectangle of tiles, cut into their pieces; turned (90° steps or
  any angle), mirrored, moved and scaled by eye with the 3D view following.
* **Names in the view**: bus stops and start points near the camera carry their names over
  them (View menu to switch off).
* **Layers** (View > Show): trees, other objects, roads and splines each hidden or shown;
  what is hidden is not picked or box-selected either, handy in a dense town.
* **Copies in a row** (an object's or a selection's properties): a fence, a row of houses, an
  avenue: N copies a step apart, each turned a few degrees more if wanted, in one undo step.
* **Groups** (Edit menu): a selection kept under a name (a stop with its shelter and signs, a
  crossing with its lights, rows and what hangs on them included), to put down again in any
  map with `Ctrl+V`; kept as tile files in `<content>/openMapper/groups`.
* **Hanging objects** (an object's properties, Hanging on it): what hangs on its attachment
  points (`[new_attachment]`), and the catalogue's object hung on one, as OMSI 2's editor does
  it (signs on a post, a timetable on a stop).
* **Path rules** (a road's or an object's properties), as in OMSI 2's editor: a speed limit,
  traffic density, priority, trucks, no cars or buses only, each for one path (lane).
* **Signal routes** (Map menu), as OMSI 2's editor keeps them in `signalroutes.cfg`: for each
  railway signal the track pieces it protects (taken from the tracks and switches selected in
  the view), its distant signal, the next signal, its aspect and a speed limit; each piece's
  place among its tile's paths is counted again on saving, as the game counts it (all 649
  pieces of the stock maps come out the same). The map check names routes whose signal or
  track is gone.
* **Chrono scenarios** (Map menu): the map's changes over the years listed with their dates,
  tiles, the lines they take off and their descriptions; the view shows the map as on any day
  chosen, every scenario active then laid over it as the game does. A scenario's dates, the
  lines it takes off, its name and description are edited there, and new scenarios made (the
  installation's scenarios are brought into the map's own copy first). What is selected in the
  view is put into a scenario (it appears from its first day), taken away from that day on, or
  given the catalogue's type then, written into the scenario's tile patches as OMSI 2's
  editor does (`[selobject]`, `[delete]`, `[typ]`).
* **Map settings** (Map menu), as in OMSI 2's editor: money system, ticket pack, depot,
  years and year offset, repair time, real rail and the seasons of the year.
* **Calendar and place** (Map menu), the map's own files as OMSI 2's editor keeps them:
  where it lies and its time zone (the sun), summer time (EU rules filled in for a span of
  years), public and school holidays by year (the timetable's day types) and the AI cars'
  number plates (made from a pattern such as `B-XX 9999`).
* **Find in map** (Edit menu or `Ctrl+F`): every kind of object and spline the map uses,
  with how many, by name, file or IDCode; select them all, go from one to the next, or replace
  them all with the object (road type) chosen in the catalogue in one undo step.
* **Texts on objects**: street signs, stop plates and line numbers show their labels in the
  view, drawn with OMSI's `.oft` fonts as the game draws them (`[texttexture]`); an object's
  texts are listed by name in its properties (`@` starts a new line).
* **Open any map**: OMSI 2's own (saved as a copy into the content folder) and mod maps.
* **New map** from OMSI 2's own template (`template/NewMap`): size, keep right or left, base
  ground, and "Start with a sample": a ring road with stops, a start point, line 1 both ways,
  a tour from 06:00 to 22:00, a depot and AI traffic, so it can be driven in service at once.
* **Getting started** (Help menu, opens by itself for a new map): a checklist that ticks
  itself as the map grows (road, stop, start point, line, tour, buses, saved, test drive),
  with a "Show me" for each step.
* **Test drive**: saves and launches openOMSI on this map.
* **Map check** (Map > Check map…): missing objects, roads and textures, road ends lying on
  one another but not joined, links to splines that are gone, bus stops with no lane near,
  start points away from a road, IDCodes used twice, a too low NextIDCode, objects off their
  tile, tiles without a file or ground; trips no way is found for, tours without departures or
  a depot, traffic groups missing from the AI lists, stops no trip calls at. Every issue is explained, a click flies there, many
  are put right with one more click (and can be undone). `openmapper --check path/to/global.cfg`
  prints the same without a window.
* **Safe saving**: every file is written under a temporary name and put in place only once
  all of them are written, so a full disk or a crash leaves the map as it was. The files a save
  replaces are kept (the last 10 saves, in `<content>/openMapper/backups/<map>`); File > Earlier
  versions… puts the map back as it was before any of them. The changes are autosaved every
  3 minutes to `<content>/openMapper/autosave` (never into the map) and offered back at the next
  start if openMapper ended without saving (the timetable and the traffic too).
* **Map picture** (`picture.jpg`, shown where maps are chosen) taken from the view, and the
  view the map opens with (`[mapcam]`), both in the map's properties.
* **Favourites and recently used** at the top of the catalogue: right-click an entry to star
  it.
* **Overview map** in the corner of the view (`M`): tiles, roads, tracks and the camera;
  click or drag on it to move there, double-click for a larger one.
* **Asset import**: drop onto the window or use the button:
  * a mod (a folder or `.zip` laid out like OMSI 2) is merged into the content folder;
  * a folder of `.sco` / `.sli` files becomes a folder under `Sceneryobjects` / `Splines`;
  * a 3D model (`.obj`, `.gltf` / `.glb`, `.fbx` from Blender, 3ds Max, Maya and asset stores, `.x`, `.o3d`) becomes a new object, with every node's transform, its materials' colours and textures (embedded ones too; any picture format is turned into one OMSI reads), and their normal and roughness / metalness / occlusion maps as `_nn` / `_orm`: a `.sco` is
    written for it, the textures are copied, and so are their **PBR maps** (`_nn`, `_orm`,
    `_rr`, `_mm`, `_aa` …, the way openOMSI finds them).
* **PBR** in the viewport: normal, roughness, metalness and AO maps are found by
  openOMSI's rules (`omsi-texture`) and drawn with the same derivative-based tangent frame.
* Undo / redo for every action (Edit > Edit history… lists the steps: a click goes back or forward to one), two languages (English / Russian), interface size 80–150 % (View menu).

## Controls

* **Mouse (as in Unity's scene view):** hold the right button to look round and fly with
  `W A S D` (`Q / E` down / up, `Shift` faster, the wheel sets the speed meanwhile), the wheel
  zooms, the middle button pans, `Alt` + left / right / middle drag orbits / zooms / pans, a
  left drag on nothing selects with a box. View > Camera > Map editor: right drag turns round
  the cursor and dragging the ground moves the map.
* **Trackpad:** two fingers move the map, pinch zooms, twist turns; Option + two fingers or
  Option + drag turns and tilts.
* **Keyboard:** `W A S D` / arrows move, `Q / E` turn, `1–7` tools, `M` overview map, `B` assets, `L` lines, `T` traffic, `I` satellite and terrain, `R` turn object, `Del`,
  `F` go to selection, `[ ]` brush size, `G` tile grid, `Ctrl+Z`, `Ctrl+Shift+Z`, `Ctrl+S`,
  `Ctrl+C / X / V / D` copy, cut, paste, duplicate, `Ctrl+A` select all, `Ctrl+F` find in map, `Alt + arrows` nudge the selection a grid step (`Shift`: 10 cm), `Alt + Page Up / Down` raise or lower it, `U` ruler, `Home` look straight down, `F5` test drive, `F12` save a picture of the view (into `~/Pictures/openMapper`).

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

`tools/package-macos.sh` makes `openMapper.app` and a zip of it in `target/package`.

`openmapper path/to/global.cfg` opens a map straight away. `OPENMAPPER_SHOT=out.png` saves
a picture of the window and exits (for checks without a person at the screen).
`OPENMAPPER_PROFILE=1` logs how long reading the map and each part of a frame take (every few
seconds, with the draw calls), `OPENMAPPER_PROFILE=orbit` also turns the camera round by
itself. On Berlin-Spandau (329 tiles, 2486 splines, 29 450 objects, Apple M4) a frame takes
about 13 ms where it took 36 ms: what is out of view or too small to see is left out, a tile's
splines are drawn as one mesh per material, the objects' instances stay on the GPU and the
selection only tints a few of them, and an edit builds only its own tile again. The tiles are
read side by side, several at a time, and the roads' meshes and the ground cut under them are
made on threads of their own. View > Graphics quality (Low / Medium / High) sets the shadows,
edge smoothing, how far the view reaches and how far small things are drawn.
`OPENMAPPER_PROFILE=gpu` measures the GPU's time for the view apart from the screen's refresh.

## License

MIT
