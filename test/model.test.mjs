// Checks Model.js against real payloads from `rustybui bar --json` and
// `rustybui app`, without needing a running Quickshell. Run it with:
//
//   node test/model.test.mjs [bar-payload.json] [app-payload.json]
//
// With no arguments it reads the fixtures in test/fixtures/, which were captured
// from the live KNMI nowcast and the live app payload.

import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import vm from "node:vm"

const here = dirname(fileURLToPath(import.meta.url))
const modelPath = resolve(here, "../qml/Model.js")
const barPath = process.argv[2] ?? resolve(here, "fixtures/bar-dry.json")
const appPath = process.argv[3] ?? resolve(here, "fixtures/app-leiden.json")

// Model.js is plain QML-compatible JS; barColor needs Qt.rgba, which the text
// helpers never touch, so a stub is enough to load the file.
const context = vm.createContext({ Qt: { rgba: (r, g, b, a) => ({ r, g, b, a }) }, console })
vm.runInContext(readFileSync(modelPath, "utf8"), context, { filename: modelPath })
// The ramp DWD publishes for the radar layer, as the generator wrote it, so the legend the map
// draws can be checked against the labels it will be given.
const legendPath = resolve(here, "../qml/Legend.js")
vm.runInContext(readFileSync(legendPath, "utf8"), context, { filename: legendPath })

const Model = context
const payload = readFileSync(barPath, "utf8")
const appPayload = readFileSync(appPath, "utf8")

