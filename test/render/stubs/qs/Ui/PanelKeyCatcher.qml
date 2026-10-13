import QtQuick

// Stand-in for qs.Ui.PanelKeyCatcher: the panel's focus scope, which reports Esc and
// tab switches to the plugin. Key handling itself belongs to the shell, so the harness
// only carries the signals.
Item {
  id: root

  signal closeRequested()
  signal tabRequested(var direction)
}
