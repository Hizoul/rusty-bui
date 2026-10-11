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
    if (joinedCommand.indexOf("app") !== -1) return Fixture.app
    if (joinedCommand.indexOf("bar") !== -1) return Fixture.bar
    return null
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