let failures = 0
function check(name, actual, expected) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected)
  if (!ok) failures++
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : `\n       expected ${JSON.stringify(expected)}\n       actual   ${JSON.stringify(actual)}`}`)
}

// ---- a live payload captured from the KNMI radar path
const dry = Model.parseView(payload)
check("parses the captured payload", dry.ok, true)
check("the fixture is a KNMI radar view", [dry.view.provider.id, dry.view.slotMinutes], ["knmi", 5])
check(
  "samples carry millisecond instants, because QML never parses timestamps",
  dry.view.points.every((point) => typeof point.timeMs === "number"),
  true
)
check(
  "the pill follows the payload's own summary",
  Model.pillText(dry.view),
  dry.view.summary.isRainingNow
    ? Model.glyphFor(dry.view) + " " + Model.shortIntensity(dry.view.summary.currentMmPerHour)
    : Model.glyphFor(dry.view)
)
check("the provider line leads with the provider", Model.providerLine(dry.view).startsWith(dry.view.provider.label), true)
check("the status line reports the source resolution", /5-min steps/.test(Model.statusLine(dry.view, true)), true)

// ---- hand-built views for the states the live feed did not show
const base = JSON.parse(payload)
base.summary = {
  isRainingNow: false, pill: "in 25m", currentMmPerHour: 0, currentClass: "dry",
  startsInMinutes: 25, startsInLabel: "25m", endsInMinutes: 65, endsInLabel: "1:05h", lastsLabel: "40m", totalMm: 1.85, peakMmPerHour: 1.2, peakInMinutes: 40
}
const soon = { ok: true, view: base }
check("rain on the way shows the wait", Model.pillText(base), "\ue318 25m")
// The same vectors as format_minutes in crates/rustybui-core/src/nowcast.rs
check("colon: minutes under an hour are minutes", Model.formatDuration(25, "colon"), "25m")
check("colon: hours plus padded minutes above", Model.formatDuration(65, "colon"), "1:05h")
check("colon: 91 minutes reads 1:31h", Model.formatDuration(91, "colon"), "1:31h")
check("colon: a whole hour keeps its zeros", Model.formatDuration(120, "colon"), "2:00h")
check("words: minutes", Model.formatDuration(45, "words"), "45 min")
check("words: hours and minutes", Model.formatDuration(91, "words"), "1 h 31 min")
check("compact: minutes", Model.formatDuration(25, "compact"), "25m")
check("compact: hours and minutes", Model.formatDuration(65, "compact"), "1h05")
check("an unknown style is the default style", Model.formatDuration(91, "nonsense"), "1:31h")
check("the payload's own label wins over the twin", Model.pillText(Object.assign({}, base, {
  summary: Object.assign({}, base.summary, { startsInLabel: "25 min" })
})), "\ue318 25 min")
check(
  "the summary reads like a sentence",
  Model.summaryLine(base, true),
  "rain in 25m · for about 40m · peak 1.2 mm/h · 1.85 mm total"
)

const raining = JSON.parse(payload)
raining.summary = {
  isRainingNow: true, pill: "2.4 mm/h", currentMmPerHour: 2.4, currentClass: "moderate",
  startsInMinutes: null, endsInMinutes: 55, endsInLabel: "55m", totalMm: 3.1, peakMmPerHour: 4.0, peakInMinutes: 20
}
check("rain now shows the rate with its unit", Model.pillText(raining), "\ue333 2.4 mm/h")
check(
  "and says when it stops",
  Model.summaryLine(raining, true),
  "2.4 mm/h now · stopping in 55m · peak 4.0 mm/h · 3.10 mm total"
)

const heavy = JSON.parse(payload)
heavy.summary = {
  isRainingNow: true, pill: "12.0 mm/h", currentMmPerHour: 12, currentClass: "very-heavy",
  startsInMinutes: null, endsInMinutes: null, totalMm: 20, peakMmPerHour: 12, peakInMinutes: 0
}
check("heavy rain earns the emphatic glyph", Model.pillText(heavy), "\ue31d 12.0 mm/h")

// ---- payload handling
check("null means the location is not covered", Model.parseView("null"), { ok: false, error: "no nowcast for this location" })
check("blank output is a failure, not a crash", Model.parseView("").ok, false)
check("garbage is reported, not thrown", Model.parseView("{oops").ok, false)
check("an empty point list is a failure", Model.parseView('{"points":[]}').ok, false)

// ---- the synthetic "rain later" fixture the render harness feeds the pill
// (test/fixtures/make-bar-wet.py rewrites the captured dry payload's rates; this keeps the
// hand-made summary consistent with the frames it ships with)
const wetFixture = Model.parseView(readFileSync(resolve(here, "fixtures/bar-wet-synthetic.json"), "utf8"))
check("the synthetic wet fixture parses", wetFixture.ok, true)
check(
  "…and its summary agrees with its own frames",
  wetFixture.view.summary.startsInMinutes,
  wetFixture.view.points.find((point) => point.mmPerHour >= 0.1).offsetMinutes
)
check("…and it draws a wait rather than a rate", Model.pillText(wetFixture.view).indexOf("\ue318") === 0, true)

// The temperature the binary formats, in whatever units are configured, sits beside the rain
// state; without one the pill is exactly as it was, and with one the word "dry" gives way to it.
check("the temperature rides beside the rain state", Model.pillText(base, "13.2°"), "\ue318 25m 13.2°")
check("and on an imperial machine it is labelled as such",
  Model.pillText(dry.view, "55.8°F"),
  Model.GLYPH.dry + " 55.8°F")
check("a dry pill with a temperature says the number, not the word",
  Model.pillText(dry.view, "13.2°").indexOf("dry"), -1)
check("with nothing to say at all it is still just the glyph", Model.pillText(dry.view), Model.GLYPH.dry)
check("and a missing temperature leaves the pill untouched", Model.pillText(base, null), "\ue318 25m")

// ---- what the pill is made of, each part switchable and the icon pickable
const parts = { showIcon: true, showState: true, showTemperature: true, iconSide: "left" }
check("the icon can be dropped", Model.pillText(base, "13.2°", { ...parts, showIcon: false }), "25m 13.2°")
check("the rain state can be dropped", Model.pillText(base, "13.2°", { ...parts, showState: false }), "\ue318 13.2°")
check("the temperature can be dropped", Model.pillText(base, "13.2°", { ...parts, showTemperature: false }), "\ue318 25m")
check("the icon can sit on the right", Model.pillText(base, "13.2°", { ...parts, iconSide: "right" }), "25m 13.2° \ue318")
check("dropping the state also moves the icon with it",
  Model.pillText(base, "13.2°", { ...parts, showState: false, iconSide: "right" }), "13.2° \ue318")
check("a dry pill with the icon off keeps its word rather than disappearing",
  Model.pillText(dry.view, null, { ...parts, showIcon: false, showState: false, showTemperature: false }),
  dry.view.summary.currentLabel)
check("and with the state off the dry icon still says dry on its own",
  Model.pillText(dry.view, "13.2°", { ...parts, showState: false }), Model.GLYPH.dry + " 13.2°")

// ---- the app payload (`rustybui app`)
const app = Model.parseApp(appPayload)
check("parses the captured app payload", app.ok, true)
check("the payload names the location", typeof app.view.location.name, "string")
check("it carries a nowcast and a forecast", [!!app.view.nowcast, !!app.view.forecast, !!app.view.air], [true, true, true])
check(
  "the current block is pre-labelled, so QML never formats numbers",
  [app.view.forecast.current.temperatureLabel, app.view.forecast.current.humidityLabel].every((label) => typeof label === "string"),
  true
)
check(
  "empty payloads are reported rather than thrown",
  [Model.parseApp("").ok, Model.parseApp("[]").ok, Model.parseApp("{}").ok],
  [false, false, false]
)

// ---- glyphs and AQI bands
check("clear night and clear day differ", [Model.conditionGlyph(0, false), Model.conditionGlyph(0, true)], ["\ue30d", "\ue32b"])
check("rain, showers and thunder have their own glyphs", [Model.conditionGlyph(63, true), Model.conditionGlyph(81, true), Model.conditionGlyph(95, true)], ["\ue318", "\ue318", "\ue31d"])
check("snow and fog too", [Model.conditionGlyph(73, true), Model.conditionGlyph(45, true)], ["\ue31a", "\ue313"])
check("an unknown code still draws something", Model.conditionGlyph(4242, true).length > 0, true)
check("AQI bands are ordered worst-last", [Model.aqiRank("good"), Model.aqiRank("very poor"), Model.aqiRank("nonsense")], [0, 4, -1])

// ---- colour handling
// A theme token is a colour object; a default palette or a plugin setting is a string, and
// building a colour out of an undefined component leaves the canvas drawing black on black.
check("a hex colour is understood", Model.withAlpha("#807060", 0.5), { r: 128 / 255, g: 112 / 255, b: 96 / 255, a: 0.5 })
check("…however short it is written", Model.withAlpha("#f80", 1), { r: 1, g: 136 / 255, b: 0, a: 1 })
check("…and so is a colour object", Model.withAlpha({ r: 0.1, g: 0.2, b: 0.3 }, 0.25), { r: 0.1, g: 0.2, b: 0.3, a: 0.25 })
check("a name QML knows and this does not is passed through, rather than dropped", Model.withAlpha("transparent", 0.5), "transparent")

// ---- chart scaling and ticks
check("the axis never collapses below half a millimetre", Model.axisTop(0), 0.5)
check("the axis rounds up to a readable step", Model.axisTop(1.2), 2)
check("and keeps up with a downpour", Model.axisTop(37), 50)
check("the next cycle is the next 5-minute boundary", Model.nextCycleMs(1760205390000, 300) % 300000, 0)
check(
  "hour ticks land on the first sample of each hour",
  Model.hourMarks([
    { timeMs: Date.UTC(2026, 9, 11, 15, 55), mmPerHour: 0 },
    { timeMs: Date.UTC(2026, 9, 11, 16, 0), mmPerHour: 0 },
    { timeMs: Date.UTC(2026, 9, 11, 16, 5), mmPerHour: 0 }
  ], true).map((mark) => mark.index),
  [0, 1]
)

// ---- the radar map
// A view is { centerLon, centerLat, spanLon, width, height }: degrees across the width, and
// the height follows from the window's own shape.
const view = Model.fitView([3.5, 51.4, 7.5, 53.6], 900, 500)
check("a fitted view is centred on its box", [view.centerLon, view.centerLat], [5.5, 52.5])
check(
  "…covers it in both directions",
  [
    Model.viewBbox(view)[0] <= 3.5, Model.viewBbox(view)[1] <= 51.4,
    Model.viewBbox(view)[2] >= 7.5, Model.viewBbox(view)[3] >= 53.6
  ],
  [true, true, true, true]
)
// The latitude extent is the tighter one here, so it is what sets the zoom: 2.2° of latitude,
// stretched by the reciprocal of the cosine, then by the window's 900/500 shape.
check("…and is only as wide as the tighter axis needs", (Model.viewBbox(view)[2] - Model.viewBbox(view)[0]).toFixed(2), "6.51")

const place = Model.project(view, 4.49, 52.16)
const roundtrip = Model.unproject(view, place.x, place.y)
check("projecting and unprojecting a place returns the place", [roundtrip.lon.toFixed(9), roundtrip.lat.toFixed(9)], ["4.490000000", "52.160000000"])

const anchor = { x: 120, y: 400 }
const anchorPlace = Model.unproject(view, anchor.x, anchor.y)
const zoomed = Model.zoomAt(view, 0.5, anchor.x, anchor.y)
const anchorAfter = Model.project(zoomed, anchorPlace.lon, anchorPlace.lat)
check("zooming keeps the place under the pointer put", [Math.round(anchorAfter.x), Math.round(anchorAfter.y)], [120, 400])
check("…and halves the span", zoomed.spanLon.toFixed(3), (view.spanLon / 2).toFixed(3))

const panned = Model.panBy(view, 100, -50)
const moved = Model.project(panned, 4.49, 52.16)
check("dragging moves the content with the pointer", [Math.round(moved.x - place.x), Math.round(moved.y - place.y)], [100, -50])

check("zooming out stops at the widest useful span", Model.zoomAt(view, 1000, 450, 250).spanLon, Model.MAX_SPAN_LON)
check("zooming in stops before the pixels run out", Model.zoomAt(view, 0.0000001, 450, 250).spanLon, Model.MIN_SPAN_LON)
const farAway = Model.panBy(view, 100000, 100000)
check("the centre cannot be dragged out of the coverage", [farAway.centerLon, farAway.centerLat], [Model.RADAR_BOUNDS[0], Model.RADAR_BOUNDS[3]])

const requested = Model.requestBbox(view)
check(
  "a request is wider than the view, so a pan has room",
  [requested[0] < Model.viewBbox(view)[0], requested[1] < Model.viewBbox(view)[1], requested[2] > Model.viewBbox(view)[2], requested[3] > Model.viewBbox(view)[3]],
  [true, true, true, true]
)
check("…stays inside what DWD publishes", [requested[0] >= Model.RADAR_BOUNDS[0], requested[1] >= Model.RADAR_BOUNDS[1], requested[2] <= Model.RADAR_BOUNDS[2], requested[3] <= Model.RADAR_BOUNDS[3]], [true, true, true, true])
check(
  "…and lands on the hundredth of a degree, so a nudge reuses the file it already has",
  requested.map((edge) => Math.round(edge * 100) / 100),
  requested
)
check(
  "…and is no wider than the margin asks for",
  requested[2] - requested[0] <= (Model.viewBbox(view)[2] - Model.viewBbox(view)[0]) * Model.IMAGE_MARGIN + 0.02,
  true
)

const held = { path: "/tmp/a.png", bbox: requested }
check("the image just fetched is not asked for again", Model.needsImage(view, held), false)
check("panning off the image asks for another", Model.needsImage(Model.panBy(view, -1800, 0), held), true)
check("zooming deep into the image asks for a sharper one", Model.needsImage(Model.zoomAt(view, 0.2, 450, 250), held), true)
check("with no image at all, one is asked for", Model.needsImage(view, null), true)
check(
  "a view that the coverage cannot answer asks for nothing, rather than looping",
  Model.needsImage({ centerLon: -10, centerLat: 0, spanLon: 2, width: 900, height: 500 }, held),
  false
)

const bar = Model.scaleBar(view)
check("the scale bar is a round number of kilometres", Model.SCALE_STEPS.indexOf(bar.km) !== -1, true)
check("…that spans a readable distance on screen", bar.pixels >= Model.SCALE_MIN_PIXELS && bar.pixels <= Model.SCALE_MAX_PIXELS, true)
check("zooming in shrinks what a pixel covers", Model.kmPerPixel(Model.zoomAt(view, 0.5, 0, 0)) < Model.kmPerPixel(view), true)

const size = Model.imageSize(requested)
const wantedAspect = (requested[3] - requested[1]) / Math.cos((requested[1] + requested[3]) / 2 * Math.PI / 180) / (requested[2] - requested[0])
check("an image is asked for at about a thousand pixels across", size.width, 1000)
check("…in the shape of the box, not stretched", Math.abs(size.height / size.width - wantedAspect) < 0.01, true)
check("a very tall request is capped rather than answered with a wall of pixels", Model.imageSize([5, 44, 5.4, 56]).height, 1400)
check("…and a deep zoom still comes back with more than a thumbnail", [Model.imageSize([5, 52, 5.005, 52.005]).width >= 320, Model.imageSize([5, 52, 5.005, 52.005]).height >= 240], [true, true])

const frames = [1000, 2000, 6000, 3000]
check("frames are stepped oldest first", Model.frameTimes(frames), [1000, 2000, 3000, 6000])
check("the map opens on the frame nearest now", Model.nowFrameIndex(frames, 2900), 2)
check("…not on the newest, which is a forecast", Model.nowFrameIndex(frames, 6500), 3)
check("an empty frame list has no frame to point at", Model.nowFrameIndex([], 0), -1)

// ---- the basemap the map draws under the radar
// The vendored Natural Earth vectors, projected into screen coordinates the way the map does
// it: this is the layer the offscreen renderer cannot photograph, so its geometry is checked
// here instead — a path built for the wrong view, or for a shape with no size, would draw
// nothing at all and look exactly like a map that is simply quiet.
const basemap = vm.createContext({ console })
vm.runInContext(readFileSync(resolve(here, "../qml/Basemap.js"), "utf8"), basemap, { filename: "Basemap.js" })
check("the vendored basemap has coastlines and borders", [basemap.land.length > 20, basemap.borders.length > 20], [true, true])

const mapView = Model.clampCenter({ centerLon: 4.49, centerLat: 52.16, spanLon: 2.6, width: 596, height: 370 })
const paths = Model.svgPath(mapView, basemap.land, 596, 370)
check("a Netherlands-sized view draws the coast through it", Model.drawnRuns(paths) >= 2, true)
check(
  "…as runs of two points or more, never a lone dot",
  paths.split(" M ").every((run) => run.indexOf(" L ") !== -1),
  true
)
check("a view of half of Europe draws a lot more", Model.drawnRuns(Model.svgPath(Model.fitView([2, 48, 12, 55], 596, 370), basemap.land, 596, 370)) > 10, true)
check(
  "the coast is drawn where the coast is",
  (function() {
    // The drawn path against the place geography puts the coast — Katwijk, 52.20°N 4.40°E.
    // They must meet within a few pixels, or the projection is not pointing where it claims.
    const tokens = paths.split(" ")
    const expected = Model.project(mapView, 4.4, 52.2)
    let best = Infinity
    for (let i = 0; i + 2 < tokens.length; i += 1) {
      if (tokens[i] !== "L") continue
      best = Math.min(best, Math.hypot(Number(tokens[i + 1]) - expected.x, Number(tokens[i + 2]) - expected.y))
    }
    return [best < 20, Model.project(mapView, 4.4, 52.2).x < Model.project(mapView, 4.49, 52.16).x]
  })(),
  [true, true]
)
check(
  "a view that is nowhere near the data draws nothing, rather than everything",
  Model.drawnRuns(Model.svgPath({ centerLon: -60, centerLat: -30, spanLon: 4, width: 596, height: 370 }, basemap.land, 596, 370)),
  0
)
check(
  "a shape with no size yet draws nothing",
  Model.svgPath(view, basemap.land, 0, 0).length,
  0
)

// ---- the moon on screen
// A disc built from two arcs, so the phase is drawn rather than looked up in a font.
check("a new moon draws nothing", Model.moonPath(0, true, 14), "")
check("a full moon is a closed circle", Model.moonPath(1, true, 14), "M 0 -14 A 14 14 0 1 1 0 14 A 14 14 0 1 1 0 -14 Z")
check("a half moon is a straight terminator", Model.moonPath(0.5, true, 14).indexOf("A 0 14 0 0") !== -1, true)
check(
  "a waxing crescent bulges towards the right limb",
  Model.moonPath(0.25, true, 14),
  "M 0 -14 A 14 14 0 0 1 0 14 A 7 14 0 0 0 0 -14 Z"
)
check(
  "a waxing gibbous bulges away from it",
  Model.moonPath(0.75, true, 14),
  "M 0 -14 A 14 14 0 0 1 0 14 A 7 14 0 0 1 0 -14 Z"
)
check(
  "and a waning moon is the same shape mirrored",
  [Model.moonPath(0.25, false, 14), Model.moonPath(0.75, false, 14)],
  [
    "M 0 -14 A 14 14 0 0 0 0 14 A 7 14 0 0 1 0 -14 Z",
    "M 0 -14 A 14 14 0 0 0 0 14 A 7 14 0 0 0 0 -14 Z"
  ]
)
check("nonsense illumination is clamped rather than drawn", [Model.moonPath(-1, true, 14), Model.moonPath(2, true, 14).indexOf("1 1") !== -1], ["", true])
check("a disc with no size draws nothing", Model.moonPath(0.5, true, 0), "")

// ---- the legend and the frame clock
// The ramp is DWD's, generated from the legend their service publishes; what the map does with
// it is say it the way a person reads it, and say when the frame on screen is.
check("the ramp is DWD's own, in their colours", [Model.RAMP.length, Model.RAMP[0].color, Model.RAMP[Model.RAMP.length - 1].color],
  [15, "#33FFFF", "#0000FE"])
check("and its bands read as ranges, not as code",
  [Model.rampLabel("[0.1 - 0.2)"), Model.rampLabel("[ 7.5 - 10 )"), Model.rampLabel(">= 150"), Model.rampLabel("Keine Daten")],
  ["0.1–0.2", "7.5–10", "≥150", "Keine Daten"])
check("the ramp splits into two columns of eight and seven",
  Model.evenColumns(Model.RAMP, 2).map(column => column.length),
  [8, 7])
check("with every band in exactly one column",
  Model.evenColumns(Model.RAMP, 2).flat().length,
  Model.RAMP.length)

const now = 1791741600000 // 2026-10-11T18:00:00Z
check("a frame in the future is ahead of us", Model.frameAge(now + 20 * 60000, now), "in 20m")
check("a frame in the past was ago", Model.frameAge(now - 35 * 60000, now), "35m ago")
check("the frame on screen is now", Model.frameAge(now, now), "now")
check("half a minute rounds to now rather than to nothing", Model.frameAge(now + 25000, now), "now")
check("an hour of nowcast reads as hours and minutes",
  [Model.frameAge(now + 65 * 60000, now), Model.frameAge(now - 125 * 60000, now)],
  ["in 1:05h", "2:05h ago"])
check("and the style is the user's", Model.frameAge(now + 65 * 60000, now, "words"), "in 1 h 05 min")
check("and a frame with no time says nothing", Model.frameAge(null, now), "")

// ---- the loop, and what the label is allowed to claim
// The map plays on its own, and steps only when the picture has caught up — these are the two
// helpers that decide which frame the stepper may name.
const framesMs = [0, 300000, 600000, 900000].map(minutes => now + minutes)
check("a picture is placed at its own frame", Model.frameIndexAt(framesMs, framesMs[2]), 2)
check("the oldest frame is frame nought", Model.frameIndexAt(framesMs, framesMs[0]), 0)
check("a picture from no known frame is not placed at all",
  [Model.frameIndexAt(framesMs, now + 450000), Model.frameIndexAt(framesMs, null), Model.frameIndexAt([], now)],
  [-1, -1, -1])
check("and an unordered frame list is matched by time, not by position",
  Model.frameIndexAt([framesMs[3], framesMs[1], framesMs[0], framesMs[2]], framesMs[3]),
  3)

console.log(failures === 0 ? "\nall model checks passed" : `\n${failures} check(s) failed`)
process.exit(failures === 0 ? 0 : 1)
