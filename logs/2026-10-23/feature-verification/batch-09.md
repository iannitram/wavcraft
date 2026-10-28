# Feature verification, batch 09

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page or a community page, and the verdict rests on my reading of it. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

C111 | exists | Edit > Extract Channels to Mono Files splits channels out to mono files. | checked in Adobe Audition 26.5 (desktop app)

C112 | exists | Each track's output is set in the Editor or Mixer to hardware, a bus or the Mix track. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C113 | exists | Each track's input is chosen from hardware inputs in the Editor or Mixer. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C114 | exists | Tracks have an Effects Rack of up to 16 effects, and Clip Effects are a separate rack. | https://helpx.adobe.com/audition/using/applying-effects-multitrack-editor.html

C115 | exists | Clips are conformed to the track's channel layout (mono, stereo, 5.1) on insertion; I found no page describing every conversion. | https://helpx.adobe.com/audition/using/multichannel-audio-workflow.html (weak)

C116 | exists | Imported files are inserted as clips on tracks from the Files panel. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/importing-files-panel.html

C117 | exists | The Move tool moves clips in time and between tracks. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C118 | exists | Dragging a clip edge trims or extends it; Clip > Trim also trims to selection or playhead. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C119 | exists | The Razor tool splits at the click and Clip > Split splits all clips at the playhead. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C120 | exists | Edit > Ripple Delete removes the selection and closes the gap. | checked in Adobe Audition 26.5 (desktop app)

C121 | exists | Edit > Copy, Cut and Paste work on clips and ranges. | https://helpx.adobe.com/audition/using/copying-cutting-pasting-deleting-audio.html

C122 | exists | Dragging a clip's edge with loop enabled repeats it. | https://helpx.adobe.com/audition/using/looping-clips.html

C123 | exists | Fade handles sit at a clip's top corners. | https://helpx.adobe.com/audition/using/clip-volume-matching-fading-mixing.html

C124 | differs | The fade menu offers linear and cosine shapes; I found no logarithmic option for clip fades. | https://helpx.adobe.com/audition/using/clip-volume-matching-fading-mixing.html

C125 | exists | Overlapping two clips on a track crossfades them, with the overlap setting its length. | https://helpx.adobe.com/audition/using/clip-volume-matching-fading-mixing.html

C126 | exists | Automatic Crossfades Enabled is an option that crossfades overlaps and can be switched off. | https://helpx.adobe.com/audition/using/clip-volume-matching-fading-mixing.html

C127 | exists | A clip volume handle and the Properties panel change clip gain. | https://helpx.adobe.com/audition/using/clip-volume-matching-fading-mixing.html

C128 | exists | Volume keyframes can be drawn on envelopes; I could not confirm a clip-level gain envelope separate from track volume automation. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html (weak)

C129 | exists | Dragging a clip edge with the stretch key or Properties Stretch settings changes duration without changing pitch (Monophonic, Polyphonic) or with it (Varispeed). | https://helpx.adobe.com/audition/using/multitrack-clip-stretching.html

C130 | exists | The Properties panel has Pitch fields in semitones and cents. | https://helpx.adobe.com/audition/using/multitrack-clip-stretching.html

