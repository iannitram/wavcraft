# Hand-test checklist for issue #69

Run by a person on a real device. Tick each box with what you saw. If a check fails, open a separate bug issue and link it here. No episode names or library paths go in this file.

## Preconditions (checked on origin/main, 8e4a168)

- Met: the viewer fixes from 2026-10-12 are merged (commit 57cd94e, "Viewer fixes from the 2026-10-12 review", is an ancestor of origin/main). The code for each fix is still present in `apps/wavcraft/src/app.rs` and `apps/wavcraft/src/player.rs`:
  - drag anchor at press origin (`press_origin` in `app.rs`);
  - Space ignores key repeat (`repeat: false` in the key match);
  - window title sent only on change;
  - decode worker disconnect ends "Opening…" with an error (`TryRecvError::Disconnected`);
  - tick labels via `tick_label` (exact to the millisecond);
  - view follows the playhead while playing (`reveal`);
  - one-channel output gets the average of the file's channels (`player.rs`);
  - no output device: Play retries and the status bar says the device stopped (`player.failed()`).
- Met: the spectrogram far-view and overview changes (commits 5ad987d and bfad432) are on main.
- Later changes in the same area: the Space ruling (commits 7feb3c7, 8ba5602, 2a45a6b and later) says Space starts playback and never toggles it. Check item 2 against that ruling.
- Missing: nothing on the code side. Every item below needs a person on a device. The host cannot open a real episode.

## Checks

Use a short test file for items 1 to 9, and a 10-minute file for item 9.

1. [ ] Drag selection starts at the press point. Press at one spot on the waveform and drag a few pixels. The selection's left edge sits under the press point, not 6 px later.
   Seen: ______
2. [ ] Holding Space plays once. Hold Space for several seconds (key repeat on). Playback starts once and does not flip on and off. Space never toggles playback.
   Seen: ______
3. [ ] Idle window does not repaint at full rate. Leave the window idle with a file open. Check the CPU or battery draw in Task Manager (Windows) or the power readout on the handheld. It should be low.
   Seen: ______
4. [ ] A failed decode does not leave "Opening…" up forever. Open a file that is not audio (for example, a renamed text file). The status shows an error and "Opening…" goes away.
   Seen: ______
5. [ ] Ruler labels at 44.1 kHz are right. Zoom to the start of a 44.1 kHz file. Labels step 0:00.001, 0:00.002, 0:00.003 and so on. They do not read one millisecond low.
   Seen: ______
6. [ ] The view follows the playhead. Press Play with the view zoomed in. The view scrolls to keep the playhead on screen. Dragging the view by hand turns follow off for that play.
   Seen: ______
7. [ ] A one-channel output device plays both channels. Set the output to a mono device (or a one-channel device). With a stereo file, both channels are audible, and the Levels meters move on both channels.
   Seen: ______
8. [ ] With no output device, Play does not show a moving playhead. Disable or unplug all output devices, then press Play. The playhead stays still, and the status bar says the output device stopped (Play tries again).
   Seen: ______
9. [ ] Scrolling and zooming the spectrogram on a 10-minute test file. Zoom in and out, and scroll end to end. The display stays smooth, and zoomed-out columns show audio.
   Seen: ______
10. [ ] Open a real full-length episode on Windows and on the Linux handheld. Scrub without stutter. (This is Phase 1's own "done".) Use an episode you own on that device. Do not copy it into the repo.
    Seen (Windows): ______
    Seen (handheld): ______

## Done when

Every box above is ticked with what was seen. Any failure has its own bug issue linked here. Then close #69 yourself.
