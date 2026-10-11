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
  property int hours: Number(setting("hours", 24))
  property bool clock24: setting("clock24", true) !== false
  property bool offline: setting("offline", false) === true

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
              text: root.nowcast ? root.nowcast.provider.attribution + " (CC BY 4.0)" : "© Open-Meteo (CC BY 4.0)"
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
