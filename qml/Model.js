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
  dry: "\ue33d" // cloudy — nothing expected
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

  if (!view || !view.points || view.points.length === 0)
    return { ok: false, error: "no nowcast samples returned" }

  if (!view.summary) view.summary = {}
  return { ok: true, view: view, error: "" }
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

// Compact bar label: the glyph, plus the current rate or the wait.
function pillText(view) {
  if (!view || !view.summary) return GLYPH.dry
  var summary = view.summary
  if (summary.isRainingNow) return glyphFor(view) + " " + shortIntensity(summary.currentMmPerHour)
  if (isSet(summary.startsInMinutes)) return glyphFor(view) + " " + compactMinutes(summary.startsInMinutes)
  return GLYPH.dry
}

function compactMinutes(minutes) {
  var value = Math.max(0, Math.round(Number(minutes)))
  if (value < 60) return value + "m"
  var rest = value % 60
  return Math.floor(value / 60) + "h" + (rest < 10 ? "0" : "") + rest
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
    if (isSet(summary.endsInMinutes)) parts.push("stopping in " + compactMinutes(summary.endsInMinutes))
  } else if (isSet(summary.startsInMinutes)) {
    parts.push("rain in " + compactMinutes(summary.startsInMinutes))
    if (isSet(summary.endsInMinutes) && Number(summary.endsInMinutes) > Number(summary.startsInMinutes))
      parts.push("for about " + compactMinutes(Number(summary.endsInMinutes) - Number(summary.startsInMinutes)))
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
  return Qt.rgba(color.r, color.g, color.b, alpha)
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
