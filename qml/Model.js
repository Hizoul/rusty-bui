// Pure helpers for the rain-nowcast plugin.
//
// Deliberately free of QML types (except Qt.rgba, used only by barColor, which
// the node test never calls) so the payload handling can be exercised outside a
// running shell: see omarchy/test/model.test.mjs.

// Nerd-font glyphs, the same set Omarchy's own weather widget uses, so the pill
// matches the bar it sits in.
var GLYPH = {
  heavy: "\ue31d", // thunderstorm — reserved for genuinely heavy rates
  rain: "\ue318", // rain
  light: "\ue333", // showers / light rain
  dry: "\ue33d", // cloudy — nothing expected
  setup: "\uf041", // map marker — no location stored yet, the pill opens the set-up popout
  loading: "\uf110" // spinner — location set, first payload still on its way
}

// Parses stdout of `rustybui bar --json`. `null` is the binary's way of saying
// "no nowcast covers this location".
function parseView(raw) {
  var text = String(raw === undefined || raw === null ? "" : raw).replace(/^\s+|\s+$/g, "")
  if (text === "" || text === "null") return { ok: false, error: "no nowcast for this location" }

  var view = null
  try {
    view = JSON.parse(text)
  } catch (e) {
    return { ok: false, error: "unreadable nowcast payload" }
  }

  // Fresh install: no location stored yet. Not an error, a state the popout sets up.
  if (view && view.needsLocation === true)
    return { ok: false, needsLocation: true, error: "no location set" }

  if (!view || !view.points || view.points.length === 0)
    return { ok: false, error: "no nowcast samples returned" }

  if (!view.summary) view.summary = {}
  return { ok: true, view: view, needsLocation: false, error: "" }
}

function shortIntensity(mmPerHour) {
  var value = Number(mmPerHour)
  if (!isFinite(value)) return "-"
  return value < 1 ? value.toFixed(2) : value.toFixed(1)
}

function glyphFor(view) {
  if (!view || !view.summary) return GLYPH.dry
  var summary = view.summary
  if (summary.isRainingNow) {
    var rate = Number(summary.currentMmPerHour)
    if (rate >= 10) return GLYPH.heavy
    if (rate >= 3) return GLYPH.rain
    return GLYPH.light
  }
  if (isSet(summary.startsInMinutes)) return GLYPH.rain
  return GLYPH.dry
}

/// The pill: the rain state, the temperature, and the icon, each optional and arranged by
/// `options` — `{ showIcon, showState, showTemperature, iconSide }`. The state is the rain
/// number ("25m", "2.4"); the icon is the weather glyph and the only thing that can carry
/// "dry" on its own, so the word stands in when every part is turned off. The temperature is
/// the binary's own label, so its units are already whatever the machine is set to.
function pillText(view, temperatureLabel, options) {
  options = options || {}
  var showIcon = options.showIcon !== false
  var showState = options.showState !== false
  var showTemperature = options.showTemperature !== false
  var iconRight = options.iconSide === "right"

  var degrees = temperatureLabel ? String(temperatureLabel) : ""
  var hasView = !!(view && view.summary)
  var summary = hasView ? view.summary : null
  var glyph = hasView ? glyphFor(view) : GLYPH.dry

  // Dry has no rate and no wait: the icon is what says "dry", so there is no state text.
  var state = ""
  if (hasView && showState) {
    if (summary.isRainingNow) state = shortIntensity(summary.currentMmPerHour) + " mm/h"
    else if (isSet(summary.startsInMinutes)) state = labelOrDuration(summary.startsInLabel, summary.startsInMinutes)
  }

  var icon = showIcon ? glyph : ""
  var temp = showTemperature ? degrees : ""

  var parts = iconRight ? [state, temp, icon] : [icon, state, temp]
  var text = parts
    .filter(function(part) { return part !== "" })
    .join(" ")

  // Turning every part off must not wipe the pill out of the bar: the word the summary already
  // carries ("dry") is the one thing that can still be said, and it keeps the widget clickable.
  if (text === "") text = hasView && summary.currentLabel ? String(summary.currentLabel) : "dry"
  return text
}

