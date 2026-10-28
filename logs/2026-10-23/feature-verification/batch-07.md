# Feature verification, batch 07

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page or a community page, and the verdict rests on my reading of it.

C041 | not in Audition | I found no repeat-punch command; a punch is repeated by recording over the range again. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C042 | differs | Repeated recording over the same range stacks takes on the track; there is no cycle (loop) recording mode that records successive passes automatically. | https://helpx.adobe.com/audition/using/recording-audio.html

C044 | differs | Takes are layered and the preferred one is dragged into place and trimmed or split by hand; there is no comp lane that assembles takes. | https://helpx.adobe.com/audition/using/recording-audio.html

C045 | exists | Input monitoring is toggled per track with the I button, and smart monitoring hears input while stopped and track content during playback. | https://helpx.adobe.com/audition/using/recording-audio.html

C046 | exists | The recorded clip's waveform grows in the editor while recording. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C047 | exists | R arms a track and the transport has default keyboard shortcuts including Record. | https://helpx.adobe.com/audition/desktop/keyboard-shortcuts/default-keyboard-shortcuts.html

C048 | exists | Arming several tracks records on all of them at once, given enough hardware inputs. | https://helpx.adobe.com/audition/using/recording-audio.html

C051 | exists | Recording writes to disk-backed temporary files (the media cache folder), not only RAM; I found no sentence on this specifically. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C052 | differs | Device and driver errors surface as dialogs and Adobe has troubleshooting articles; there is no dedicated recording diagnostics report. | https://helpx.adobe.com/audition/kb/troubleshoot-recording-playback-monitoring-audition-1.html (weak)

C060 | exists | Track meters show in the Mixer and Editor for each track, and right-click sets range, peaks and display style. | https://helpx.adobe.com/audition/using/monitoring-recording-playback-levels.html

C061 | exists | The Level Meters panel shows the mix output, and the Mix track has its own meter in the Mixer. | https://helpx.adobe.com/audition/using/monitoring-recording-playback-levels.html

C069 | exists | Under another name: the Frequency Analysis and Spectral Frequency displays update during playback and recording; I found no page that ties them to recording. | https://helpx.adobe.com/audition/using/analyzing-phase-frequency-amplitude.html (weak)

C076 | differs | Session files store file paths only; Export can save copies of associated files (archive style), but I found no documented relink dialog. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html (weak)

C077 | exists | File > Export > Session As Template saves tracks and clips; a template is chosen when creating a new session. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C078 | exists | Sample rate and bit depth are set when a session is created (bit depth cannot be changed later) and all files in a session share the sample rate. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C079 | exists | The Files panel imports files, which can be inserted into the session on separate tracks at the current time. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/importing-files-panel.html

C080 | exists | File > Open Recent lists recent files and sessions; I found no Adobe page that documents it. | https://helpx.adobe.com/audition/using/creating-opening-files.html (weak)

C081 | exists | Edit > Undo and the History panel cover multitrack edits, and Adobe says multitrack clip edits are non-destructive and undoable. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C082 | exists | Zoom buttons, scroll bar and mouse wheel zoom and scroll the timeline. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C083 | exists | View > Time Display switches the ruler between formats including Bars and Beats; the full list was not on pages I could read. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html (weak)

