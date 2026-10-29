# Your first map with openMapper

This walks you from an empty window to a map you can drive in service in openOMSI. It
takes about half an hour. Everything here can be undone with `Ctrl+Z`, and `Ctrl+K`
finds any command by name if you lose your way.

## 1. Start a map

1. Open openMapper. The first time it asks for your OMSI 2 folder (the one with
   `Omsi.exe`). Nothing in it is ever changed: your maps go into openOMSI's content folder.
2. **New map…** Give it a name, choose its size (3 × 3 tiles is plenty to start) and
   whether traffic keeps right or left.
   * Tick **Start with a sample** (off at first) if you would like a ring road, two stops, a line and AI
     buses ready to drive. It is a good way to see how the pieces fit together.
   * To build a real place, start without the sample and go to step 7 first.
3. The **Getting started** checklist opens. It ticks itself as the map grows; each step has
   a **Show me** button.

## 2. Roads

1. Press `2` (Roads). Pick a road type in the list on the left.
2. Click where the road starts, then click where it goes. It keeps going from there: click
   again to continue, right-click or `Esc` to stop.
   * Start on a white dot (a road end) and the new road continues smoothly from it.
   * Draw into the middle of another road, or across it, and a junction is made by itself.
   * Hold `Shift` for a straight; type a length, radius or heading in the fields at the
     view's corner and press `Enter`.
3. To change a road afterwards press `1` (Select) and click it: drag its white end dots to
   move an end, the middle dot to bend it. Double-click selects the whole road.
4. With a road selected, the properties on the right show its **height profile**: green is
   gentle, orange is steep for a bus (over 8 %), red is very steep. **Lay on the ground**
   puts it back onto the terrain after you reshape the ground.

A **roundabout** is a Roads tool option: click its centre, then its ring.

## 3. Ground

* `5` Terrain: raise, lower, smooth, flatten (to a height you type), roughen. `[` and `]`
  set the brush size.
* `6` Paint: pick a ground texture and paint. **Add…** brings in another texture. Select a
  road and use **Paint the ground along it** for a gravel verge or dirt beside a track.
* **By slope and height** (Paint panel) puts rock on steep slopes or snow on hilltops over
  the whole map in one go.
* `7` Tiles: click beside the map to add a tile. Click a tile to give it **water**, then
  lower the ground under it for a lake.

## 4. Things on the map

* `3` Objects: the catalogue on the left holds buildings, trees, signs and everything else
  OMSI 2 has (and the mods you imported). Click to place; `R` turns it. Buildings turn to
  face the nearest road by themselves.
* **Plant brush**: paint trees, or choose **In an outline** and click round an area to fill
  a wood or a park at once (roads are left clear).
* Select a road and use **Objects along the road** for street lamps, posts or fences every
  so many metres.
* Several objects selected can be turned at random, lined up along the road, or **leaned
  with the ground** on a slope.

## 5. Stops and start points

1. Press `4` (Game logic) and choose **Bus stops**. Click beside a road: the stop goes to
   the kerb of the nearest lane, and with **On both sides of the road** a twin goes across
   the road facing the other way.
2. Select a stop and type its name in the properties. Its twin takes the same name.
3. Choose **Start points** and click on a lane where the bus should appear when a drive
   starts (a depot yard, or the first stop).

4. Traffic lights from the catalogue (a signal head) light only when linked to their
   crossing: select the head, open **Linked objects** in its properties, **Link to an
   object…** and click the crossing, then choose which of its lights it shows.

## 6. A line and its timetable

1. Press `L` (Lines and timetable) and make a **New line**.
2. Add a trip and pick its stops on the map, in order. The way along the roads is found by
   itself and drawn in blue; a problem (a stop away from a lane, a gap in the roads) is
   named where it is.
3. **Make the way back** turns the trip round.
4. Under **Tours and times** add a **New tour**: departures from a first to a last time with a break. A map without a depot
   gets one, so AI buses drive the tours you do not drive yourself.
5. `T` (Traffic and AI) sets how busy the roads are over the day.

## 7. A real place

1. `I` (Satellite and terrain): search for the place and **Place the map here**. **Load satellite imagery**
   and **Load terrain heights** for all tiles.
2. **Map > From OpenStreetMap…**: tick what to bring (roads, tram and rail tracks, stops,
   bus lines, trees, water, buildings), choose a road type for each kind of road, and
   build. Everything it brings is one undo step, and a map with no start point gets one
   before a bus stop, so you can drive the town straight away.
3. Have a route recorded on your phone? **Map > GPS tracks to trace…** (or drop the GPX,
   KML or GeoJSON file on the window) shows it in the view, and can build a road along it.

## 8. Check, save, drive

1. **Map > Check map…** lists what would go wrong in the game: missing files, road ends not
   joined, stops away from a lane, trips without a way, tiles too full of objects, and more.
   Click an issue to fly there; many are put right with one more click (a stop moved to
   the kerb, a start point or a depot added, a lost junction made again).
2. **Map > Check in openOMSI…** runs the game itself on your map, unseen, with its own
   checks: lanes without road under them, buried roads, steps a bus wheel cannot climb.
3. `Ctrl+S` saves. Every save keeps the files it replaced (File > Earlier versions…), and
   your changes are autosaved every 3 minutes in case anything goes wrong.
4. `F5` saves and starts openOMSI on your map.

## 9. Share it

* **File > Export map for sharing…** packs the map and every file it uses from your content
  folder into a zip, with a readme of what else it needs. Dropping such a zip on openMapper
  installs it.
* **File > Export a plan of the map…** makes an SVG plan with the roads, the stops and every
  line in its colour, to show players where things are.
* In the map's properties (click empty ground with Select), **Import your thumbnail** takes a
  picture of your choice (cut to 370 × 280) as the one shown where maps are chosen.

## Handy to know

| Keys | What |
| --- | --- |
| `Ctrl+K` | search for any command |
| `1`–`7` | the tools |
| `F` | go to the selection |
| `Ctrl+1…9` | go to a saved view (`Ctrl+Shift+1…9` keeps the view there) |
| `M`, `B` | overview map, assets |
| `U` | ruler (distance, height, gradient) |
| `Ctrl+C / V / D` | copy, paste at the cursor, duplicate |
| `Ctrl+F` | find stops, start points or any object by name |
| `F12` | save a picture of the view |

File > Controls lists every key and mouse button, and any key can be changed there.
