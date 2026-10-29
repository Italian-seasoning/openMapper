# Modern maps: trams, trolleybuses, metro and trains

openMapper makes two kinds of map:

| | Classic | Modern |
| --- | --- | --- |
| Runs in | OMSI 2 and openOMSI | openOMSI 0.3.42 and later only |
| Transport | buses; trams and trains as OMSI 2 has them (tracks, switches, signal routes, `TTData` timetables) | the modes chosen for it: bus, tram, trolleybus, metro, railway, one or several |
| Network | tours and trips of the timetable | stations, routes of segments and services (no tours needed), track rules, overhead wire, third rail, trolleybus wire, switches, depots, signal blocks |
| Vehicles | any | only those of the map's modes (openOMSI refuses the others and says why) |

A Modern map is chosen in **New map** (Classic / Modern, then the modes) or later in **Map >
Map settings**. Everything else of a map (roads, objects, terrain, the timetable, traffic) works
on both kinds; a Modern map may keep `TTData` lines beside its routes.

OMSI 2 cannot run what makes a map Modern: it skips the mode keywords and the `transport`
folder, so a Modern map loads there, but its routes, power and blocks do nothing and openOMSI's
own vehicles rules do not apply. That is the trade: openMapper does not hold Modern maps back to
what OMSI 2 can do.

## What is written

