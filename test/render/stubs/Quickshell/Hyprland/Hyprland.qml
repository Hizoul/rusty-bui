pragma Singleton
import QtQuick

// The shell's Commons/Style.qml imports Quickshell.Hyprland (for `hyprctl getoption` on
// startup). The harness only needs the module to exist; the value reads land on each
// caller's fallback because the Process stub answers nothing.
QtObject {
  property var toplevels: []
  property var monitors: []

  // The shell's Style.qml connects to this; without it, that file logs a noisy
  // "no signal of the target matches the name" on every render.
  signal rawEvent(var event)
}
