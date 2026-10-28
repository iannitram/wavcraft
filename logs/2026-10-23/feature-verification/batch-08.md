# Feature verification, batch 08

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page or a community page, and the verdict rests on my reading of it.

C085 | exists | The Markers panel adds, edits and selects point and range markers (also called cues). | https://helpx.adobe.com/audition/using/markers.html

C086 | exists | A time selection and the current-time indicator work in the multitrack editor, including selection of ranges across tracks. | https://helpx.adobe.com/audition/using/selecting-audio.html

C087 | differs | Audition has one current-time indicator that is both cursor and playhead; I found no separate edit cursor. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html (weak)

C088 | exists | Snapping can be toggled in the editor and pulls selection edges and the current-time indicator to targets. | https://helpx.adobe.com/audition/using/selecting-audio.html

C089 | exists | Snap targets are markers, ruler ticks (coarse or fine), zero crossings, clips, loops and frames. | https://helpx.adobe.com/audition/using/selecting-audio.html

C090 | exists | Dragging the current-time indicator or using J, K, L previews audio as you scrub. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C091 | exists | Transport controls play, pause, stop and loop playback in the multitrack editor. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C092 | exists | Loop Playback repeats a selected range. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C093 | exists | Meters show playback levels as well as recording levels. | https://helpx.adobe.com/audition/using/monitoring-recording-playback-levels.html

C094 | differs | Audition has a Metadata panel for file-level fields, not free-text session notes; I found no Adobe page on it. | https://helpx.adobe.com/audition/using/creating-opening-files.html (weak)

C099 | exists | Bus tracks group several tracks or sends under one control. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C102 | exists | I believe multitrack sessions can hold MIDI tracks with virtual instruments; the pages I could read did not describe them. | https://helpx.adobe.com/audition/using/multitrack-editor-overview.html (weak)

C103 | exists | Tracks are added singly or several at a time, deleted (including all empty tracks) and duplicated with clips, effects, EQ and automation. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C104 | exists | Tracks can be reordered by dragging; I did not find this stated on a page. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html (weak)

C105 | differs | Tracks are renamed in the Editor or Mixer and the track colour is set from a swatch; I found no per-track notes field. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html (weak)

C106 | differs | Track height is changed by dragging the divider or with zoom tools; I found no named height presets. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C107 | differs | The Track Visibility panel hides tracks by eye icon and saves visibility presets; I found no track folders or groups. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C108 | exists | Bounce to New Track creates a track holding a clip that reflects a bus or mix track (also available for clips). | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C109 | not in Audition | I found no Freeze Track command; bouncing to a new track is the nearest feature. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html (weak)

C110 | exists | A Polarity Reverse (phase invert) button in the Mixer input controls flips a track's phase. | https://helpx.adobe.com/audition/using/analyzing-phase-frequency-amplitude.html

