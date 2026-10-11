// Checks Model.js against a real `rustybui bar --json` payload, without needing a
// running Quickshell. Run it with:
//
//   node omarchy/test/model.test.mjs [payload.json]
//
// With no argument it reads ../test/fixtures/bar-dry.json in this repo, which was
// captured from the live KNMI nowcast.

import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import vm from "node:vm"

const here = dirname(fileURLToPath(import.meta.url))
const modelPath = resolve(here, "../Model.js")
const fixturePath = process.argv[2] ?? resolve(here, "fixtures/bar-dry.json")

// Model.js is plain QML-compatible JS; barColor needs Qt.rgba, which the text
// helpers never touch, so a stub is enough to load the file.
const context = vm.createContext({ Qt: { rgba: (r, g, b, a) => ({ r, g, b, a }) }, console })
vm.runInContext(readFileSync(modelPath, "utf8"), context, { filename: modelPath })

const Model = context
const payload = readFileSync(fixturePath, "utf8")

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
  isRainingNow: false, pill: "in 25 min", currentMmPerHour: 0, currentClass: "dry",
  startsInMinutes: 25, endsInMinutes: 65, totalMm: 1.85, peakMmPerHour: 1.2, peakInMinutes: 40
}
const soon = { ok: true, view: base }
check("rain on the way shows the wait", Model.pillText(base), "\ue318 25m")
check("minutes under an hour are minutes", Model.compactMinutes(25), "25m")
check("and hours plus padded minutes above", Model.compactMinutes(65), "1h05")
check(
  "the summary reads like a sentence",
  Model.summaryLine(base, true),
  "rain in 25m · for about 40m · peak 1.2 mm/h · 1.85 mm total"
)

const raining = JSON.parse(payload)
raining.summary = {
  isRainingNow: true, pill: "2.4 mm/h", currentMmPerHour: 2.4, currentClass: "moderate",
  startsInMinutes: null, endsInMinutes: 55, totalMm: 3.1, peakMmPerHour: 4.0, peakInMinutes: 20
}
check("rain now shows the rate", Model.pillText(raining), "\ue333 2.4")
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
check("heavy rain earns the emphatic glyph", Model.pillText(heavy), "\ue31d 12.0")

// ---- payload handling
check("null means the location is not covered", Model.parseView("null"), { ok: false, error: "no nowcast for this location" })
check("blank output is a failure, not a crash", Model.parseView("").ok, false)
check("garbage is reported, not thrown", Model.parseView("{oops").ok, false)
check("an empty point list is a failure", Model.parseView('{"points":[]}').ok, false)

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

console.log(failures === 0 ? "\nall model checks passed" : `\n${failures} check(s) failed`)
process.exit(failures === 0 ? 0 : 1)
