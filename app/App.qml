import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
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
  property bool closingFromHost: false

  function open(payloadJson) {
    closingFromHost = false
    window.visible = true
    Qt.callLater(function() {
      if (keyScope) keyScope.forceActiveFocus()
      root.loadPrefs()
      root.refresh()
    })
  }

  // Host-initiated close (`omarchy-shell shell hide …`): the host already knows,
  // so do not report back.
  function close() {
    closingFromHost = true
    window.visible = false
    closingFromHost = false
  }

  // User-initiated close (Esc, window close button): tell the shell, so its open
  // panel bookkeeping stays consistent and `toggle` works next time.
  function requestClose() {
    if (shell && typeof shell.hide === "function") shell.hide("mmb.weather")
    else window.visible = false
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
  property bool offline: pref("offline", "offline", setting("offline", false) === true) === true

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
        try {
          var found = JSON.parse(text)
          root.searchResults = found
          root.searchHint = found.length === 0 ? "no matches" : ""
        } catch (error) {
          root.searchResults = []
          root.searchHint = "search failed"
        }
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
      if (exitCode !== 0) root.searchHint = "could not store that preference"
    }
  }

  function setPref(name, value) {
    root.pendingPrefs = { name: name, value: value }
    Qt.callLater(function() {
      if (!savePrefsProc.running) savePrefsProc.running = true
    })
  }

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
    color: root.background
    implicitWidth: Style.space(620)
    implicitHeight: Style.space(780)
    // The shape Omarchy's own dev-gallery uses; Quickshell's FloatingWindow adds both to
    // QQuickWindow, so the window still has to be checked against the real shell and not
    // only against the render-harness stub.
    minimumSize: Qt.size(Style.space(480), Style.space(480))

    onVisibleChanged: {
      if (!visible && !root.closingFromHost && root.shell && typeof root.shell.hide === "function")
        root.shell.hide("mmb.weather")
    }

    FocusScope {
      id: keyScope
      anchors.fill: parent
      focus: true

      Keys.onEscapePressed: root.requestClose()
      Keys.onPressed: function(event) {
        if (event.key === Qt.Key_R || event.key === Qt.Key_F5) {
          root.refresh()
          event.accepted = true
        }
      }

      Flickable {
        id: scroller
        anchors.fill: parent
        anchors.margins: Style.spacing.panelPadding
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height

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
                  if (root.showSettings) root.loadPrefs()
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
                    }
                  ]

                  delegate: Rectangle {
                    width: (parent.width - Style.space(12)) / 3
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
            visible: root.view !== null && root.view.alerts && root.view.alerts.length > 0
            // The theme's urgent role, tinted, so a warning reads as one at a glance.
            color: Model.withAlpha(root.urgent, 0.14)
            borderSpec: Border.surfaceSpec("popups", "border", root.urgent, Math.max(1, Style.space(1)))

            Column {
              width: parent.width
              spacing: Style.space(8)

              Text {
                id: alertsHeading
                text: {
                  if (!root.view) return ""
                  var count = root.view.alerts.length
                  return count === 1 ? "1 warning in force" : count + " warnings in force"
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

          // ---- rain nowcast --------------------------------------------------
          Card {
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

          // ---- next hours ----------------------------------------------------
          Card {
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

              Flickable {
                width: parent.width
                height: Style.space(96)
                contentWidth: hourRow.implicitWidth
                contentHeight: height
                clip: true
                flickableDirection: Flickable.HorizontalFlick

                Row {
                  id: hourRow
                  spacing: Style.space(10)

                  Repeater {
                    model: root.hourlyRows

                    delegate: Column {
                      width: Style.space(46)
                      spacing: Style.space(3)

                      Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: modelData.clockLabel
                        color: root.muted(0.6)
                        font.family: Style.font.family
                        font.pixelSize: Style.font.caption
                      }

                      Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: Model.conditionGlyph(modelData.weatherCode, modelData.isDaylight)
                        color: root.accent
                        font.family: Style.font.family
                        font.pixelSize: Style.font.title
                      }

                      Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: modelData.temperatureLabel || "—"
                        color: root.foreground
                        font.family: Style.font.family
                        font.pixelSize: Style.font.body
                      }

                      Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: modelData.probabilityLabel || ""
                        color: root.muted(0.5)
                        font.family: Style.font.family
                        font.pixelSize: Style.font.caption
                      }
                    }
                  }
                }
              }
            }
          }

          // ---- days ----------------------------------------------------------
          Card {
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

                  Text {
                    id: dayName
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    width: Style.space(84)
                    text: modelData.dayLabel
                    color: index === 0 ? root.foreground : root.muted(0.8)
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

          // ---- air -----------------------------------------------------------
          Card {
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
              text: "R refresh · Esc close"
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
