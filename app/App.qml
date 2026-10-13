import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// ─────────────────────────────────────────────────────────────────────────────
// The weather window.
//
// Opened from the Omarchy menu, from a keybinding, or with
//   omarchy-shell shell summon mmb.weather '{}'
//
// One payload — `rustybui app` — drives every card, and the shared
// NowcastChart.qml draws the same two-hour chart the bar popout shows. All
// fetching, caching and rate limiting stays in the binary.
// ─────────────────────────────────────────────────────────────────────────────
Item {
  id: root

  // ---- plugin lifecycle (the shape the shell's summon routing expects) -----
  // `opened` is the state the shell reads back to answer `isPluginOpen`; the window
  // is shown and hidden from it rather than being visible of its own accord. That
  // matters here because the plugin is keepLoaded: the shell mounts it at startup,
  // and a Quickshell window is visible by default, so without this the window would
  // open on every shell restart.
  property bool opened: false
  property bool closingFromHost: false

  function syncWindowVisibility() {
    window.visible = root.opened
  }
  onOpenedChanged: Qt.callLater(syncWindowVisibility)

  function open(payloadJson) {
    closingFromHost = false
    opened = true
    Qt.callLater(function() {
      if (keyScope) keyScope.forceActiveFocus()
      root.loadPrefs()
      // The pill's settings live in shell.json, which the shell writes atomically; the file
      // watch misses that rename, so reread on every open — a window that has been shut is a
      // window that is about to show values it did not see change.
      if (shellConfigFile) shellConfigFile.reload()
      root.refresh()
    })
  }

  // Host-initiated close (`omarchy-shell shell hide …`): the host already knows,
  // so do not report back.
  function close() {
    closingFromHost = true
    opened = false
    closingFromHost = false
  }

  // User-initiated close (Esc, window close button): tell the shell, so its open
  // panel bookkeeping stays consistent and `toggle` works next time.
  function requestClose() {
    if (shell && typeof shell.hide === "function") shell.hide("mmb.weather")
    else close()
  }

  // ---- host injections -----------------------------------------------------
  property var shell: null
  property var settings: ({})

  function setting(name, fallback) {
    var value = settings ? settings[name] : undefined
    return value === undefined || value === null ? fallback : value
  }

  property string binaryPath: setting("binaryPath", "rustybui")
  property int refreshMinutes: Number(setting("refreshMinutes", 10))
  // The window's own stored preferences win over the plugin's defaults, once the user has
  // actually chosen one; until then the plugin's setting stands.
  property int hours: Number(pref("hours", "hours", setting("hours", 24)))
  property bool clock24: pref("clock24", "clock", setting("clock24", true) !== false) === true
  readonly property bool offline: pref("offline", "offline", setting("offline", false) === true) === true
  // Units are the stored preference rather than a plugin default: the payload's labels are
  // already in them, so this only decides what the toggle says.
  readonly property bool imperial: root.prefs !== null && String(root.prefs.units) === "imperial"
  readonly property string unitsLabel: root.imperial ? "Imperial" : "Metric"

  // ---- the window's own settings, which the binary stores -------------------
  property var prefs: null
  property bool showSettings: false
  property string searchQuery: ""
  property var searchResults: []
  property string searchHint: ""
  property var pendingPlace: null
  property var pendingPrefs: null

  /// A stored preference when the user has chosen one, else `fallback`.
  function pref(name, storedKey, fallback) {
    if (root.prefs && root.prefs.stored && root.prefs.stored[storedKey]) return root.prefs[name]
    return fallback
  }

  // ---- data ----------------------------------------------------------------
  property var view: null
  property string errorText: ""
  property string hintText: ""

  readonly property var nowcast: view ? view.nowcast : null
  readonly property var forecast: view ? view.forecast : null
  readonly property var current: (view && view.forecast) ? view.forecast.current : null
  readonly property var air: view ? view.air : null
  readonly property var sky: view ? view.sky : null

  readonly property color background: Commons.Color.background
  readonly property color foreground: Commons.Color.foreground
  readonly property color accent: Commons.Color.accent
  readonly property color urgent: Commons.Color.urgent

  readonly property var theme: ({ foreground: foreground, accent: accent, urgent: urgent })

  function muted(alpha) {
    return Model.withAlpha(Commons.Color.foreground, alpha)
  }

  readonly property var commandLine: {
    var command = [root.binaryPath]
    if (root.offline) command.push("--offline")
    command.push("app", "--hours", String(root.hours))
    return command
  }

  Process {
    id: appProc
    command: root.commandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyPayload(text)
    }

    onExited: function(exitCode) {
      if (exitCode === 0) return
      root.hintText = root.binaryPath + " exited " + exitCode
    }
  }

  // ---- the radar map's image -----------------------------------------------
  // The map says which box, size and frame it wants; this asks the binary for it, the way
  // every other number on screen arrives. The image itself lives on disk — a few hundred
  // kilobytes of PNG — and QML loads it from the path in the payload.
  property var radar: null
  property var radarRequest: null
  property string radarNotice: ""

  readonly property var radarCommandLine: {
    if (!root.radarRequest) return []
    var command = [root.binaryPath]
    if (root.offline) command.push("--offline")
    command.push(
      "radar", "--json",
      "--bbox", root.radarRequest.bbox.join(","),
      "--width", String(root.radarRequest.width),
      "--height", String(root.radarRequest.height)
    )
    if (root.radarRequest.timeMs) command.push("--time", new Date(Number(root.radarRequest.timeMs)).toISOString())
    return command
  }

  Process {
    id: radarProc
    command: root.radarCommandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyRadar(text)
    }

    onExited: function(exitCode) {
      if (exitCode === 0) return
      root.radarNotice = "no radar image for this view"
    }
  }

  function requestRadar(bbox, width, height, timeMs) {
    root.radarNotice = ""
    root.radarRequest = { bbox: bbox, width: width, height: height, timeMs: timeMs }
    // Changing the command while the process runs restarts it with the new arguments, which
    // is what a second request during a drag wants anyway.
    radarProc.running = true
  }

  function applyRadar(text) {
    var payload = null
    try {
      payload = JSON.parse(text)
    } catch (error) {
      root.radarNotice = "unreadable radar payload"
      return
    }
    if (!payload || !payload.image || !payload.image.path) {
      root.radarNotice = "no radar image for this view"
      return
    }
    root.radar = payload
    root.radarNotice = ""
  }

  // ---- the settings the window can change, all through the binary ----------
  // The binary owns them (the same file the CLI reads and writes), so the window and the
  // command line can never end up with different ideas about the location or the clock.

  Process {
    id: prefsProc
    command: [root.binaryPath, "prefs", "--json"]

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          root.prefs = JSON.parse(text)
        } catch (error) {
          root.searchHint = "could not read the stored preferences"
        }
      }
    }
  }

  function loadPrefs() {
    if (!prefsProc.running) prefsProc.running = true
  }

  // ---- the bar pill's display settings -------------------------------------
  // These live in Omarchy's own settings (shell.json, beside the widget in the bar layout), not
  // in the binary's file, so the window edits them the way any bar widget's settings are: through
  // `omarchy bar set`, which the shell then hands to the pill as its `settings`. Reading shell.json
  // back is what lets the controls show the current value rather than a guess.
  property var pillSettings: ({ showIcon: true, showState: true, showTemperature: true, iconSide: "left" })
  property var pendingPillSetting: null

  readonly property var pillCommandLine: {
    var change = root.pendingPillSetting
    if (!change) return []
    return ["omarchy", "bar", "set", "mmb.rainnowcast", String(change.name), JSON.stringify(change.value), "--json"]
  }

  Process {
    id: pillSettingProc
    command: root.pillCommandLine

    onExited: function(exitCode) {
      root.pendingPillSetting = null
      if (exitCode === 0) {
        if (shellConfigFile) shellConfigFile.reload()
      } else {
        root.searchHint = "could not store the pill setting"
      }
    }
  }

  function setPillSetting(name, value) {
    // Show the change at once, then let shell.json be the source of truth again when it is read
    // back; the pill updates from the shell's `settings` the moment the write lands.
    var next = ({})
    for (var key in root.pillSettings) next[key] = root.pillSettings[key]
    next[name] = value
    root.pillSettings = next
    root.pendingPillSetting = { name: name, value: value }
    Qt.callLater(function() {
      if (!pillSettingProc.running && root.pillCommandLine.length > 0) pillSettingProc.running = true
    })
  }

  function readPillSettings(raw) {
    var parsed = null
    try {
      parsed = JSON.parse(String(raw || ""))
    } catch (error) {
      return
    }
    var layout = parsed && parsed.bar && parsed.bar.layout ? parsed.bar.layout : null
    if (!layout) return
    var sections = ["left", "center", "right"]
    for (var s = 0; s < sections.length; s++) {
      var entries = Array.isArray(layout[sections[s]]) ? layout[sections[s]] : []
      for (var i = 0; i < entries.length; i++) {
        if (entries[i] && entries[i].id === "mmb.rainnowcast") {
          root.pillSettings = {
            showIcon: entries[i].showIcon !== false,
            showState: entries[i].showState !== false,
            showTemperature: entries[i].showTemperature !== false,
            iconSide: String(entries[i].iconSide || "left") === "right" ? "right" : "left"
          }
          return
        }
      }
    }
  }

  FileView {
    id: shellConfigFile
    path: Quickshell.env("HOME") + "/.config/omarchy/shell.json"
    watchChanges: true
    printErrors: false
    onLoaded: root.readPillSettings(text())
    onFileChanged: root.readPillSettings(text())
  }

  readonly property var searchCommandLine: {
    var query = String(root.searchQuery || "").replace(/^\s+|\s+$/g, "")
    if (query === "") return []
    return [root.binaryPath, "search", query, "--count", "6", "--json"]
  }

  Process {
    id: searchProc
    command: root.searchCommandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var found = null
        try {
          found = JSON.parse(text)
        } catch (error) {
          root.searchResults = []
          root.searchHint = "search failed"
          return
        }
        root.searchResults = found
        root.searchHint = found.length === 0 ? "no matches" : ""
      }
    }

    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root.searchResults = []
        root.searchHint = "search failed (exit " + exitCode + ")"
      }
    }
  }

  function runSearch() {
    if (root.searchCommandLine.length === 0) return
    root.searchHint = "searching…"
    root.searchResults = []
    if (!searchProc.running) searchProc.running = true
  }

  readonly property var setLocationCommandLine: {
    var place = root.pendingPlace
    if (!place) return []
    var command = [
      root.binaryPath, "set-location", place.name,
      "--lat", String(place.latitude),
      "--lon", String(place.longitude)
    ]
    if (place.country_code) command.push("--country", String(place.country_code))
    if (place.timezone) command.push("--timezone", String(place.timezone))
    return command
  }

  Process {
    id: setLocationProc
    command: root.setLocationCommandLine

    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root.searchHint = "could not store that location (exit " + exitCode + ")"
        return
      }
      root.showSettings = false
      root.searchResults = []
      root.searchQuery = ""
      root.searchHint = ""
      root.loadPrefs()
      root.refresh()
    }
  }

  function choosePlace(place) {
    root.pendingPlace = place
    // The command is a binding on the pending place, so it is only complete once the
    // assignment has landed.
    Qt.callLater(function() {
      if (!setLocationProc.running && root.setLocationCommandLine.length > 0)
        setLocationProc.running = true
    })
  }

  readonly property var savePrefsCommandLine: {
    var change = root.pendingPrefs
    if (!change) return []
    return [root.binaryPath, "prefs", "--" + change.name, String(change.value), "--json"]
  }

  Process {
    id: savePrefsProc
    command: root.savePrefsCommandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          root.prefs = JSON.parse(text)
        } catch (error) {
          root.searchHint = "could not read back the stored preferences"
        }
      }
    }

    onExited: function(exitCode) {
      root.pendingPrefs = null
      if (exitCode !== 0) {
        root.searchHint = "could not store that preference"
        return
      }
      // Clock, hours and units all change the labels the payload carries, so a preference is
      // only really applied once the window has asked for a new payload.
      root.loadPrefs()
      root.refresh()
    }
  }

  function setPref(name, value) {
    root.pendingPrefs = { name: name, value: value }
    Qt.callLater(function() {
      if (!savePrefsProc.running) savePrefsProc.running = true
    })
  }

  // ---- saved places --------------------------------------------------------
  // The stored location is what the window shows; the places are the ones worth coming back to.
  // Switching is a `places --use`, saving is a `places --add`, and forgetting a place takes a
  // deliberate click on its × — there is no undo, but there is the search box.
  property var placeAction: null
  property string placesHint: ""

  readonly property var savedPlaces: (root.prefs && root.prefs.places) ? root.prefs.places : []
  readonly property var storedPlace: root.prefs ? root.prefs.location : null
  readonly property int placeLimit: (root.prefs && root.prefs.placeLimit) ? Number(root.prefs.placeLimit) : 12

  function hasSaved(name) {
    if (!name) return false
    for (var index = 0; index < root.savedPlaces.length; index++)
      if (String(root.savedPlaces[index].name).toLowerCase() === String(name).toLowerCase()) return true
    return false
  }

  /// Whether the place on screen could be saved — it exists, it is not saved, and there is room.
  readonly property bool canSavePlace: root.storedPlace !== null
    && !root.hasSaved(root.storedPlace.name)
    && root.savedPlaces.length < root.placeLimit

  readonly property var placesCommandLine: {
    var action = root.placeAction
    if (!action) return []
    if (action.kind === "add") return [root.binaryPath, "places", "--add"]
    return [root.binaryPath, "places", "--" + action.kind, String(action.name)]
  }

  Process {
    id: placesProc
    command: root.placesCommandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var said = String(text).replace(/^\s+|\s+$/g, "")
        if (said !== "") root.placesHint = said
      }
    }

    // A refusal ("12 saved places is the limit") arrives on stderr, and is worth showing.
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var said = String(text).replace(/^\s+|\s+$/g, "")
        if (said !== "") root.placesHint = said.split("\n")[0].replace(/^Error: /, "")
      }
    }

    onExited: function(exitCode) {
      root.placeAction = null
      if (exitCode !== 0) {
        if (root.placesHint === "") root.placesHint = "that did not work (exit " + exitCode + ")"
        return
      }
      root.loadPrefs()
      root.refresh()
    }
  }

  function placeStep(kind, name) {
    root.placesHint = ""
    root.placeAction = { kind: kind, name: name }
    Qt.callLater(function() {
      if (!placesProc.running && root.placesCommandLine.length > 0) placesProc.running = true
    })
  }

  // ---- one day, opened up --------------------------------------------------
  // A row in the day list expands into the hours behind it: the same payload shape the CLI's
  // `day` command prints, fetched for that one date and rendered by the same hour strip.
  property var dayDetail: null
  property string dayHint: ""
  property var dayRequest: null

  readonly property var dayCommandLine: {
    var request = root.dayRequest
    if (!request) return []
    var command = [root.binaryPath]
    if (root.offline) command.push("--offline")
    command.push("day", "--date", String(request.date))
    if (root.view && root.view.location) {
      command.push(
        "--lat", String(root.view.location.latitude),
        "--lon", String(root.view.location.longitude),
        "--name", String(root.view.location.name)
      )
    }
    if (!root.clock24) command.push("--clock", "12h")
    return command
  }

  Process {
    id: dayProc
    command: root.dayCommandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyDay(text)
    }

    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var said = String(text).replace(/^\s+|\s+$/g, "")
        if (said !== "") root.dayHint = said.split("\n")[0].replace(/^Error: /, "")
      }
    }

    onExited: function(exitCode) {
      root.dayRequest = null
      if (exitCode === 0) return
      if (root.dayHint === "") root.dayHint = "no forecast for that day (exit " + exitCode + ")"
    }
  }

  /// Brings a card into view. A row near the end of the list opens a card that starts below the
  /// window, and a detail nobody can see is a detail nobody asked for.
  function reveal(item) {
    if (!scroller || !item) return
    var wanted = item.y - Style.space(8)
    scroller.contentY = Math.max(0, Math.min(scroller.contentHeight - scroller.height, wanted))
  }

  function revealDay() {
    root.reveal(dayCard)
  }

  function revealSettings() {
    root.reveal(settingsCard)
  }

  /// Opens a day, or closes it when it is the one already open.
  function openDay(day) {
    if (!day) return
    if (root.dayDetail !== null && String(root.dayDetail.date) === String(day.date)) {
      root.closeDay()
      return
    }
    root.dayHint = "loading…"
    root.dayDetail = null
    root.dayRequest = { date: String(day.date) }
    Qt.callLater(root.revealDay)
    Qt.callLater(function() {
      if (!dayProc.running && root.dayCommandLine.length > 0) dayProc.running = true
    })
  }

  function closeDay() {
    root.dayDetail = null
    root.dayRequest = null
    root.dayHint = ""
  }

  function applyDay(raw) {
    var text = String(raw === undefined || raw === null ? "" : raw).replace(/^\s+|\s+$/g, "")
    if (text === "") {
      root.dayHint = "nothing came back for that day"
      return
    }
    var parsed = null
    try {
      parsed = JSON.parse(text)
    } catch (error) {
      root.dayHint = "unreadable day payload"
      return
    }
    if (!parsed || !parsed.date) {
      root.dayHint = "unreadable day payload"
      return
    }
    root.dayDetail = parsed
    root.dayHint = parsed.hours && parsed.hours.length > 0 ? "" : "no hourly detail for that day"
    // The card has just grown by the height of a day's hours; keep it in view.
    Qt.callLater(root.revealDay)
  }

  /// `15.3° / 3.0° · Overcast · 0.40 mm · 07:27 → 18:17`, or as much of it as the day has.
  readonly property string daySummary: {
    if (!root.dayDetail || !root.dayDetail.day) return ""
    var day = root.dayDetail.day
    var parts = []
    if (day.highLabel || day.lowLabel) parts.push((day.highLabel || "—") + " / " + (day.lowLabel || "—"))
    if (day.conditionLabel) parts.push(String(day.conditionLabel))
    if (day.precipitationLabel) parts.push(String(day.precipitationLabel))
    if (day.sunriseLabel && day.sunsetLabel) parts.push(day.sunriseLabel + " → " + day.sunsetLabel)
    return parts.join(" · ")
  }

  // ---- the keyboard ---------------------------------------------------------
  // One entry point, so the same keys can be exercised from a harness as from the window: a
  // handler that only exists inside `Keys.onPressed` can be rendered but never pressed.
  property bool showKeys: false
  property int dayCursor: -1

  // ---- stepping through the window's blocks ---------------------------------
  // The blocks the keys walk, in the order they are drawn. The masthead and the settings
  // affordance are page furniture, not stops: only the cards the window is about. And only
  // what is on screen — a hidden card is not a place to land — so the list is a binding, and
  // opening the settings or a day inserts that card as a stop for as long as it is there.
  readonly property var scrollStops: [
    settingsCard, warnings,
    nowCard, skyCard, nowcastCard, mapCard,
    hoursCard, daysCard, dayCard, airCard, footer
  ].filter(function(item) { return item && item.visible })

  /// Where the top of `item` sits in the scrolled content, clamped to what can be scrolled to.
  function stopOffset(item) {
    var furthest = Math.max(0, scroller.contentHeight - scroller.height)
    return Math.max(0, Math.min(furthest, item.y))
  }

  /// Puts one block's top flush with the top of the window, so every step lands the same way.
  function scrollToStop(item) {
    if (!scroller || !item) return
    scroller.contentY = root.stopOffset(item)
  }

  /// Steps to the first block below (or the last above) where the view actually rests — the
  /// wheel can leave the list anywhere, and the keys should carry on from there rather than
  /// from wherever the keyboard was last.
  function stepSection(direction) {
    var stops = root.scrollStops
    if (stops.length === 0 || !scroller) return false
    var index = -1
    if (direction > 0) {
      for (var down = 0; down < stops.length; down++) {
        if (root.stopOffset(stops[down]) > scroller.contentY + 1) { index = down; break }
      }
      if (index < 0) index = stops.length - 1
    } else {
      for (var up = stops.length - 1; up >= 0; up--) {
        if (root.stopOffset(stops[up]) < scroller.contentY - 1) { index = up; break }
      }
      if (index < 0) index = 0
    }
    root.scrollToStop(stops[index])
    return true
  }

  /// Moves the ring in the day list. It used to be the arrow keys' job; they now scroll the
  /// window, and the list keeps n/p so Enter still has something to open.
  function stepDay(direction) {
    if (!root.dailyRows.length) return false
    root.showKeys = false
    var from = root.dayCursor < 0 ? (direction > 0 ? -1 : 0) : root.dayCursor
    root.dayCursor = Math.max(0, Math.min(root.dailyRows.length - 1, from + direction))
    return true
  }

  function pressKey(key, text) {
    var character = String(text === undefined || text === null ? "" : text).toLowerCase()
    switch (key) {
      case Qt.Key_Question:
        root.showKeys = !root.showKeys
        return true
      case Qt.Key_Escape:
        // Escape backs out of one layer at a time: the key list, then the settings, then the window.
        if (root.showKeys) root.showKeys = false
        else if (root.showSettings) root.showSettings = false
        else if (root.dayDetail !== null) root.closeDay()
        else root.requestClose()
        return true
      case Qt.Key_Slash:
        root.showKeys = false
        root.showSettings = true
        Qt.callLater(function() { if (searchInput) searchInput.forceActiveFocus() })
        return true
      case Qt.Key_Up:
      case Qt.Key_Down:
      case Qt.Key_J:
      case Qt.Key_K:
        // One key, one block: ↑/k and ↓/j walk the window from wherever the view rests.
        if (!root.stepSection(key === Qt.Key_Up || key === Qt.Key_K ? -1 : 1)) return false
        root.showKeys = false
        return true
      case Qt.Key_N:
      case Qt.Key_P:
        return root.stepDay(key === Qt.Key_N ? 1 : -1)
      case Qt.Key_Return:
      case Qt.Key_Enter:
        if (root.dayCursor < 0 || root.dayCursor >= root.dailyRows.length) return false
        root.showKeys = false
        root.openDay(root.dailyRows[root.dayCursor])
        return true
      case Qt.Key_M:
        root.showKeys = false
        root.reveal(mapCard)
        return true
      case Qt.Key_S:
        // The list is help, not a mode: anything it helps you do dismisses it.
        root.showKeys = false
        root.showSettings = !root.showSettings
        if (root.showSettings) {
          root.loadPrefs()
          Qt.callLater(root.revealSettings)
        }
        return true
      case Qt.Key_R:
      case Qt.Key_F5:
        root.refresh()
        return true
      default:
        // A layout where "?" needs no modifier arrives as its own key; one where it does arrives
        // as the question mark's key code too, so only the character needs checking.
        if (character === "?") {
          root.showKeys = !root.showKeys
          return true
        }
        if (character === "/") {
          root.showSettings = true
          root.loadPrefs()
          Qt.callLater(root.revealSettings)
          Qt.callLater(function() { if (searchInput) searchInput.forceActiveFocus() })
          return true
        }
        return false
    }
  }

  /// The keys the window answers to, in the order worth learning them. The list and the handler
  /// are one thing: a key that is not here is a key that does not work.
  readonly property var keyList: [
    { keys: "?", what: "this list" },
    { keys: "/", what: "search for a place" },
    { keys: "↑ ↓ · j k", what: "scroll block by block" },
    { keys: "n p", what: "walk the day list" },
    { keys: "Enter", what: "open that day's hours" },
    { keys: "m", what: "jump to the map" },
    { keys: "s", what: "settings — location, clock, units" },
    { keys: "r", what: "fetch everything again" },
    { keys: "Esc", what: "back out of whatever is open" }
  ]

  // A good payload always replaces the last one; a failure keeps the last good
  // screen visible and only records why.
  function applyPayload(raw) {
    var parsed = Model.parseApp(raw)
    if (!parsed.ok) {
      root.errorText = parsed.error
      return
    }
    root.view = parsed.view
    root.errorText = ""
    root.hintText = ""
  }

  function refresh() {
    if (!appProc.running) appProc.running = true
  }

  Timer {
    interval: Math.max(1, root.refreshMinutes) * 60 * 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  // ---- derived rows --------------------------------------------------------
  readonly property var hourlyRows: (root.forecast && root.forecast.hourly) ? root.forecast.hourly : []
  readonly property var dailyRows: (root.forecast && root.forecast.daily) ? root.forecast.daily.slice(0, 7) : []

  readonly property var stats: {
    if (!root.current) return []
    var currentRow = root.current
    var cloud = currentRow.cloudCoverPct
    return [
      { label: "Wind", value: currentRow.windLabel || "—" },
      { label: "Humidity", value: currentRow.humidityLabel || "—" },
      { label: "Pressure", value: currentRow.pressureLabel || "—" },
      { label: "Cloud cover", value: (cloud === null || cloud === undefined) ? "—" : Math.round(cloud) + "%" },
      { label: "UV index", value: currentRow.uvLabel || "—" },
      { label: "Rain now", value: currentRow.precipitationLabel || "—" }
    ]
  }

  readonly property string heroGlyph: root.current
    ? Model.conditionGlyph(root.current.weatherCode, root.current.isDay)
    : ""

  readonly property string updatedLabel: {
    if (!root.view) return ""
    return "updated " + Model.formatClock(root.view.generatedAtMs, root.clock24)
  }

  readonly property string sourceLine: {
    var parts = []
    if (root.nowcast) parts.push(root.nowcast.provider.label)
    if (root.forecast) parts.push(root.forecast.timezone)
    return parts.join(" · ")
  }

  // ───────────────────────────────────────────────────────────────────────────
  FloatingWindow {
    id: window
    title: "Weather"
    // The window paints nothing itself: the frame below carries the surface, so the theme's
    // border and corner radius are drawn rather than left to a compositor whose border the
    // look'n'feel has turned off.
    color: "transparent"
    implicitWidth: Style.space(620)
    // Three quarters of the monitor's height, so the window reads as an overlay: the bar and
    // the workspace stay visible, while the nowcast, the map and the first days are reachable
    // without scrolling past everything. `window.screen` is in logical pixels, so this holds
    // on any display; a fixed 780 was most of this machine's screen already. The fallback is
    // that old 780 for the moment before Quickshell has placed the surface on a screen.
    readonly property real screenHeight: window.screen && window.screen.height > 0
      ? window.screen.height
      : Style.space(1040)
    implicitHeight: Math.max(Style.space(480), Math.round(screenHeight * 0.75))
    // The shape Omarchy's own dev-gallery uses; Quickshell's FloatingWindow adds both to
    // QQuickWindow, so the window still has to be checked against the real shell and not
    // only against the render-harness stub.
    minimumSize: Qt.size(Style.space(480), Style.space(480))
    // Starts hidden and only appears when the shell summons it. Declared explicitly so
    // a keepLoaded mount at shell start never paints a frame before syncWindowVisibility()
    // runs; the first sync drops the binding and drives it imperatively from there on.
    visible: false

    Component.onCompleted: root.syncWindowVisibility()

    // A close the host did not ask for — the window manager's close button — still has
    // to reach the shell, or `opened` would stay true and the next toggle would hide.
    onVisibleChanged: {
      if (!visible && root.opened && !root.closingFromHost) root.requestClose()
    }

    // The frame: the shell's popup surface — the popup role's fill and border, the theme's
    // corner radius, the popout's padding — so the window reads as the nowcast's larger
    // sibling rather than a bare rectangle behind the cards. The FocusScope is inset to the
    // frame's content box, so the content sits inside the border and the padding.
    BorderSurface {
      id: frame
      anchors.fill: parent
      color: Commons.Color.popups.background
      radius: Style.cornerRadius
      borderSpec: Border.surfaceSpec("popups", "border", Commons.Color.popups.border, Math.max(1, Style.space(2)))
      padding: Style.spacing.popupPadding
    }

    FocusScope {
      id: keyScope
      anchors.fill: parent
      anchors.topMargin: frame.contentTopInset
      anchors.rightMargin: frame.contentRightInset
      anchors.bottomMargin: frame.contentBottomInset
      anchors.leftMargin: frame.contentLeftInset
      focus: true

      Keys.onEscapePressed: function(event) {
        root.pressKey(Qt.Key_Escape, "")
        event.accepted = true
      }

      Keys.onPressed: function(event) {
        event.accepted = root.pressKey(event.key, event.text)
      }

      // ---- the key list ---------------------------------------------------
      // `?` puts the keys on screen. A list that is not there when you need it is a list nobody
      // reads, and this is the one place they are written down.
      Item {
        id: keyOverlay
        anchors.fill: parent
        visible: root.showKeys
        z: 20

        Rectangle {
          anchors.fill: parent
          color: Model.withAlpha(root.background, 0.72)

          MouseArea {
            anchors.fill: parent
            onClicked: root.showKeys = false
          }
        }

        Card {
          anchors.centerIn: parent
          width: Math.min(parent.width - Style.space(24), keyColumn.width + Style.space(20))
          height: keyColumn.height + Style.space(16)

          Column {
            id: keyColumn
            width: keyRows.width
            spacing: Style.space(4)

            Text {
              text: "Keys"
              color: root.foreground
              font.family: Style.font.family
              font.pixelSize: Style.font.subtitle
            }

            Column {
              id: keyRows
              spacing: Style.space(3)

              Repeater {
                model: root.keyList

                delegate: Item {
                  required property var modelData
                  width: Math.max(keyName.implicitWidth, Style.space(48)) + Style.space(12) + keyWhat.implicitWidth
                  height: Math.max(keyName.implicitHeight, keyWhat.implicitHeight)

                  Text {
                    id: keyName
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    width: Math.max(implicitWidth, Style.space(48))
                    text: modelData.keys
                    color: root.accent
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                  }

                  Text {
                    id: keyWhat
                    anchors.left: parent.left
                    anchors.leftMargin: Math.max(keyName.width, Style.space(48)) + Style.space(12)
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.what
                    color: root.muted(0.8)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                  }
                }
              }
            }

            Text {
              text: "Esc or ? closes this"
              color: root.muted(0.45)
              font.family: Style.font.family
              font.pixelSize: Style.font.caption
            }
          }
        }
      }

      Flickable {
        id: scroller
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height
        // The built-in wheel step is conservative; one notch here is one notch's worth of pixels,
        // so a window this tall does not feel stuck. Touchpads keep their own pixel deltas.
        flickDeceleration: 3000
        maximumFlickVelocity: 4000

        WheelHandler {
          acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
          onWheel: function(event) {
            var delta = event.pixelDelta.y !== 0 ? event.pixelDelta.y : event.angleDelta.y
            if (delta === 0) return
            scroller.contentY = Math.max(0, Math.min(scroller.contentHeight - scroller.height, scroller.contentY - delta))
            event.accepted = true
          }
        }

        Column {
          id: column
          width: scroller.width
          spacing: Style.spacing.panelGap

          // ---- masthead ------------------------------------------------------
          Item {
            width: parent.width
            height: Math.max(place.implicitHeight, stamp.implicitHeight)

            Column {
              id: place
              anchors.left: parent.left
              spacing: Style.space(2)

              Text {
                text: root.view ? root.view.location.name : "Weather"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.heading
              }

              Text {
                text: root.sourceLine
                color: root.muted(0.55)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }

            Text {
              id: stamp
              anchors.right: parent.right
              anchors.top: parent.top
              text: root.updatedLabel
              color: root.muted(0.45)
              font.family: Style.font.family
              font.pixelSize: Style.font.caption
            }
          }

          // ---- a way in to the window's own settings --------------------------
          // The location and the display preferences are all editable from here, through
          // the binary, so the window does not need the command line to be usable.
          Item {
            width: parent.width
            height: settingsToggle.implicitHeight

            Text {
              id: settingsToggle
              anchors.right: parent.right
              text: root.showSettings ? "Done" : "Settings"
              color: root.muted(0.7)
              font.family: Style.font.family
              font.pixelSize: Style.font.caption

              MouseArea {
                anchors.fill: parent
                anchors.margins: -Style.space(4)
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                  root.showSettings = !root.showSettings
                  if (root.showSettings) {
                    root.loadPrefs()
                    Qt.callLater(root.revealSettings)
                  }
                }
              }
            }
          }

          Card {
            id: settingsCard
            width: parent.width
            visible: root.showSettings

            Column {
              width: parent.width
              spacing: Style.space(8)

              Text {
                text: "Where"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              Row {
                width: parent.width
                spacing: Style.space(6)

                Rectangle {
                  width: Math.max(Style.space(120), parent.width - searchButton.width - Style.space(6))
                  height: Style.space(30)
                  radius: Style.cornerRadius
                  color: Model.withAlpha(root.foreground, 0.06)
                  border.width: 1
                  border.color: Model.withAlpha(root.foreground, 0.18)

                  TextInput {
                    id: searchInput
                    anchors.fill: parent
                    anchors.leftMargin: Style.space(8)
                    anchors.rightMargin: Style.space(8)
                    verticalAlignment: TextInput.AlignVCenter
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                    selectByMouse: true
                    clip: true
                    text: root.searchQuery
                    onTextEdited: root.searchQuery = text
                    onAccepted: root.runSearch()
                  }

                  Text {
                    anchors.left: parent.left
                    anchors.leftMargin: Style.space(8)
                    anchors.verticalCenter: parent.verticalCenter
                    visible: searchInput.text === ""
                    text: "Search a place…"
                    color: root.muted(0.4)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                  }
                }

                Rectangle {
                  id: searchButton
                  width: Style.space(80)
                  height: Style.space(30)
                  radius: Style.cornerRadius
                  color: Model.withAlpha(root.accent, 0.18)

                  Text {
                    anchors.centerIn: parent
                    text: "Search"
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                  }

                  MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.runSearch()
                  }
                }
              }

              Text {
                width: parent.width
                visible: text !== ""
                text: root.searchHint
                color: root.muted(0.6)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }

              Repeater {
                model: root.searchResults

                delegate: Rectangle {
                  width: parent.width
                  height: Style.space(30)
                  radius: Style.cornerRadius
                  color: searchRow.containsMouse ? Model.withAlpha(root.accent, 0.14) : "transparent"

                  Text {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.name + (modelData.country_code ? "  ·  " + modelData.country_code : "")
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                    elide: Text.ElideRight
                  }

                  MouseArea {
                    id: searchRow
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.choosePlace(modelData)
                  }
                }
              }

              Text {
                width: parent.width
                text: {
                  if (!root.prefs) return "Preferences are not readable right now."
                  var place = root.prefs.location
                  return place ? "Stored: " + place.name : "No location stored yet — search for one above."
                }
                color: root.muted(0.55)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }

              Text {
                text: "Places"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              // One chip per saved place, wrapping rather than scrolling: the whole point is to
              // see them all at once and click one. The current place is outlined; each chip's ×
              // forgets it, which is the only way to prune from here.
              Flow {
                width: parent.width
                spacing: Style.space(6)

                Repeater {
                  model: root.savedPlaces

                  delegate: Rectangle {
                    id: placeChip
                    required property var modelData

                    readonly property bool isCurrent: root.storedPlace !== null
                      && String(root.storedPlace.name).toLowerCase() === String(modelData.name).toLowerCase()

                    width: placeName.implicitWidth + forgetLabel.implicitWidth + Style.space(14)
                    height: Style.space(28)
                    radius: Style.cornerRadius
                    color: Model.withAlpha(root.foreground, placeChip.isCurrent ? 0.14 : (placeRow.containsMouse ? 0.1 : 0.06))
                    border.width: placeChip.isCurrent ? 1 : 0
                    border.color: Model.withAlpha(root.accent, 0.7)

                    MouseArea {
                      id: placeRow
                      anchors.fill: parent
                      anchors.rightMargin: forgetLabel.width + Style.space(4)
                      hoverEnabled: true
                      cursorShape: Qt.PointingHandCursor
                      onClicked: root.placeStep("use", placeChip.modelData.name)
                    }

                    Text {
                      id: placeName
                      anchors.left: parent.left
                      anchors.leftMargin: Style.space(8)
                      anchors.verticalCenter: parent.verticalCenter
                      text: placeChip.modelData.name
                      color: root.foreground
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                    }

                    Text {
                      id: forgetLabel
                      anchors.right: parent.right
                      anchors.rightMargin: Style.space(6)
                      anchors.verticalCenter: parent.verticalCenter
                      text: "×"
                      color: forgetRow.containsMouse ? root.urgent : root.muted(0.5)
                      font.family: Style.font.family
                      font.pixelSize: Style.font.body

                      MouseArea {
                        id: forgetRow
                        anchors.fill: parent
                        anchors.margins: -Style.space(4)
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.placeStep("remove", placeChip.modelData.name)
                      }
                    }
                  }
                }

                // The place on screen, when it is not saved yet — so switching to somewhere new
                // and keeping it is one click, not a trip to the command line.
                Rectangle {
                  visible: root.canSavePlace
                  width: savePlaceLabel.implicitWidth + Style.space(16)
                  height: Style.space(28)
                  radius: Style.cornerRadius
                  color: Model.withAlpha(root.accent, savePlaceRow.containsMouse ? 0.26 : 0.16)

                  Text {
                    id: savePlaceLabel
                    anchors.centerIn: parent
                    text: "+ Save " + (root.storedPlace ? root.storedPlace.name : "")
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                  }

                  MouseArea {
                    id: savePlaceRow
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.placeStep("add", "")
                  }
                }

                Text {
                  visible: root.savedPlaces.length === 0 && !root.canSavePlace
                  text: "None saved yet."
                  color: root.muted(0.5)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption
                }
              }

              Text {
                width: parent.width
                visible: text !== ""
                text: root.placesHint
                color: root.muted(0.7)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }

              Text {
                text: "Display"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              Row {
                width: parent.width
                spacing: Style.space(6)

                Repeater {
                  model: [
                    {
                      label: root.clock24 ? "24-hour clock" : "12-hour clock",
                      name: "clock",
                      value: root.clock24 ? "12h" : "24h"
                    },
                    {
                      label: root.hours >= 24 ? "24 hours" : root.hours + " hours",
                      name: "hours",
                      value: root.hours >= 24 ? 12 : 24
                    },
                    {
                      label: root.offline ? "Offline" : "Online",
                      name: "offline",
                      value: root.offline ? "false" : "true"
                    },
                    {
                      label: root.unitsLabel,
                      name: "units",
                      value: root.imperial ? "metric" : "imperial"
                    }
                  ]

                  delegate: Rectangle {
                    width: (parent.width - Style.space(18)) / 4
                    height: Style.space(30)
                    radius: Style.cornerRadius
                    color: Model.withAlpha(root.foreground, preferenceRow.containsMouse ? 0.12 : 0.06)
                    border.width: 1
                    border.color: Model.withAlpha(root.foreground, 0.18)

                    Text {
                      anchors.centerIn: parent
                      text: modelData.label
                      color: root.foreground
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                    }

                    MouseArea {
                      id: preferenceRow
                      anchors.fill: parent
                      hoverEnabled: true
                      cursorShape: Qt.PointingHandCursor
                      onClicked: root.setPref(modelData.name, modelData.value)
                    }
                  }
                }
              }

              Text {
                text: "Pill"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              Row {
                width: parent.width
                spacing: Style.space(6)

                Repeater {
                  model: [
                    {
                      label: root.pillSettings.showIcon ? "Icon" : "No icon",
                      name: "showIcon",
                      value: !root.pillSettings.showIcon
                    },
                    {
                      label: root.pillSettings.showState ? "Rain state" : "No state",
                      name: "showState",
                      value: !root.pillSettings.showState
                    },
                    {
                      label: root.pillSettings.showTemperature ? "Temperature" : "No temp",
                      name: "showTemperature",
                      value: !root.pillSettings.showTemperature
                    },
                    {
                      label: root.pillSettings.iconSide === "right" ? "Icon right" : "Icon left",
                      name: "iconSide",
                      value: root.pillSettings.iconSide === "right" ? "left" : "right"
                    }
                  ]

                  delegate: Rectangle {
                    width: (parent.width - Style.space(18)) / 4
                    height: Style.space(30)
                    radius: Style.cornerRadius
                    color: Model.withAlpha(root.foreground, pillRow.containsMouse ? 0.12 : 0.06)
                    border.width: 1
                    border.color: Model.withAlpha(root.foreground, 0.18)

                    Text {
                      anchors.centerIn: parent
                      text: modelData.label
                      color: root.foreground
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                    }

                    MouseArea {
                      id: pillRow
                      anchors.fill: parent
                      hoverEnabled: true
                      cursorShape: Qt.PointingHandCursor
                      onClicked: root.setPillSetting(modelData.name, modelData.value)
                    }
                  }
                }
              }

              Text {
                width: parent.width
                text: "The bar pill's own parts, stored in Omarchy's settings and applied live."
                color: root.muted(0.4)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }

              Text {
                width: parent.width
                text: "Stored by rusty-bui, the same file the CLI reads."
                color: root.muted(0.4)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }
          }

          // ---- warnings ------------------------------------------------------
          // Only when there are any: an empty card would push everything down to say nothing.
          Card {
            id: warnings
            width: parent.width
            // Defensive about a payload from an older binary: no field means no warnings,
            // rather than a TypeError that takes the whole window down.
            visible: (root.view && root.view.alerts ? root.view.alerts.length > 0 : false)
            // The theme's urgent role, tinted, so a warning reads as one at a glance.
            color: Model.withAlpha(root.urgent, 0.14)
            borderSpec: Border.surfaceSpec("popups", "border", root.urgent, Math.max(1, Style.space(1)))

            Column {
              width: parent.width
              spacing: Style.space(8)

              Text {
                id: alertsHeading
                text: {
                  var alerts = (root.view && root.view.alerts) ? root.view.alerts : []
                  return alerts.length === 1 ? "1 warning in force" : alerts.length + " warnings in force"
                }
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              Repeater {
                model: (root.view && root.view.alerts) ? root.view.alerts : []

                delegate: Column {
                  width: parent.width
                  spacing: Style.space(3)

                  Item {
                    width: parent.width
                    height: Math.max(alertsHeading.implicitHeight, alertEvent.implicitHeight)

                    Text {
                      id: alertSeverity
                      anchors.left: parent.left
                      anchors.verticalCenter: parent.verticalCenter
                      // Severe and extreme get the urgent colour; the milder two the accent.
                      text: modelData.severityLabel
                      color: modelData.severityRank >= 2 ? root.urgent : root.accent
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                      width: Style.space(58)
                    }

                    Text {
                      id: alertEvent
                      anchors.left: alertSeverity.right
                      anchors.right: alertWindow.left
                      anchors.verticalCenter: parent.verticalCenter
                      text: modelData.event
                      color: root.foreground
                      font.family: Style.font.family
                      font.pixelSize: Style.font.body
                      elide: Text.ElideRight
                    }

                    Text {
                      id: alertWindow
                      anchors.right: parent.right
                      anchors.verticalCenter: parent.verticalCenter
                      text: modelData.windowLabel
                      color: root.muted(0.6)
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                    }
                  }

                  Text {
                    width: parent.width
                    text: modelData.headline || ""
                    color: root.muted(0.85)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    wrapMode: Text.WordWrap
                  }

                  Text {
                    width: parent.width
                    text: modelData.detail || ""
                    color: root.muted(0.55)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    wrapMode: Text.WordWrap
                    maximumLineCount: 3
                    elide: Text.ElideRight
                  }

                  Text {
                    width: parent.width
                    visible: text !== ""
                    text: modelData.instruction || ""
                    color: root.accent
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    wrapMode: Text.WordWrap
                  }
                }
              }
            }
          }

          // ---- now ------------------------------------------------------------
          Card {
            id: nowCard
            width: parent.width
            visible: root.current !== null

            Column {
              width: parent.width
              spacing: Style.space(10)

              Item {
                width: parent.width
                height: Math.max(heroGlyph.implicitHeight, heroColumn.implicitHeight)

                Text {
                  id: heroGlyph
                  anchors.left: parent.left
                  anchors.verticalCenter: parent.verticalCenter
                  text: root.heroGlyph
                  color: root.accent
                  font.family: Style.font.family
                  font.pixelSize: Style.font.displayLarge
                }

                Column {
                  id: heroColumn
                  anchors.left: heroGlyph.right
                  anchors.leftMargin: Style.space(14)
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.space(2)

                  Text {
                    text: root.current ? (root.current.temperatureLabel || "—") : "—"
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.display
                  }

                  Text {
                    text: root.current
                      ? [root.current.conditionLabel, root.current.feelsLikeLabel].filter(function(part) { return !!part }).join(" · ")
                      : ""
                    color: root.muted(0.75)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.subtitle
                  }
                }
              }

              Grid {
                width: parent.width
                columns: 3
                columnSpacing: Style.space(12)
                rowSpacing: Style.space(8)

                Repeater {
                  model: root.stats

                  delegate: Column {
                    width: (parent.width - Style.space(24)) / 3
                    spacing: Style.space(1)

                    Text {
                      text: modelData.label
                      color: root.muted(0.5)
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                    }

                    Text {
                      text: modelData.value
                      color: root.foreground
                      font.family: Style.font.family
                      font.pixelSize: Style.font.body
                      elide: Text.ElideRight
                      width: parent.width
                    }
                  }
                }
              }
            }
          }

          // ---- sun and moon ---------------------------------------------------
          // The sun's times come from the provider when it has them and from the arithmetic when
          // it does not — which is every DWD day. The moon is arithmetic either way.
          Card {
            id: skyCard
            width: parent.width
            visible: root.sky !== null

            Column {
              width: parent.width
              spacing: Style.space(10)

              Row {
                width: parent.width
                spacing: Style.space(12)

                Text {
                  anchors.verticalCenter: parent.verticalCenter
                  text: Model.conditionGlyph(0, true)
                  color: root.accent
                  font.family: Style.font.family
                  font.pixelSize: Style.font.title
                }

                Column {
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.space(1)

                  Text {
                    text: root.sky
                      ? [root.sky.sunriseLabel, root.sky.sunsetLabel].filter(function(part) { return !!part }).join(" → ")
                      : ""
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                  }

                  Text {
                    text: root.sky
                      ? [root.sky.daylightLabel, root.sky.sunEventLabel].filter(function(part) { return !!part }).join(" · ")
                      : ""
                    color: root.muted(0.55)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                  }
                }
              }

              // How far through the daylight it is, and nothing at all once the sun is down.
              Rectangle {
                width: parent.width
                height: 2
                radius: 1
                visible: root.sky !== null && root.sky.daylightProgress !== null && root.sky.daylightProgress !== undefined
                color: Model.withAlpha(root.foreground, 0.14)

                Rectangle {
                  width: parent.width * (root.sky && root.sky.daylightProgress ? root.sky.daylightProgress : 0)
                  height: parent.height
                  radius: parent.radius
                  color: root.accent
                }
              }

              Row {
                width: parent.width
                spacing: Style.space(12)

                MoonDisc {
                  anchors.verticalCenter: parent.verticalCenter
                  illumination: root.sky ? Number(root.sky.moonIllumination) : 0
                  waxing: root.sky ? root.sky.moonWaxing === true : true
                  color: root.foreground
                }

                Column {
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.space(1)

                  Text {
                    text: root.sky ? root.sky.moonPhaseLabel : ""
                    color: root.foreground
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                  }

                  Text {
                    text: root.sky ? root.sky.moonLabel : ""
                    color: root.muted(0.55)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                  }
                }
              }
            }
          }

          // ---- rain nowcast --------------------------------------------------
          Card {
            id: nowcastCard
            width: parent.width
            visible: root.nowcast !== null

            Column {
              width: parent.width
              spacing: Style.space(8)

              Item {
                width: parent.width
                height: Math.max(nowcastTitle.implicitHeight, nowcastSource.implicitHeight)

                Text {
                  id: nowcastTitle
                  anchors.left: parent.left
                  text: "Rain nowcast"
                  color: root.foreground
                  font.family: Style.font.family
                  font.pixelSize: Style.font.subtitle
                }

                Text {
                  id: nowcastSource
                  anchors.right: parent.right
                  text: root.nowcast ? root.nowcast.provider.attribution : ""
                  color: root.muted(0.5)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption
                }
              }

              NowcastChart {
                width: parent.width
                visible: root.nowcast !== null
                points: root.nowcast ? root.nowcast.points : []
                slotMinutes: root.nowcast ? root.nowcast.slotMinutes : 5
                clock24: root.clock24
                hoveredIndex: -1
              }

              Text {
                width: parent.width
                wrapMode: Text.WordWrap
                text: root.nowcast ? Model.summaryLine(root.nowcast, root.clock24) : ""
                color: root.muted(0.85)
                font.family: Style.font.family
                font.pixelSize: Style.font.body
              }

              Text {
                width: parent.width
                text: root.nowcast ? Model.statusLine(root.nowcast, root.clock24) : ""
                color: root.muted(0.45)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }
          }

          // ---- the radar map --------------------------------------------------
          // DWD's composite, which answers in the projection it is asked for, over coastlines
          // vendored from Natural Earth. Drag it, wheel to zoom, step through the frames.
          Card {
            id: mapCard
            width: parent.width
            visible: root.view !== null

            Column {
              width: parent.width
              spacing: Style.space(8)

              Item {
                width: parent.width
                height: Math.max(mapTitle.implicitHeight, mapHint.implicitHeight)

                Text {
                  id: mapTitle
                  anchors.left: parent.left
                  text: "Rain map"
                  color: root.foreground
                  font.family: Style.font.family
                  font.pixelSize: Style.font.subtitle
                }

                Text {
                  id: mapHint
                  anchors.right: parent.right
                  text: "drag to move · wheel to zoom"
                  color: root.muted(0.5)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption
                }
              }

              RadarMap {
                width: parent.width
                location: root.view ? root.view.location : null
                radar: root.radar
                theme: root.theme
                clock24: root.clock24
                notice: root.radarNotice
                // The window is kept loaded between summons: a map still playing behind a closed
                // window would keep asking for one image per frame.
                active: window.visible

                onImageNeeded: function(bbox, width, height, timeMs) {
                  root.requestRadar(bbox, width, height, timeMs)
                }
              }
            }
          }

          // ---- next hours ----------------------------------------------------
          Card {
            id: hoursCard
            width: parent.width
            visible: root.hourlyRows.length > 0

            Column {
              width: parent.width
              spacing: Style.space(8)

              Text {
                text: "Next " + root.hourlyRows.length + " hours"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              HourStrip {
                width: parent.width
                hours: root.hourlyRows
                foreground: root.foreground
                accent: root.accent
              }
            }
          }

          // ---- days ----------------------------------------------------------
          Card {
            id: daysCard
            width: parent.width
            visible: root.dailyRows.length > 0

            Column {
              width: parent.width
              spacing: Style.space(6)

              // Titled like the hours card above it; without a header the day rows read as a
              // loose list of figures.
              Text {
                text: "Next " + root.dailyRows.length + " days"
                color: root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.subtitle
              }

              Repeater {
                model: root.dailyRows

                delegate: Item {
                  width: parent.width
                  height: Math.max(dayGlyph.implicitHeight, dayHigh.implicitHeight) + Style.space(6)

                  // The whole row opens that day's hours; the open one is the one that closes. The
                  // cursor is the row the keyboard is on, which is a third thing again.
                  readonly property bool opened: root.dayDetail !== null
                    && String(root.dayDetail.date) === String(modelData.date)
                  readonly property bool cursored: index === root.dayCursor

                  Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: -Style.space(6)
                    anchors.rightMargin: -Style.space(6)
                    radius: Style.cornerRadius
                    color: Model.withAlpha(root.foreground,
                      opened ? 0.1 : (cursored ? 0.07 : (dayRow.containsMouse ? 0.06 : 0)))
                    border.width: cursored ? 1 : 0
                    border.color: Model.withAlpha(root.accent, 0.7)
                  }

                  MouseArea {
                    id: dayRow
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.openDay(modelData)
                  }

                  Text {
                    id: dayName
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    width: Style.space(84)
                    text: modelData.dayLabel
                    color: index === 0 || parent.opened ? root.foreground : root.muted(0.8)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.body
                    elide: Text.ElideRight
                  }

                  Text {
                    id: dayGlyph
                    anchors.left: dayName.right
                    anchors.verticalCenter: parent.verticalCenter
                    text: Model.conditionGlyph(modelData.weatherCode, true)
                    color: root.accent
                    font.family: Style.font.family
                    font.pixelSize: Style.font.title
                  }

                  Text {
                    id: dayRain
                    anchors.left: dayGlyph.right
                    anchors.leftMargin: Style.space(10)
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.precipitationLabel || ""
                    color: root.muted(0.55)
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                  }

                  Row {
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Style.space(8)

                    Text {
                      text: "UV " + (modelData.uvLabel || "—")
                      color: root.muted(0.5)
                      font.family: Style.font.family
                      font.pixelSize: Style.font.caption
                    }

                    Text {
                      id: dayLow
                      text: modelData.lowLabel || "—"
                      color: root.muted(0.6)
                      font.family: Style.font.family
                      font.pixelSize: Style.font.body
                    }

                    Text {
                      id: dayHigh
                      text: modelData.highLabel || "—"
                      color: root.foreground
                      font.family: Style.font.family
                      font.pixelSize: Style.font.body
                    }
                  }
                }
              }

              Text {
                width: parent.width
                visible: root.dailyRows.length > 0
                text: {
                  var today = root.dailyRows.length > 0 ? root.dailyRows[0] : null
                  if (!today) return ""
                  var parts = []
                  if (today.sunriseLabel) parts.push("sunrise " + today.sunriseLabel)
                  if (today.sunsetLabel) parts.push("sunset " + today.sunsetLabel)
                  return parts.join(" · ")
                }
                color: root.muted(0.45)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }
          }

          // ---- one day, opened up ----------------------------------------------
          // What an opened row in the list above turns into: that day's own figures, then its
          // hours. Fetched for one date only, so reading a day costs one request.
          Card {
            id: dayCard
            width: parent.width
            visible: root.dayDetail !== null || root.dayHint !== ""

            Column {
              width: parent.width
              spacing: Style.space(8)

              Item {
                width: parent.width
                height: Math.max(dayTitle.implicitHeight, dayClose.implicitHeight)

                Text {
                  id: dayTitle
                  anchors.left: parent.left
                  text: root.dayDetail ? root.dayDetail.dayLabel : "Day"
                  color: root.foreground
                  font.family: Style.font.family
                  font.pixelSize: Style.font.subtitle
                }

                Text {
                  id: dayClose
                  anchors.right: parent.right
                  text: "Close"
                  color: root.muted(0.7)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption

                  MouseArea {
                    anchors.fill: parent
                    anchors.margins: -Style.space(4)
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.closeDay()
                  }
                }
              }

              Text {
                width: parent.width
                visible: text !== ""
                text: root.daySummary
                color: root.muted(0.85)
                font.family: Style.font.family
                font.pixelSize: Style.font.body
              }

              HourStrip {
                width: parent.width
                visible: root.dayDetail !== null
                hours: root.dayDetail ? root.dayDetail.hours : []
                foreground: root.foreground
                accent: root.accent
              }

              Text {
                width: parent.width
                visible: text !== ""
                text: root.dayHint
                color: root.muted(0.6)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }

              Text {
                width: parent.width
                visible: root.dayDetail !== null && root.dayDetail.forecastAttribution !== undefined
                text: root.dayDetail
                  ? [root.dayDetail.timezone, root.dayDetail.forecastAttribution].filter(function(part) { return !!part }).join(" · ")
                  : ""
                color: root.muted(0.4)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }
          }

          // ---- air -----------------------------------------------------------
          Card {
            id: airCard
            width: parent.width
            visible: root.air !== null

            Row {
              width: parent.width
              spacing: Style.space(16)

              Column {
                spacing: Style.space(2)

                Text {
                  text: root.air ? root.air.bandLabel : ""
                  color: root.air ? Model.aqiColor(root.air.band, root.theme) : root.foreground
                  font.family: Style.font.family
                  font.pixelSize: Style.font.title
                }

                Text {
                  text: "Air quality"
                  color: root.muted(0.5)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption
                }
              }

              Column {
                spacing: Style.space(1)

                Text {
                  text: root.air && root.air.europeanAqi !== null && root.air.europeanAqi !== undefined
                    ? "European AQI " + Math.round(root.air.europeanAqi)
                    : "European AQI —"
                  color: root.foreground
                  font.family: Style.font.family
                  font.pixelSize: Style.font.body
                }

                Text {
                  text: root.air
                    ? ["PM2.5 " + (root.air.pm25Label || "—"), "PM10 " + (root.air.pm10Label || "—")].join(" · ")
                    : ""
                  color: root.muted(0.55)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption
                }
              }
            }
          }

          // ---- provenance and failures ---------------------------------------
          Column {
            id: footer
            width: parent.width
            spacing: Style.space(4)

            Repeater {
              model: root.view ? root.view.errors : []

              delegate: Text {
                width: parent.width
                wrapMode: Text.WordWrap
                text: "! " + modelData
                color: root.urgent
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }

            Text {
              width: parent.width
              visible: root.errorText !== ""
              wrapMode: Text.WordWrap
              text: root.errorText
              color: root.urgent
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
            }

            Text {
              width: parent.width
              visible: root.hintText !== ""
              wrapMode: Text.WordWrap
              text: root.hintText + " — build and install it with install.sh from the rusty-bui repo"
              color: root.urgent
              font.family: Style.font.family
              font.pixelSize: Style.font.caption
            }

            Text {
              width: parent.width
              visible: root.view !== null
              text: {
                // Name whichever providers actually answered, since a German location reads
                // DWD's model and a Dutch one Open-Meteo's.
                var parts = []
                if (root.nowcast) parts.push(root.nowcast.provider.attribution)
                if (root.view && root.view.forecastAttribution) parts.push(root.view.forecastAttribution)
                if (parts.length === 0) parts.push("© Open-Meteo")
                return parts.join(" · ") + " (CC BY 4.0)"
              }
              color: root.muted(0.35)
              font.family: Style.font.family
              font.pixelSize: Style.font.caption
            }

            Text {
              width: parent.width
              text: "? for the keys · r refresh · Esc close"
              color: root.muted(0.35)
              font.family: Style.font.family
              font.pixelSize: Style.font.caption
            }
          }
        }
      }
    }
  }
}