/// A span of minutes in the user's duration style — "colon" (25m, 1:31h), "words" (25 min,
/// 1 h 31 min) or "compact" (25m, 1h31). The nowcast payload arrives with its labels already
/// written by Rust (`format_minutes` in nowcast.rs); this is the one client-side twin, for the
/// spans only the client can compute (a radar frame's distance from now), and the tests hold
/// the two to the same vectors.
function formatDuration(minutes, style) {
  var value = Math.max(0, Math.round(Number(minutes)))
  var hours = Math.floor(value / 60)
  var rest = value % 60
  var padded = (rest < 10 ? "0" : "") + rest
  if (style === "words") return hours === 0 ? rest + " min" : hours + " h " + padded + " min"
  if (style === "compact") return hours === 0 ? rest + "m" : hours + "h" + padded
  return hours === 0 ? rest + "m" : hours + ":" + padded + "h"
}

/// A summary's wait, as the binary wrote it; the twin only fills in for a payload that predates
/// the label.
function labelOrDuration(label, minutes) {
  return isSet(label) && label !== "" ? String(label) : formatDuration(minutes, "colon")
}

function formatClock(epochMillis, clock24) {
  var date = new Date(Number(epochMillis))
  if (clock24 === false) {
    var hours = date.getHours()
    var suffix = hours >= 12 ? "PM" : "AM"
    var twelve = hours % 12
    if (twelve === 0) twelve = 12
    return twelve + ":" + pad(date.getMinutes()) + " " + suffix
  }
  return pad(date.getHours()) + ":" + pad(date.getMinutes())
}

function pad(value) {
  return (value < 10 ? "0" : "") + value
}

// `dry now · rain in 25m · peak 1.2 mm/h · 1.85 mm total`
function summaryLine(view, clock24) {
  if (!view || !view.summary) return ""
  var summary = view.summary
  var parts = []

  if (summary.isRainingNow) {
    parts.push(shortIntensity(summary.currentMmPerHour) + " mm/h now")
    if (isSet(summary.endsInMinutes)) parts.push("stopping in " + labelOrDuration(summary.endsInLabel, summary.endsInMinutes))
  } else if (isSet(summary.startsInMinutes)) {
    parts.push("rain in " + labelOrDuration(summary.startsInLabel, summary.startsInMinutes))
    if (isSet(summary.endsInMinutes) && Number(summary.endsInMinutes) > Number(summary.startsInMinutes))
      parts.push("for about " + labelOrDuration(summary.lastsLabel,
        Number(summary.endsInMinutes) - Number(summary.startsInMinutes)))
  } else {
    parts.push("no rain expected")
  }

  if (Number(summary.peakMmPerHour) > 0) parts.push("peak " + shortIntensity(summary.peakMmPerHour) + " mm/h")
  if (Number(summary.totalMm) > 0) parts.push(Number(summary.totalMm).toFixed(2) + " mm total")
  return parts.join(" · ")
}

// `KNMI · Radar © KNMI`
function providerLine(view) {
  if (!view || !view.provider) return ""
  var label = String(view.provider.label || "")
  var attribution = String(view.provider.attribution || "")
  return attribution === "" ? label : label + " · " + attribution
}

function locationLine(view) {
  if (!view || !view.location) return ""
  return String(view.location.name || "")
}

// `updated 15:56 · next cycle 16:00`
function statusLine(view, clock24) {
  if (!view) return ""
  var updated = isSet(view.fetchedAtMs) ? formatClock(view.fetchedAtMs, clock24) : ""
  var next = isSet(view.nowMs) ? nextCycleMs(view.nowMs) : 0
  var parts = []
  if (updated !== "") parts.push("updated " + updated)
  if (next > 0) parts.push("next cycle " + formatClock(next, clock24))
  parts.push(String(view.slotMinutes || 5) + "-min steps")
  return parts.join(" · ")
}

// One label per hour boundary inside the window, with the index of the first
// sample in that hour, so the chart can draw ticks without duplicating logic.
function hourMarks(points, clock24) {
  var marks = []
  if (!points || !points.length) return marks
  var previousHour = -1
  for (var i = 0; i < points.length; i++) {
    var date = new Date(Number(points[i].timeMs))
    var hour = date.getHours()
    if (hour !== previousHour) {
      marks.push({ index: i, label: pad(hour) + (clock24 === false ? "" : ":00") })
      previousHour = hour
    }
  }
  return marks
}

