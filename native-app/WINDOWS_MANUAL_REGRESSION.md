# Windows Manual Regression Checklist

Hardware-dependent checks below are pending on the current Phase 8.4 build.
Run them on the intended Windows display setup and record the OS scaling and
monitor arrangement when reporting results.

## Display capture and geometry

- [ ] Capture and select a region on a single monitor.
- [ ] Select a secondary monitor from Settings and confirm the Overlay targets it.
- [ ] Put a monitor left of and then above the primary; verify capture, pointer
  selection, crop bounds, and exported pixels across negative desktop origins.
- [ ] Use displays at different scaling values (including 100%, 125%, and
  150%); move or reconfigure the Overlay while it is open and verify the
  viewport follows the effective DPI without changing the captured frame.
- [ ] After a DPI transition, select a known region and verify crop dimensions
  and pixels, annotations, mosaic redaction, PNG export, and clipboard paste.
- [ ] Disconnect or rearrange a monitor before capture and during an Overlay
  session; confirm stale display geometry is rejected safely and capture can
  be started again after the layout stabilizes.

## Lifecycle and regression paths

- [ ] Start capture with `Ctrl+Shift+S` and through the tray menu.
- [ ] Verify Settings hide/restore behavior and tray availability during capture.
- [ ] Finish with Done and cancel with Escape; confirm the Overlay disappears
  without a white flash or visible window-shrinking transition.
- [ ] Repeat captures and confirm the hotkey, tray, crop, annotation, and export
  paths continue to work.

## Overlay taskbar regression

- [ ] Start Carambola and confirm the Settings window's taskbar behavior is unchanged.
- [ ] Trigger a screenshot using the global `Ctrl+Shift+S` hotkey.
- [ ] On the first capture and on later captures, watch the taskbar while the
  Overlay opens, remains fullscreen, and closes. No iconless Carambola entry
  should appear.
- [ ] Exit the Overlay using Escape; confirm the iconless taskbar entry does not appear during closure.
- [ ] Trigger another screenshot and exit using the normal completion action; confirm the entry remains absent.
- [ ] Repeat both exit paths several times, including after opening and closing Settings.
- [ ] Confirm the tray icon remains available and Carambola continues running in the background.
