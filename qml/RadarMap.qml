import QtQuick
import QtQuick.Shapes
import qs.Commons
import "Model.js" as Model
import "Basemap.js" as Basemap
import "Legend.js" as Legend

// ─────────────────────────────────────────────────────────────────────────────
// The radar map.
//
// DWD's composite as a georeferenced PNG — the service answers in the projection it was
// asked for, so the image is a lat/lon rectangle — over coastlines and borders vendored
// from Natural Earth, in an equirectangular projection that Model.js does the arithmetic
// for. Drag to move it, wheel or the buttons to zoom, the location's own button to come
// back to it, and the stepper to walk the frames back through the last two hours and
// forward through the nowcast.
//
// The map holds no network of its own. When it needs an image it says so — with the box,
// the size and the frame it wants — and the host fetches it through the binary, the way
// every other number on screen arrives.
// ─────────────────────────────────────────────────────────────────────────────
Item {
  id: map

  property var location: null
  // `rustybui radar --json`: { image: { path, bbox, width, height, timeMs }, framesMs: […] }
  property var radar: null
  property var theme: ({ foreground: "#cacccc", accent: "#3b82f6", urgent: "#ef4444" })
  property bool clock24: true
  // How a span of minutes is written (colon, words or compact), as the preferences say.
  property string durationStyle: "colon"
  /// Whether the map is on screen at all. The window is kept loaded between summons, so a map
  /// left playing would keep fetching images for a window nobody is looking at.
  property bool active: true
  // Said under the map by the host, when there is something to say (a failed fetch, a frame
  // list the service would not give, and so on).
  property string notice: ""

  /// The host's cue to fetch: the box, the size, and the frame (null for the newest).
  signal imageNeeded(var bbox, int width, int height, var timeMs)

  readonly property var image: radar && radar.image ? radar.image : null
  readonly property var frames: Model.frameTimes(radar ? radar.framesMs : [])
  readonly property var attribution: radar && radar.attribution ? radar.attribution : Model.RADAR_ATTRIBUTION

  /// The view: { centerLon, centerLat, spanLon, width, height }, all of it the map's own.
  property var view: null
  property int frameIndex: -1
  /// The radar loop runs on its own: a map that has to be started is a map that mostly sits still.
  property bool playing: true

  /// The frame the picture on screen is actually of — what the stepper should be labelling. A
  /// payload carries one image, so while the loop runs ahead of the service the image is of an
  /// earlier frame; a label taken from `frameIndex` would then put a time on a picture it is not.
  readonly property int shownFrame: {
    if (!frames.length) return -1
    var at = Model.frameIndexAt(frames, map.image ? map.image.timeMs : null)
    return at >= 0 ? at : frameIndex
  }

  // The map takes the width it is given and keeps a map-shaped height, the stepper and the
  // credit line hanging under it.
  property int mapHeight: Math.round(width * 0.62)
  implicitHeight: mapHeight + Style.space(34)

  // ---- the view ------------------------------------------------------------

  /// Frames the location with a couple of degrees across — the Netherlands and a good way
  /// past them, which is a few hours of rain in any direction.
  function reset() {
    if (!location || body.width <= 0) return
    view = Model.clampCenter({
      centerLon: Number(location.longitude),
      centerLat: Number(location.latitude),
      spanLon: 2.6,
      width: body.width,
      height: body.height
    })
  }

  function resized() {
    if (body.width <= 0) return
    if (!view) {
      reset()
      return
    }
    view = Model.clampCenter({
      centerLon: view.centerLon,
      centerLat: view.centerLat,
      spanLon: view.spanLon,
      width: body.width,
      height: body.height
    })
  }

  // The location arrives as a binding, so this fires during construction — when the body may
  // not have been sized yet, which is why a width change also builds the view.
  onLocationChanged: {
    frameIndex = -1
    view = null
    reset()
  }

  onWidthChanged: resized()

  Component.onCompleted: reset()

  onRadarChanged: {
    // Read the frames off the payload rather than the `frames` property: a change handler
    // runs before the bindings that depend on the changed property are re-evaluated.
    var times = Model.frameTimes(radar ? radar.framesMs : [])
    if (!times.length) {
      frameIndex = -1
      return
    }
    if (frameIndex < 0 || frameIndex >= times.length) {
      // A loop starts at the oldest frame it has, or the animation would begin half way through
      // and only reach the beginning after a full pass. Standing still, the useful frame is the
      // one nearest now.
      const wanted = map.playing ? 0 : Model.nowFrameIndex(times, Date.now())
      frameIndex = wanted
      // The request goes out on the next turn of the event loop: a change handler runs before the
      // bindings that depend on the changed property are re-evaluated, so `frames` — and anything
      // computed from it, `goToFrame` included — is still the old list in here.
      Qt.callLater(function() {
        if (view && frames.length) goToFrame(wanted)
      })
    }
    // A payload can leave the map still wanting a different picture — a wider view after a
    // first fetch that was clipped to the coverage, say.
    settle.restart()
  }

  onViewChanged: settle.restart()

  /// Asks the host for an image, if what is on screen needs one.
  function requestIfNeeded() {
    if (!view) return
    var held = map.image ? { path: map.image.path, bbox: map.image.bbox } : null
    if (!Model.needsImage(view, held)) return
    var box = Model.requestBbox(view)
    var size = Model.imageSize(box)
    var time = frames.length > 0 && frameIndex >= 0 ? frames[frameIndex] : null
    map.imageNeeded(box, size.width, size.height, time)
  }

  function goToFrame(index) {
    if (!frames.length || !view) return
    frameIndex = Math.max(0, Math.min(frames.length - 1, index))
    var box = Model.requestBbox(view)
    var size = Model.imageSize(box)
    map.imageNeeded(box, size.width, size.height, frames[frameIndex])
  }

  // One request per movement, not one per mouse-move event.
  Timer {
    id: settle
    interval: 300
    repeat: false
    onTriggered: map.requestIfNeeded()
  }

  Timer {
    id: playback
    interval: 700
    repeat: true
    // Two frames is the least that can move; a hidden window plays to nobody.
    running: map.playing && map.active && map.frames.length > 1
    onTriggered: map.stepForward(false)
  }

  // A frame that never arrives must not end the animation. After a pause long enough for a slow
  // answer, the loop walks on regardless and the label keeps telling the truth about which frame
  // is on screen, rather than the two of them disagreeing for the rest of the session.
  Timer {
    id: stalled
    interval: 6000
    repeat: false
    onTriggered: map.stepForward(true)
  }

  /// One frame on, wrapping at the end: a radar loop that halts at the last frame is a loop that
  /// has to be started again every time. Unless `force`, a step waits for the picture to catch up
  /// with the frame that was asked for, so the map never outruns the service and never labels a
  /// picture with a time it is not.
  function stepForward(force) {
    if (!frames.length) return
    if (!force && shownFrame !== frameIndex) return
    goToFrame(frameIndex >= frames.length - 1 ? 0 : frameIndex + 1)
    stalled.restart()
  }

  // ---- the map itself ------------------------------------------------------

  Rectangle {
    id: body
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    height: map.mapHeight
    radius: Style.cornerRadius
    color: Model.withAlpha(map.theme.foreground, 0.06)
    border.width: 1
    border.color: Model.withAlpha(map.theme.foreground, 0.16)
    clip: true

    // The basemap: coastlines and borders, drawn from the vendored Natural Earth vectors as
    // two paths in screen coordinates. A Shape rather than a Canvas, because Shape draws
    // geometry instead of into a texture — and a texture is what the offscreen renderer
    // cannot photograph, which would leave this layer unverified.
    Shape {
      id: world
      anchors.fill: parent
      antialiasing: true

      // The view the paths were built for, and the paths themselves. A binding rather than a
      // handler, because it must be rebuilt when the shape is *sized* as well as when the view
      // moves — a path built for a zero-width shape is a path that draws nothing at all.
      property var drawn: map.view
      property var built: build(drawn)

      function build(view) {
        if (!view || width <= 0 || height <= 0) return { land: "", borders: "", runs: 0, costMs: 0 }
        var started = Date.now()
        var land = Model.svgPath(view, Basemap.land, width, height)
        var borders = Model.svgPath(view, Basemap.borders, width, height)
        return {
          land: land,
          borders: borders,
          runs: Model.drawnRuns(land),
          costMs: Date.now() - started
        }
      }

      ShapePath {
        strokeColor: Model.withAlpha(map.theme.foreground, 0.5)
        strokeWidth: 1
        fillColor: "transparent"
        PathSvg { path: world.built ? world.built.land : "" }
      }

      ShapePath {
        strokeColor: Model.withAlpha(map.theme.foreground, 0.2)
        strokeWidth: 1
        fillColor: "transparent"
        PathSvg { path: world.built ? world.built.borders : "" }
      }
    }

    Image {
      id: rain
      source: map.image ? "file://" + map.image.path : ""
      visible: map.image !== null
      // Nearest-neighbour: a radar raster is data, and smoothing it invents rates that are
      // not in the composite.
      smooth: false
      cache: false
      opacity: 0.95

      // The image is a lat/lon rectangle, so its corners land on the view's axes — up to
      // the y of its northern edge and across to the x of its eastern one.
      readonly property var topLeft: map.image && map.view
        ? Model.project(map.view, map.image.bbox[0], map.image.bbox[3]) : null
      readonly property var bottomRight: map.image && map.view
        ? Model.project(map.view, map.image.bbox[2], map.image.bbox[1]) : null

      x: topLeft ? topLeft.x : 0
      y: topLeft ? topLeft.y : 0
      width: topLeft && bottomRight ? bottomRight.x - topLeft.x : 0
      height: topLeft && bottomRight ? bottomRight.y - topLeft.y : 0
    }

    // The location: a ring that reads against both a dark basemap and a bright radar edge.
    Item {
      visible: map.location !== null && map.view !== null
      readonly property var at: map.location && map.view
        ? Model.project(map.view, map.location.longitude, map.location.latitude) : null
      x: at ? at.x : 0
      y: at ? at.y : 0

      Rectangle {
        anchors.centerIn: parent
        width: Style.space(11)
        height: width
        radius: width / 2
        color: "transparent"
        border.width: 1
        border.color: Model.withAlpha(map.theme.foreground, 0.75)
      }

      Rectangle {
        anchors.centerIn: parent
        width: Style.space(4)
        height: width
        radius: width / 2
        color: map.theme.foreground
      }
    }

    MouseArea {
      id: panning
      anchors.fill: parent
      acceptedButtons: Qt.LeftButton
      cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
      property real lastX: 0
      property real lastY: 0

      onPressed: function(mouse) {
        lastX = mouse.x
        lastY = mouse.y
      }

      onPositionChanged: function(mouse) {
        if (!pressed || !map.view) return
        map.view = Model.panBy(map.view, mouse.x - lastX, mouse.y - lastY)
        lastX = mouse.x
        lastY = mouse.y
      }

      // Zoom where the pointer is, so the place under it stays there.
      onWheel: function(wheel) {
        if (!map.view) return
        var factor = Math.pow(1.4, -wheel.angleDelta.y / 120)
        map.view = Model.zoomAt(map.view, factor, wheel.x, wheel.y)
      }

      onDoubleClicked: map.reset()
    }
  }

  // ---- the chrome over it --------------------------------------------------

  // What the colours mean: DWD's own bands, in DWD's own colours (qml/Legend.js, generated from
  // the legend the service publishes), drawn small in the theme's font rather than shipped as
  // the 100x390 white plate their GetLegendGraphic returns. A map whose colours are unexplained
  // is a map you have to already know how to read.
  Rectangle {
    id: legend
    anchors.left: body.left
    anchors.top: body.top
    anchors.margins: Style.space(4)
    // Two columns of eight rather than one of fifteen, and no legend at all on a map too narrow
    // to hold one: this is a caption, not the subject.
    readonly property var bands: Model.evenColumns(Legend.RAMP, 2)
    visible: body.width >= Style.space(380)
    width: legendBody.width + Style.space(10)
    height: legendBody.height + Style.space(8)
    radius: Style.cornerRadius
    color: Model.withAlpha(map.theme.background, 0.72)
    border.width: 1
    border.color: Model.withAlpha(map.theme.foreground, 0.16)

    Column {
      id: legendBody
      x: Style.space(5)
      y: Style.space(4)
      spacing: Style.space(3)

      Text {
        text: "rain, " + Legend.TITLE
        color: Model.withAlpha(map.theme.foreground, 0.6)
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
      }

      Row {
        id: legendColumns
        spacing: Style.space(6)

        Repeater {
          model: legend.bands

          delegate: Column {
            required property var modelData
            spacing: Style.space(1)

            Repeater {
              model: modelData

              delegate: Row {
                required property var modelData
                spacing: Style.space(3)

                Rectangle {
                  anchors.verticalCenter: parent.verticalCenter
                  width: Style.space(10)
                  height: Style.space(5)
                  radius: 1
                  color: modelData.color
                }

                Text {
                  anchors.verticalCenter: parent.verticalCenter
                  text: Model.rampLabel(modelData.label)
                  color: Model.withAlpha(map.theme.foreground, 0.85)
                  font.family: Style.font.family
                  font.pixelSize: Style.font.caption
                }
              }
            }
          }
        }
      }
    }
  }

  Row {
    anchors.top: body.top
    anchors.right: body.right
    anchors.margins: Style.space(4)
    spacing: Style.space(3)

    Repeater {
      model: [
        { action: "in", label: "+" },
        { action: "out", label: "−" },
        { action: "reset", label: map.location ? String(map.location.name) : "here" }
      ]

      delegate: Rectangle {
        required property var modelData
        width: Math.max(Style.space(16), chip.implicitWidth + Style.space(8))
        height: Style.space(16)
        radius: Style.cornerRadius
        color: hovered.hovered
          ? Model.withAlpha(map.theme.foreground, 0.18)
          : Model.withAlpha(map.theme.foreground, 0.08)

        Text {
          id: chip
          anchors.centerIn: parent
          text: modelData.label
          color: map.theme.foreground
          font.family: Style.font.family
          font.pixelSize: Style.font.caption
        }

        HoverHandler { id: hovered }

        MouseArea {
          anchors.fill: parent
          cursorShape: Qt.PointingHandCursor
          onClicked: {
            if (!map.view) return
            if (modelData.action === "reset") map.reset()
            else map.view = Model.zoomAt(map.view, modelData.action === "in" ? 1 / 1.4 : 1.4, body.width / 2, body.height / 2)
          }
        }
      }
    }
  }

  // The scale bar, in the bottom-left corner of the map.
  Item {
    anchors.left: body.left
    anchors.bottom: body.bottom
    anchors.margins: Style.space(4)
    width: scaleLabel.width + scaleLine.width + Style.space(4)
    height: scaleLabel.height

    Rectangle {
      id: scaleLine
      anchors.verticalCenter: parent.verticalCenter
      width: map.view ? Model.scaleBar(map.view).pixels : 0
      height: 1
      color: Model.withAlpha(map.theme.foreground, 0.55)
    }

    Text {
      id: scaleLabel
      anchors.right: scaleLine.left
      anchors.rightMargin: Style.space(4)
      anchors.verticalCenter: parent.verticalCenter
      text: map.view ? Model.scaleBar(map.view).km + " km" : ""
      color: Model.withAlpha(map.theme.foreground, 0.65)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption
    }
  }

  // ---- the frame stepper ---------------------------------------------------

  Row {
    id: stepper
    anchors.top: body.bottom
    anchors.topMargin: Style.space(4)
    anchors.left: parent.left
    anchors.right: parent.right
    height: Style.space(16)
    spacing: Style.space(4)

    Text {
      anchors.verticalCenter: parent.verticalCenter
      text: "◀"
      color: map.frames.length && map.frameIndex > 0
        ? Model.withAlpha(map.theme.foreground, 0.8)
        : Model.withAlpha(map.theme.foreground, 0.25)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption

      MouseArea {
        anchors.fill: parent
        anchors.margins: -Style.space(3)
        cursorShape: Qt.PointingHandCursor
        onClicked: map.goToFrame(map.frameIndex - 1)
      }
    }

    Text {
      id: frameLabel
      anchors.verticalCenter: parent.verticalCenter
      width: Math.max(Style.space(34), implicitWidth)
      horizontalAlignment: Text.AlignHCenter
      // The frame's own clock, and how far off it is: the extent runs from an hour or two back
      // to two hours ahead, so "20:15" alone does not say whether that is past or forecast.
      text: {
        if (!(map.shownFrame >= 0 && map.frames.length)) return "–"
        var clock = Model.formatClock(map.frames[map.shownFrame], map.clock24)
        var age = Model.frameAge(map.frames[map.shownFrame], Date.now(), map.durationStyle)
        return age === "" || age === "now" ? clock : clock + " · " + age
      }
      color: map.theme.foreground
      font.family: Style.font.family
      font.pixelSize: Style.font.caption
    }

    Text {
      anchors.verticalCenter: parent.verticalCenter
      text: "▶"
      color: map.frames.length && map.frameIndex >= 0 && map.frameIndex < map.frames.length - 1
        ? Model.withAlpha(map.theme.foreground, 0.8)
        : Model.withAlpha(map.theme.foreground, 0.25)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption

      MouseArea {
        anchors.fill: parent
        anchors.margins: -Style.space(3)
        cursorShape: Qt.PointingHandCursor
        onClicked: map.goToFrame(map.frameIndex + 1)
      }
    }

    Text {
      anchors.verticalCenter: parent.verticalCenter
      text: map.playing ? "‖" : "▶"
      color: Model.withAlpha(map.theme.foreground, map.frames.length ? 0.8 : 0.25)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption

      MouseArea {
        anchors.fill: parent
        anchors.margins: -Style.space(3)
        cursorShape: Qt.PointingHandCursor
        onClicked: {
          if (!map.frames.length) return
          // Playing from the end would show four frames and stop; start over instead.
          if (!map.playing && map.frameIndex >= map.frames.length - 1) map.goToFrame(0)
          map.playing = !map.playing
        }
      }
    }

    // Where you are in the nowcast: one mark per frame, brightest at the one on screen, plus
    // the present moment, so the forecast half of the extent is visible as such.
    Item {
      id: track
      anchors.verticalCenter: parent.verticalCenter
      width: Math.max(0, stepper.width - Style.space(190))
      height: Style.space(6)

      Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width
        height: 2
        radius: 1
        color: Model.withAlpha(map.theme.foreground, 0.15)
      }

      Repeater {
        model: map.frames
        delegate: Rectangle {
          required property int index
          width: Style.space(3)
          height: Style.space(3)
          radius: width / 2
          color: index === map.shownFrame
            ? map.theme.accent
            : Model.withAlpha(map.theme.foreground, 0.4)
          x: map.frames.length > 1
            ? index * (track.width - width) / (map.frames.length - 1)
            : 0
          anchors.verticalCenter: parent.verticalCenter
        }
      }

      // The present, between the frames either side of it.
      Rectangle {
        readonly property real at: {
          if (map.frames.length < 2) return -1
          var index = Model.nowFrameIndex(map.frames, Date.now())
          return index * (track.width - Style.space(3)) / (map.frames.length - 1)
        }
        visible: at >= 0
        x: Math.round(at + Style.space(1))
        anchors.verticalCenter: parent.verticalCenter
        width: 1
        height: Style.space(8)
        color: Model.withAlpha(map.theme.urgent, 0.7)
      }
    }
  }

  Text {
    anchors.top: stepper.bottom
    anchors.topMargin: Style.space(2)
    anchors.left: parent.left
    text: {
      var parts = [map.attribution]
      if (map.notice !== "") parts.unshift(map.notice)
      else if (map.view) parts.push(Math.round(Model.kmPerPixel(map.view) * 100) / 100 + " km per pixel")
      return parts.join(" · ")
    }
    color: Model.withAlpha(map.theme.foreground, map.notice === "" ? 0.5 : 0.75)
    font.family: Style.font.family
    font.pixelSize: Style.font.caption
  }

  // ---- drawing ------------------------------------------------------------

  /// What the basemap drew, last time it drew: the runs that survived the clip and how long
  /// the projection took. Exposed so a harness (or a curious user) can see that the layer is
  /// neither empty nor slow, rather than having to trust that it is.
  readonly property var basemap: world.built
}
