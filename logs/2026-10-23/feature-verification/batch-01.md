# Feature verification, batch 01

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page I read, and the verdict rests on my reading of it. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

A015 | exists | The toolbar has buttons that switch between the Waveform Editor and Multitrack Editor, and the default shortcut is 1. | https://helpx.adobe.com/audition/desktop/keyboard-shortcuts/default-keyboard-shortcuts.html

A024 | exists | Window > Media Browser opens the Media Browser panel. | checked in Adobe Audition 26.5 (desktop app)

A040 | differs | The current docs have no Phase Analysis panel; the Phase Meter panel (Window > Phase Meter) compares the phase of two chosen channels and shows out-of-phase or mono-compatible audio. | https://helpx.adobe.com/audition/desktop/editing-audio-files/analyzing-phase-frequency-amplitude.html

A041 | differs | Audition's radar loudness view is an effect plug-in (Effects > Special > Loudness Radar Meter, and the Loudness Meter) showing momentary, history, true peak and range, not a dockable panel of its own. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

A044 | exists | Properties (clip and session settings, including tempo), Mixer and Playlist (Window > Playlist) panels all exist; Mixer and session properties belong to the Multitrack Editor, Playlist to the Waveform Editor. | https://helpx.adobe.com/audition/desktop/mixing-multitrack-sessions/multitrack-editor-overview.html

A053 | exists | The Lasso Selection tool makes free-form selections in the spectral display, and Shift-drag adds to one. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A054 | exists | The Paintbrush Selection tool makes free-form selections with a Size setting; its Opacity setting (or layered strokes) sets how strongly effects apply. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A055 | exists | The Spot Healing Brush repairs clicks and pops in the spectral display by running Favorites > Auto Heal on what you click or drag across (selections of four seconds or less). | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A056 | exists | Shift-drag adds to a lasso or paintbrush selection and Alt-drag subtracts from it. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A067 | differs | The Spectral Pitch Display exists only as the pitch view inside the Manual Pitch Correction effect (fundamental in blue, overtones yellow to red); it is not a general alternate spectral view. | https://helpx.adobe.com/audition/desktop/applying-effects/manual-pitch-correction-effect.html

A068 | not in Audition | View offers only Show Spectral Frequency Display and Show Spectral Pitch Display; no spectral phase display. | checked in Adobe Audition 26.5 (desktop app)

A077 | exists | Dragging the Fade In or Fade Out handle makes a linear fade (drag horizontally), logarithmic (drag up or down) or cosine (hold Ctrl or Command); in the Waveform Editor it changes the audio data directly. | https://helpx.adobe.com/audition/desktop/editing-audio-files/visually-fading-changing-amplitude.html

A078 | exists | The General preferences hold a Default Fade setting (cosine by default with modifier keys giving linear or log) and Show HUD for selection ranges only; View > Show HUD hides the gain HUD. | https://helpx.adobe.com/audition/desktop/editing-audio-files/visually-fading-changing-amplitude.html

A100 | exists | Samples is one of the time display formats, counting the actual samples elapsed since the start of the file; the amplitude ruler can also show sample values. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/navigating-time-playing-audio.html

A166 | exists | Edit > Frequency Band Splitter makes up to eight copies of the selection, each limited to a frequency band set by crossover values, each on its own track. | https://helpx.adobe.com/audition/desktop/editing-audio-files/frequency-band-splitter.html

A168 | exists | With the Bars and Beats time format, Edit Tempo opens tempo and time signature settings in the Properties panel. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/navigating-time-playing-audio.html

A187 | not in Audition | Audition 26.3 and later removed CD Layout, the CD marker type and the CD time display; old CD markers become Cue markers when a project opens. | https://helpx.adobe.com/audition/desktop/introduction/removal-of-cd-authoring.html

A189 | exists | The Playlist panel plays all or part of a list of marker ranges, in any order, with a loop count per item. | https://helpx.adobe.com/audition/desktop/editing-audio-files/markers.html

A190 | exists | Playlist items are reordered by dragging up or down and can be removed from the list. | https://helpx.adobe.com/audition/desktop/editing-audio-files/markers.html

A191 | exists | A playlist is stored in the file only when the audio is saved as a WAV file. | https://helpx.adobe.com/audition/desktop/editing-audio-files/markers.html