// Axis top: a readable step above the peak, never below 0.5 mm/h.
function axisTop(mmPerHour) {
  var peak = Number(mmPerHour)
  if (!isFinite(peak) || peak <= 0) return 0.5
  var steps = [0.5, 1, 2, 2.5, 5, 7.5, 10, 15, 20, 30, 50, 75, 100]
  for (var i = 0; i < steps.length; i++) if (peak <= steps[i]) return steps[i]
  return Math.ceil(peak / 50) * 50
}

function nextCycleMs(nowMs, cycleSeconds) {
  var cycle = (cycleSeconds || 300) * 1000
  return Math.ceil(Number(nowMs) / cycle) * cycle
}

// Bars: accent while it rains lightly, urgent once it is heavy, faint when dry.
// The palette is passed in from QML (Commons.Color), so the chart follows the
// active Omarchy theme instead of hard-coding a buienradar ramp.
function barColor(cls, palette) {
  var base = palette.foreground
  if (cls === "drizzle" || cls === "light" || cls === "moderate") base = palette.accent
  else if (cls === "heavy" || cls === "very-heavy" || cls === "extreme") base = palette.urgent
  return withAlpha(base, classAlpha(cls))
}

function classAlpha(cls) {
  if (cls === "dry") return 0.18
  if (cls === "drizzle") return 0.45
  if (cls === "light") return 0.65
  if (cls === "moderate") return 0.85
  return 1.0
}

function withAlpha(color, alpha) {
  var rgb = typeof color === "string" ? parseColor(color) : color
  // A colour the canvas cannot make sense of is dropped without a word — the stroke stays
  // black and the map goes invisible — so a string is parsed rather than passed through.
  if (!rgb || rgb.r === undefined) return color
  return Qt.rgba(rgb.r, rgb.g, rgb.b, alpha)
}

/// `#rgb`, `#rrggbb`, and either with an alpha byte, as 0–1 components. Theme tokens arrive
/// as colours, but a hand-written palette in a default or a plugin setting is a string.
function parseColor(text) {
  var hex = String(text).replace(/^\s+|\s+$/g, "")
  if (hex.charAt(0) !== "#") return null
  hex = hex.slice(1)
  if (hex.length === 3 || hex.length === 4)
    hex = hex.charAt(0) + hex.charAt(0) + hex.charAt(1) + hex.charAt(1) + hex.charAt(2) + hex.charAt(2)
  if (hex.length !== 6 && hex.length !== 8) return null
  var value = parseInt(hex.slice(0, 6), 16)
  if (!isFinite(value)) return null
  return { r: ((value >> 16) & 255) / 255, g: ((value >> 8) & 255) / 255, b: (value & 255) / 255 }
}

// ---- the app view (`rustybui app`) -----------------------------------------

// Parses one app payload — the whole screen in one process spawn.
function parseApp(raw) {
  var text = String(raw === undefined || raw === null ? "" : raw).replace(/^\s+|\s+$/g, "")
  if (text === "") return { ok: false, error: "rustybui returned nothing" }

  var view = null
  try {
    view = JSON.parse(text)
  } catch (e) {
    return { ok: false, error: "unreadable app payload" }
  }

  if (!view || !view.location) return { ok: false, error: "app payload has no location" }
  return { ok: true, view: view, error: "" }
}

// WMO weather code → glyph from the same nerd-font set the bar's weather pill
// uses, so the window and the bar agree at a glance.
function conditionGlyph(code, daylight) {
  var value = Number(code)
  if (!isFinite(value)) return "\ue33d"
  if (value === 0) return daylight === false ? "\ue30d" : "\ue32b"
  if (value === 1 || value === 2) return "\ue302"
  if (value === 3) return "\ue33d"
  if (value === 45 || value === 48) return "\ue313"
  if (value >= 51 && value <= 57) return "\ue333"
  if (value === 66 || value === 67) return "\ue3ad"
  if (value >= 71 && value <= 77) return "\ue31a"
  if (value >= 61 && value <= 65) return "\ue318"
  if (value >= 80 && value <= 82) return "\ue318"
  if (value === 85 || value === 86) return "\ue31a"
  if (value >= 95) return "\ue31d"
  return "\ue33d"
}