All of it is openOMSI's own format ([TRANSPORT_FORMATS.md](https://github.com/openOMSI-org/openOMSI/blob/main/docs/TRANSPORT_FORMATS.md),
[TRANSPORT_MODES.md](https://github.com/openOMSI-org/openOMSI/blob/main/docs/TRANSPORT_MODES.md),
[TROLLEYBUS.md](https://github.com/openOMSI-org/openOMSI/blob/main/docs/TROLLEYBUS.md)):

| File | From |
| --- | --- |
| `global.cfg`: `[bus_transport]`, `[tram_transport]`, `[trolleybus_transport]`, `[metro_transport]`, `[railway_transport]` | the map's modes |
| `transport/network.cfg` | Transit network > Tracks: which track types serve which mode, street or railway, power, tunnel, speed, gauge |
| `transport/stations.cfg` | Transit network > Stations: a station on a stop object, its modes, platforms, the stations to change to |
| `transport/routes.cfg` | Transit network > Routes: segments (from, to, the stops between, express stops passed, standing time, speed), services (segments in order, headway, from–to, days, what happens at the end, cars in a train, vehicle, depot), departures of their own |
| `transport/depots.cfg` | Transit network > Depots: AI group, capacity, gates, its stop |
| `transport/catenary.cfg` | Transit network > Power > Overhead wire: the line, sections, spans laid along tracks, insulators |
| `transport/thirdrail.cfg` | Transit network > Power > Third rail: the same for the third rail |
| `trolley.txt` | Transit network > Power > Trolleybus wire: the line, sections, a pair of wires over each way's lane of the selected roads, frogs |
| `transport/switches.cfg` | Transit network > Switches and signals: the selected switch objects, rest position, drive, diverging speed, locking signal |
| `transport/signals.cfg` | Transit network > Switches and signals: blocks (where they begin and end on the rails) and the signals that show them |

A file is written only when something in it changed in openMapper; keys openMapper does not
edit itself are kept as they were (comments are not). The Overview tab, and **Map > Check**,
show what openOMSI's own reader of these files finds (the game's crate `omsi-transit`, the
same texts as its log), and two checks of openMapper's: a station whose stop is not on the map,
and a station with no lane of its modes within 25 m (the game routes a service over those).

Stations are the map's stop objects: saving puts each station's stop into the timetable's stop
list (`TTData/Busstops.cfg`), where openOMSI looks for it.

## The transit kit

OMSI 2 ships no metro, third rail, trolleybus wire or tram platforms, so openMapper makes its
own. They are made, not copied: nothing of the installation is in them, so they go with any map
(Export packs them like the map's other content).

* Splines (`Splines\openMapper\Transit\`): tram street track (rails in paving, flush with a
  road), tram track on grass, railway track on ballast, metro track on a slab with a third rail
  (OMSI 2's own `[third_rail]` too), the metro track in a tunnel; tram (0.30 m), metro (1.10 m,
  side 4 m and island 10 m) and railway (0.76 m, side 4 m and island 8 m) platforms with a
  walkway; the overhead contact wire (copper, round) on droppers every 8 m from its messenger;
  trolleybus wires (round copper conductors) as one pair or a pair over each lane of 3.0 m or
  3.5 m; tunnel shells made to the width of what runs in them.
* Objects (`Sceneryobjects\openMapper\Transit\`), textured (galvanized and painted steel,
  copper, brown porcelain, cast iron, concrete): catenary masts on a footing with their
  cantilever (bracket, stay, registration and steady arm, insulators), trolleybus poles with a
  bracket and an insulated hanger over each pair of wires, the trolleybus frog, section
  insulators, tunnel portals, a metro sign lit at night, a platform shelter, a buffer stop, and
  the station stop (a `[busstop]`). Masts, poles and portals are made to measure when wiring
  or digging asks for them (`om_catenary_mast_r30`: reaching 3.0 m; `om_trolleybus_pole_h22_57`:
  hangers 2.2 m and 5.7 m out).

The kit is made in the content folder when a map is made Modern (or with **Make the kit** on
the Overview tab), and appears in the catalogue under openMapper; the Overview tab also has a
button for each piece. A Modern map gets a `network.cfg` rule for each kit track of its modes,
and the networks of its lines (600 V overhead for trams, 15 kV for a railway alone, 750 V
third rail, 600 V trolleybus wire) with one live section each.

## Start with a tram line

A Modern map with Tram can start as a working line: a loop of street track under its wire with
masts, two stations, a loop route with a service every 10 minutes from 05:30 to 23:30, the
wire's spans over the whole loop and a start point on the rails. openOMSI routes the service
by itself (`transport route T1_all_day_out: 17 lanes, 0.8 km` in its log). OMSI 2 has no tram
of its own: the AI service runs with the vehicles of a depot's AI group, and to drive one
yourself you need a tram mod.

## What openOMSI does not do yet (0.3.45)

These are the game's limits, not openMapper's; the files say what is wanted already:

* one overhead network per map: a 600 V tram and a 15 kV railway cannot have their own lines on
  one map;
* AI vehicles are not powered by the line (only the player's vehicle needs live wire);
* the wire's height is the class's standard one, not read from the map;
* a depot's `capacity` is read but not acted on;
* signal blocks hold only the timetable's AI trains;
* the trolleybus simulation reads `trolley.txt`, not a two-wire `catenary.cfg`.

And openMapper's own:

* openOMSI looks at the 3 lanes nearest each station: a two-way track is two of them, so a
  station beside a dense junction may be given junction pieces that lead nowhere the line goes
  (Prague's I. P. Pavlova: 17 of 27 trips routed). The game's log names such a trip (`no way over
  the track from … to …`): move that station's stop along its track away from the junction.

* a station's stop is placed like any stop (the kit's station stop, or a bus stop); platforms
  are splines laid beside the track by hand (Parallel copy helps);
* a trolleybus junction wired whole, every turn of a crossroads, makes joints of four or more
  wires, where openOMSI makes no frog: wire the turns the lines take (the roads they run on and
  the junction between them), and the toast says where it happened;
* a metro line from OpenStreetMap is built at ground level (its tunnels too): build it with the
  kit's tunnel track and cut the ground away (the road's ground cut, Map > Check finds buried
  roads), or lay it by hand.

## Wiring

**Power > Electrify the selected tracks** (overhead wire, third rail) and **Wire the selected
roads** (trolleybus) lay spans over the lanes the selected pieces really have, read from their
types:

* a track's every rail lane (a double-track spline gets a wire over each track), with the
  kit's catenary wire spline over it (a parallel copy where a track runs off the spline's
  middle);
* a road's kerb lane of each way (the outermost lane on the side its traffic keeps to), a pair of
  trolleybus wires over it, in the way the lane runs;
* the selected junction plates and switches along their own lanes, joined into one lane through
  the plate; for the trolleybus only the lanes from a wired road into a wired road (the turns
  the lines take), each with a wire spline over it;
* where exactly three trolleybus wires meet, a frog set by the current the bus draws.

With **lay the wires, masts, poles and frogs too** (on by default) the hardware comes with them:

* catenary masts every 40 m, at the kerb of the street a track runs in (on the track's side of
  it) or 3 m beside a track off the street, each reaching exactly to its wire;
* trolleybus poles every 30 m at the road's kerb, the bracket reaching over the road with a
  hanger over each wired pair;
* a frog object at each frog, facing the single wire.

Spans laid again over the same piece replace its old ones; **Take their spans off** takes them
away.

## Building into the ground

A road's (or a selection's) properties have **Build into the ground**: the chain of the
selected pieces lowered by a depth, with ramps at its ends at the gradient asked for (eased in
and out, so a ramp is 1.5 times depth over gradient long):

* **Cut**: an open cutting; the ground is dug down to the road and its banks sloped up to the
  ground round it (metres out per metre up), the ground under the road cut away besides;
* **Cover**: cut and cover; open ramps down to the portals and, where the road runs 6 m or more
  under the ground, a tunnel under the ground as it was: a shell of walls, roof and floor over
  the road (made to its width) and a portal headwall with wing walls at each mouth. The ground
  is cut away over the first 10 m of the shell at a mouth (the ground's coarse grid cannot
  stand up straight there), so a short stretch of roof shows behind a portal.

The pieces are cut so no piece is longer than a third of a ramp (a cubic per piece follows the
eased ramp) and the ground is only ever taken down. A wire laid on the lowered pieces is taken
off with its masts, poles, frogs and spans (it would hang at the old height): wire them again.
All of it is one step to undo. An end without a ramp stays at the full depth (to go on into
another piece built so).

## Platforms

**Stations > Platforms at the stations** lays the kit's platform of each rail station's mode
beside the track nearest its stop, on the side the stop stands, its edge 1.6 m from the track's
middle, as long as the station's platform says (else 30 m for a tram, 100 m for the metro,
120 m for a train), centred on the stop; a station without a platform gets one with that side,
length and height. A station with a platform within 12 m is left as it is.

## Vehicles

openOMSI runs only vehicles of a Modern map's modes on it. A vehicle file says its modes with
the `[…_transport]` keywords, else openOMSI judges it by its shape (a rail vehicle a tram, a
metro train with a contact shoe, a railway train over 45 t or with `[rail_class]`; trolley poles
a trolleybus and a bus; anything else a bus). openMapper reads them the same way:

* the Depots tab shows how many vehicles of each mode a depot's AI group has;
* Map > Check names the vehicles of the AI lists the game leaves out on the map (a vehicle that
  says another mode; a depot's vehicle of another shape: cars of no mode stay, as in the game),
  and the services whose depot's AI group (or own vehicle file) has no vehicle of the route's
  mode.

## OpenStreetMap

On a Modern map, Map > From OpenStreetMap also brings:

* metro tracks (`railway=subway`) as a class of their own, built with a track type like the
  others (the kit's tram, railway and metro tracks are chosen until others are); where OSM says
  a metro runs in a tunnel, its track is built 12 m under the ground as a covered tunnel
  (Building into the ground, without ramps);
* tram stops and stations of the map's rail modes, each with the kit's station stop beside the
  nearest track, on the side the station lies; every stop of the map then becomes a station;
* the tram, trolleybus, metro and train routes as routes of `routes.cfg`: each way of a line
  through two or more of the map's stations a segment, a line's two ways that join up one
  service round them, as often as OSM's `interval` says (else every 10 minutes), 05:30 to
  23:30. Bus routes stay timetable lines (`TTData`).

A tram network's branches are cut where they meet, with no switch object: OMSI 2's turnouts
are railway ones, 27 m long and of 190 m radius, far too wide for a tram's junction, and
openOMSI takes the branch a route names (or the driver's indicator) at a plain fork.

Tried on Prague around I. P. Pavlova (9 tiles): 86 roads with 60 junctions, 15 bus stops, 11
station stops, 26 stations and 12 tram routes from OpenStreetMap; openOMSI routes 17 of their 27
trips. The others cross the dense junction at I. P. Pavlova, where the tracks as OpenStreetMap
draws them do not all join up after building: look at them in the game's log
(`transport route …: no way over the track from … to …`) and join the tracks there by hand.

## Editing together

Collaborate passes the transit network on like the map's settings: its modes and transport
folder are one part, versioned by the host; an edit somebody else made first is turned down and
kept for recovery like any other. Editors of an older version cannot join (protocol 2).
