import QtQuick
import "Fixture.js" as Fixture

// Stand-in for Quickshell's Process, for the headless render harness.
//
// A command naming the rusty-bui binary is answered from Fixture.js — the payloads the
// binary really produces, captured from live runs — instead of spawning anything, so the
// harness renders exactly what the plugins see on the bar without a network or a build.
// Any other command (the shell's own `hyprctl` calls, for instance) exits non-zero with no
// output, which is the "tool missing" path those callers already handle.
Item {
  id: proc

  property var command: []
  property bool running: false
  property var stdout: null
  property var stderr: null
  property int exitCode: 0

  signal exited(int exitCode)

  readonly property string joinedCommand: (command || []).join(" ")
  readonly property bool servesPayload: joinedCommand.indexOf("rustybui") !== -1
    || joinedCommand.indexOf(" app") !== -1
    || joinedCommand.indexOf(" app ") !== -1
    || joinedCommand.indexOf(" bar") !== -1

  onRunningChanged: {
    if (running) timer.restart()
  }

  function payloadFor() {
    if (joinedCommand.indexOf("prefs") !== -1) return Fixture.prefs
    if (joinedCommand.indexOf("search") !== -1) return Fixture.search
    if (joinedCommand.indexOf("radar") !== -1) return radar()
    if (joinedCommand.indexOf(" day") !== -1) return Fixture.day
    if (joinedCommand.indexOf("app") !== -1) return Fixture.app
    if (joinedCommand.indexOf("bar") !== -1) return Fixture.bar
    return null
  }

  /// The radar image the binary would answer with: the one it was asked for. `rustybui radar
  /// --time <iso>` fetches that frame and reports it as the image, so a stub that always handed
  /// back the fixture's own frame would make a map that steps only when the picture has caught up
  /// look broken — and would hide the difference between the frame asked for and the one shown.
  function radar() {
    var payload = Fixture.radar
    var asked = joinedCommand.match(/--time[= ](\S+)/)
    if (!asked || !payload || !payload.image) return payload
    var at = Date.parse(asked[1])
    if (isNaN(at)) return payload
    var answer = JSON.parse(JSON.stringify(payload))
    answer.image.timeMs = at
    return answer
  }

  Timer {
    id: timer
    interval: 120
    repeat: false
    onTriggered: {
      var payload = proc.servesPayload ? proc.payloadFor() : null
      if (payload !== null && payload !== undefined) {
        // The real collector hands over text; Fixture.js holds JS objects, so serialize
        // the way a process writing JSON on stdout would.
        if (proc.stdout && proc.stdout.deliver) proc.stdout.deliver(JSON.stringify(payload))
        proc.exitCode = 0
      } else {
        proc.exitCode = proc.servesPayload ? 66 : 127
      }
      proc.running = false
      proc.exited(proc.exitCode)
    }
  }
}