// Open-Meteo's European AQI bands, ordered worst-last, for colouring a badge.
function aqiRank(band) {
  if (band === "good") return 0
  if (band === "fair") return 1
  if (band === "moderate") return 2
  if (band === "poor") return 3
  if (band === "very poor") return 4
  if (band === "extremely poor") return 5
  return -1
}

function aqiColor(band, palette) {
  var rank = aqiRank(band)
  if (rank < 0) return palette.foreground
  if (rank <= 1) return palette.accent
  if (rank <= 3) return palette.warning || palette.urgent
  return palette.urgent
}

function isSet(value) {
  return value !== null && value !== undefined
}

// ---- the radar map ---------------------------------------------------------

// How much of the world DWD's composite covers. Outside it there is no radar to ask for, so
// the map shows its basemap and says so instead of fetching and failing.
var RADAR_BOUNDS = [1.46, 45.68, 18.73, 56.22]

// What the credit line says when no payload has said it yet.
var RADAR_ATTRIBUTION = "Radar © DWD"

var MIN_SPAN_LON = 0.15
var MAX_SPAN_LON = 22
// The image we ask for is this much wider than the view, so a pan has room to move in.
var IMAGE_MARGIN = 1.35
// …and it is fetched again once the view is this small a part of it, because by then the
// pixels on screen are the image's own pixels rather than an interpolation of them.
var DEEP_ZOOM = 0.34
// Request boxes snap outward to a hundredth of a degree, so a small pan reuses the file the
// last one wrote instead of asking the service for the same picture again.
var SNAP = 100
// A scale bar wants a round number of kilometres between this many pixels wide and that many.
var SCALE_MIN_PIXELS = 40
var SCALE_MAX_PIXELS = 140
var SCALE_STEPS = [1, 2, 5, 10, 20, 50, 100, 200, 500]

function clampNumber(value, low, high) {
  return value < low ? low : value > high ? high : value
}

function clampSpan(span) {
  return clampNumber(Number(span), MIN_SPAN_LON, MAX_SPAN_LON)
}

// Equirectangular, with the latitude scale divided by the cosine of the view's own latitude,
// so a degree of latitude is not drawn the same length as a degree of longitude. Over a few
// hundred kilometres — which is the whole of this map's business — that is close enough to
// right, and it keeps the projection to a division and a multiplication.
function pxPerDegree(view) {
  var perLon = Number(view.width) / clampSpan(view.spanLon)
  var latitude = clampNumber(Number(view.centerLat), -85, 85)
  return { lon: perLon, lat: perLon / Math.max(0.05, Math.cos(latitude * Math.PI / 180)) }
}

function project(view, lon, lat) {
  var scale = pxPerDegree(view)
  return {
    x: Number(view.width) / 2 + (Number(lon) - Number(view.centerLon)) * scale.lon,
    y: Number(view.height) / 2 - (Number(lat) - Number(view.centerLat)) * scale.lat
  }
}

function unproject(view, x, y) {
  var scale = pxPerDegree(view)
  return {
    lon: Number(view.centerLon) + (Number(x) - Number(view.width) / 2) / scale.lon,
    lat: Number(view.centerLat) - (Number(y) - Number(view.height) / 2) / scale.lat
  }
}

// `[minLon, minLat, maxLon, maxLat]` — the same order the radar command takes.
function viewBbox(view) {
  var scale = pxPerDegree(view)
  var halfLon = Number(view.width) / 2 / scale.lon
  var halfLat = Number(view.height) / 2 / scale.lat
  return [
    Number(view.centerLon) - halfLon,
    Number(view.centerLat) - halfLat,
    Number(view.centerLon) + halfLon,
    Number(view.centerLat) + halfLat
  ]
}

/// The view that shows `bbox` whole, centred, in a `width`×`height` window.
function fitView(bbox, width, height) {
  var centerLon = (Number(bbox[0]) + Number(bbox[2])) / 2
  var centerLat = (Number(bbox[1]) + Number(bbox[3])) / 2
  var scale = Math.max(0.05, Math.cos(clampNumber(centerLat, -85, 85) * Math.PI / 180))
  // Fit both axes; the tighter one decides.
  var forWidth = Number(bbox[2]) - Number(bbox[0])
  var forHeight = (Number(bbox[3]) - Number(bbox[1])) / scale * (Number(width) / Number(height))
  return {
    centerLon: centerLon,
    centerLat: centerLat,
    spanLon: clampSpan(Math.max(forWidth, forHeight)),
    width: Number(width),
    height: Number(height)
  }
}

