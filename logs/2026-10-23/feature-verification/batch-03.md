# Feature verification, batch 03

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page I read, and the verdict rests on my reading of it. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

A294 | exists | The shortcut editor warns of a conflict at the bottom of the editor, highlights the command in conflict and enables Undo and Clear. | https://helpx.adobe.com/audition/desktop/keyboard-shortcuts/finding-customizing-shortcuts.html

A305 | exists | Effects > Generate > Speech turns typed or pasted text into a voice-over using the speech libraries of the operating system. | https://helpx.adobe.com/audition/desktop/effects-reference/generate-tones-effect.html

A319 | exists | The Spectral Displays preference Play Only Selected Frequencies When A Spectral Selection Exists, and the Play Spectral Selection Only menu item, switch between selected frequencies and all audio. | https://helpx.adobe.com/audition/desktop/editing-audio-files/displaying-audio-waveform-editor.html

A323 | exists | The Video preferences have a Timecode Overlay (enable, size, background opacity, position, session or media time reference) shown on the Video panel and full-screen monitor. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/importing-video-video-clips.html

A324 | exists | Edit > Snapping > Snap To Frames snaps clips and the playhead to video frames, and the Video panel previews the frame under a clip being moved. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/importing-video-video-clips.html

A326 | exists | Embed Edit Original link data (Markers and Metadata preferences) lets a mixdown file reopen its session through Edit > Edit Original. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/video-applications.html

A333 | exists | Effects > Reverse plays the selection, or the whole waveform, backward. | https://helpx.adobe.com/audition/desktop/editing-audio-files/inverting-reversing-silencing-audio.html

A334 | exists | The Amplify effect has per-channel gain sliders with a link option and works in the Effects Rack; a floating gain HUD also changes level of a selection. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html

A336 | exists | Favorites > Fade In and Fade Out fade the selection, in addition to the visual fade handles. | https://helpx.adobe.com/audition/desktop/editing-audio-files/visually-fading-changing-amplitude.html

A345 | differs | Audition ships for Windows 11 (including Arm) and macOS only; there is no Linux build. | https://helpx.adobe.com/audition/desktop/introduction/system-requirements.html

A349 | exists | Adobe's help center has a Learn section with step-by-step tutorials and hands-on guidance, and release notes. No Help menu page was read. | https://helpx.adobe.com/audition/desktop.html (weak)

A353 | not in Audition | No help page covers screen reader support or a high-contrast display mode. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop.html (weak)

B008 | exists | Favorites > Start Recording Favorite / Stop Recording Favorite record your own actions. | checked in Adobe Audition 26.5 (desktop app)

B009 | differs | Normalize presets are -0.1 dB and -3 dB; there is no 0 dB preset and no Dynamics presets (fade, DC offset and Auto Heal Favorites are documented). | checked in Adobe Audition 26.5 (desktop app)

B014 | exists | Each effect slot has a Power button and the rack has a main Power button, plus Toggle Power State of Selected Effects. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B015 | exists | Effects are reordered by dragging between slots, replaced by choosing another effect in the slot menu, and removed with Remove Effect or Delete. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B016 | exists | The Effects Rack has Input and Output level meters and sliders to set levels. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B017 | differs | The rack has a single Mix slider (100% wet to 0% dry); a per-effect wet control is not described for the rack itself. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B019 | differs | The Waveform Editor rack has a Process menu (selection or entire file) and an Apply button; no help page describes extending the file for effect tails. | https://helpx.adobe.com/audition/desktop/applying-effects/applying-effects-waveform-editor.html (weak)

B020 | exists | In the Multitrack Editor the rack has FX Pre/Post-Fader buttons and a Pre-render Track button. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html