/// The box the view wants fetched: a margin wider than the view, inside what DWD publishes,
/// snapped outward so that nudging the map does not re-request it.
function requestBbox(view) {
  var box = viewBbox(view)
  var halfLon = Math.max((box[2] - box[0]) * IMAGE_MARGIN / 2, MIN_SPAN_LON)
  var halfLat = Math.max((box[3] - box[1]) * IMAGE_MARGIN / 2, MIN_SPAN_LON / 2)
  var centerLon = (box[0] + box[2]) / 2
  var centerLat = (box[1] + box[3]) / 2
  return [
    snapDown(Math.max(RADAR_BOUNDS[0], centerLon - halfLon)),
    snapDown(Math.max(RADAR_BOUNDS[1], centerLat - halfLat)),
    snapUp(Math.min(RADAR_BOUNDS[2], centerLon + halfLon)),
    snapUp(Math.min(RADAR_BOUNDS[3], centerLat + halfLat))
  ]
}

function snapDown(value) {
  return Math.floor(Number(value) * SNAP) / SNAP
}

function snapUp(value) {
  return Math.ceil(Number(value) * SNAP) / SNAP
}

/// Whether what is on screen needs a newly fetched image: either the view has run off the
/// one we hold, or it has zoomed so far into it that its pixels are being stretched.
///
/// Both sides are clipped to DWD's coverage first, because a view that is partly outside it
/// can never be covered, and asking again would not change that.
function needsImage(view, image) {
  if (!image || !image.bbox || !image.path) return true
  var visible = clipBbox(viewBbox(view))
  if (!visible) return false
  var covered = clipBbox(image.bbox)
  if (!covered) return true
  if (!containsBbox(covered, visible)) return true
  return visible[2] - visible[0] < (covered[2] - covered[0]) * DEEP_ZOOM
}

/// The part of `box` that DWD's composite covers, or `null` when it covers none of it.
function clipBbox(box) {
  var clipped = [
    Math.max(Number(box[0]), RADAR_BOUNDS[0]),
    Math.max(Number(box[1]), RADAR_BOUNDS[1]),
    Math.min(Number(box[2]), RADAR_BOUNDS[2]),
    Math.min(Number(box[3]), RADAR_BOUNDS[3])
  ]
  return clipped[0] >= clipped[2] || clipped[1] >= clipped[3] ? null : clipped
}

function containsBbox(outer, inner) {
  return inner[0] >= outer[0] && inner[1] >= outer[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

/// Drag the map by a pixel delta: the content follows the pointer, so the view goes the
/// other way.
function panBy(view, dx, dy) {
  var scale = pxPerDegree(view)
  return clampCenter({
    centerLon: Number(view.centerLon) - Number(dx) / scale.lon,
    centerLat: Number(view.centerLat) + Number(dy) / scale.lat,
    spanLon: view.spanLon,
    width: view.width,
    height: view.height
  })
}

/// Zoom about a point on screen, so whatever is under the pointer stays under it.
function zoomAt(view, factor, anchorX, anchorY) {
  var spanLon = clampSpan(Number(view.spanLon) * Number(factor))
  var before = unproject(view, anchorX, anchorY)
  var next = {
    centerLon: Number(view.centerLon),
    centerLat: Number(view.centerLat),
    spanLon: spanLon,
    width: Number(view.width),
    height: Number(view.height)
  }
  // Twice, because the latitude scale is itself a function of the centre being moved: one
  // pass leaves the anchor a pixel off where it started.
  for (var pass = 0; pass < 2; pass++) {
    var after = unproject(next, anchorX, anchorY)
    next = {
      centerLon: next.centerLon + (before.lon - after.lon),
      centerLat: next.centerLat + (before.lat - after.lat),
      spanLon: spanLon,
      width: Number(view.width),
      height: Number(view.height)
    }
  }
  return clampCenter(next)
}

/// The centre stays inside the coverage, so the map cannot be panned off into an empty
/// ocean of basemap with no way back.
function clampCenter(view) {
  return {
    centerLon: clampNumber(Number(view.centerLon), RADAR_BOUNDS[0], RADAR_BOUNDS[2]),
    centerLat: clampNumber(Number(view.centerLat), RADAR_BOUNDS[1], RADAR_BOUNDS[3]),
    spanLon: clampSpan(view.spanLon),
    width: Number(view.width),
    height: Number(view.height)
  }
}

function kmPerPixel(view) {
  var scale = pxPerDegree(view)
  var latitude = clampNumber(Number(view.centerLat), -85, 85)
  return 111.32 * Math.max(0.05, Math.cos(latitude * Math.PI / 180)) / scale.lon
}

/// A round distance to draw beside the map, with the pixels it takes up at this zoom.
function scaleBar(view) {
  var perPixel = kmPerPixel(view)
  for (var i = 0; i < SCALE_STEPS.length; i++) {
    var pixels = SCALE_STEPS[i] / perPixel
    if (pixels >= SCALE_MIN_PIXELS && pixels <= SCALE_MAX_PIXELS)
      return { km: SCALE_STEPS[i], pixels: Math.round(pixels) }
  }
  var last = SCALE_STEPS[SCALE_STEPS.length - 1]
  return { km: last, pixels: Math.round(last / perPixel) }
}

/// One SVG path string for a whole layer of the basemap, in screen coordinates: `M x y L …`,
/// with each ring a subpath and any segment that misses the window dropped.
///
/// Segments rather than points, because at 1:110m a coastline's points are hundreds of
/// kilometres apart: the segment that crosses the whole window can perfectly well have both
/// its ends outside it, and dropping it by point would leave the coast of the Netherlands
/// undrawn on a map of the Netherlands.
///
/// A `PathSvg` consumes this, which keeps the basemap a geometry node rather than a texture.
function svgPath(view, lines, width, height, pad) {
  var margin = isSet(pad) ? Number(pad) : 2
  var right = Number(width) + margin
  var bottom = Number(height) + margin
  var parts = []

  for (var i = 0; i < lines.length; i++) {
    var line = lines[i]
    var pen = false
    var previous = null
    for (var j = 0; j < line.length; j++) {
      var at = project(view, line[j][0], line[j][1])
      var from = previous || at
      // The segment's bounding box against the window: a coarse line is kept whole, off-screen
      // parts and all, rather than being broken at every point outside.
      var visible = Math.min(from.x, at.x) <= right && Math.max(from.x, at.x) >= -margin
        && Math.min(from.y, at.y) <= bottom && Math.max(from.y, at.y) >= -margin
      if (visible) {
        if (!pen) parts.push("M " + round1(from.x) + " " + round1(from.y))
        parts.push("L " + round1(at.x) + " " + round1(at.y))
        pen = true
      } else {
        pen = false
      }
      previous = at
    }
  }
  return parts.join(" ")
}

function round1(value) {
  return Math.round(Number(value) * 10) / 10
}

/// An SVG path for the moon at this illumination, centred on the origin, of the given radius.
///
/// A semicircle for the lit limb and an ellipse for the terminator: the terminator's semi-minor
/// axis is the radius times |cos of the phase angle|, which is the radius at new and full, a
/// straight line at the quarters, and bulges towards the lit limb while the moon is a crescent
/// and away from it once it is gibbous. That is the difference between a sliver and a disc with
/// a bite out of it, and it is why this is geometry rather than a glyph.
function moonPath(illumination, waxing, radius) {
  var r = Number(radius)
  var lit = clampNumber(Number(illumination), 0, 1)
  if (!(r > 0) || lit <= 0.005) return ""
  var circle = "M 0 " + -r + " A " + r + " " + r + " 0 1 1 0 " + r + " A " + r + " " + r + " 0 1 1 0 " + -r + " Z"
  if (lit >= 0.995) return circle

  // The lit limb, then the terminator back the other way.
  var limb = waxing ? 1 : 0
  var terminator = (waxing === (lit >= 0.5)) ? 1 : 0
  var axis = r * Math.abs(1 - 2 * lit)
  return "M 0 " + -r
    + " A " + r + " " + r + " 0 0 " + limb + " 0 " + r
    + " A " + round1(axis) + " " + r + " 0 0 " + terminator + " 0 " + -r
    + " Z"
}

/// How many runs a path actually draws: one per ring that crosses the view, counting only
/// runs of two points or more, since a lone point draws nothing. The tests use it to tell a
/// basemap that is being drawn from one that is quietly empty.
function drawnRuns(path) {
  var runs = String(path).split(" M ")
  var drawn = 0
  for (var i = 0; i < runs.length; i++) if (runs[i].indexOf(" L ") !== -1) drawn += 1
  return drawn
}

/// The pixel size to ask the service for: about a thousand across, in the box's own shape,
/// and capped — a wide request answered with a wall of pixels is slow to fetch, and one
/// answered with a postage stamp is not worth zooming into.
function imageSize(box) {
  var lonSpan = Math.max(0.01, Number(box[2]) - Number(box[0]))
  var centerLat = (Number(box[1]) + Number(box[3])) / 2
  var scale = Math.max(0.05, Math.cos(clampNumber(centerLat, -85, 85) * Math.PI / 180))
  var latSpan = Math.max(0.01, (Number(box[3]) - Number(box[1])) / scale)
  var width = 1000
  var height = Math.round(width * latSpan / lonSpan)
  if (height > 1400) {
    height = 1400
    width = Math.round(height * lonSpan / latSpan)
  }
  return { width: Math.max(320, width), height: Math.max(240, height) }
}

/// The frames worth stepping through: DWD's extent reaches hours into the future, and the
/// part of it that has already happened is the part worth replaying.
function frameTimes(framesMs) {
  if (!framesMs || !framesMs.length) return []
  return framesMs.slice().sort(function(a, b) { return Number(a) - Number(b) })
}

/// The frame nearest now, which is the one a map should open on: the extent is a nowcast, so
/// its last frame is a forecast.
function nowFrameIndex(framesMs, nowMs) {
  var times = frameTimes(framesMs)
  if (!times.length) return -1
  var at = isSet(nowMs) ? Number(nowMs) : Date.now()
  var best = 0
  for (var i = 1; i < times.length; i++)
    if (Math.abs(Number(times[i]) - at) < Math.abs(Number(times[best]) - at)) best = i
  return best
}

/// Which of the frames a picture of that instant is: the index the map should be labelling. A
/// payload carries one image at a time, so while an animation steps faster than the service
/// answers, the picture on screen is of an earlier frame — and a label taken from the frame the
/// map *asked* for would put a time on a picture it does not belong to.
function frameIndexAt(framesMs, timeMs) {
  var times = frameTimes(framesMs)
  if (!times.length || !isSet(timeMs)) return -1
  var at = Number(timeMs)
  for (var i = 0; i < times.length; i++) if (Number(times[i]) === at) return i
  return -1
}

/// How far off that frame is, said the way the rest of the app says it: "in 20m", "35m ago",
/// "now". The stepper's clock already says which frame is on screen; this is the part a person
/// actually reads, and without it a nowcast frame looks like any other timestamp.
function frameAge(frameMs, nowMs, style) {
  if (!isSet(frameMs)) return ""
  var at = isSet(nowMs) ? Number(nowMs) : Date.now()
  var minutes = Math.round((Number(frameMs) - at) / 60000)
  if (Math.abs(minutes) < 1) return "now"
  var size = formatDuration(Math.abs(minutes), style)
  return minutes > 0 ? "in " + size : size + " ago"
}

/// DWD states its bands as half-open intervals — "[0.1 - 0.2)", ">= 150" — which is precise and
/// reads like code beside a map. The same thing, said the way a person would say it.
function rampLabel(label) {
  var text = String(label === undefined || label === null ? "" : label).replace(/^\s+|\s+$/g, "")
  var range = text.match(/^\[\s*([0-9.]+)\s*-\s*([0-9.]+)\s*\)$/)
  if (range) return range[1] + "–" + range[2]
  var open = text.match(/^>=\s*([0-9.]+)$/)
  if (open) return "≥" + open[1]
  return text
}

/// A legend's entries split into columns of nearly equal height, read down each column. DWD's
/// ramp is fifteen bands: one column of fifteen is half the map's height, two is a glance.
function evenColumns(items, count) {
  var list = items || []
  var parts = Math.max(1, Math.floor(count))
  var out = []
  for (var column = 0; column < parts; column++) out.push([])
  for (var index = 0; index < list.length; index++)
    out[Math.floor(index * parts / Math.max(1, list.length))].push(list[index])
  return out
}

