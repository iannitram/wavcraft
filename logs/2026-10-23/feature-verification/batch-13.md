# Batch 13: Audition verification

Checked 2026-10-28 against Adobe's public Audition help (helpx.adobe.com) as found through web search; Adobe's pages refused direct fetches from my tool, so most lines rest on the search excerpt of the page cited, not a full read. Some rest on forum or press sources and are marked `(weak)`. C243 and C312 were checked in the Audition desktop app (26.5) instead. Format: `row id | verdict | what Audition does | source`. Verdicts: exists, differs (exists under another name or in part), not in Audition (documented as absent or removed, or absent from every page I searched), not found (I could not source it either way). The last two are not proof of absence. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

Rows: 19.

C203 | exists | Bars and Beats is a time display format, and snapping works against it. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html
C206 | not in Audition | Clip > Stretch has global clip stretching and stretch modes but no option for clips to follow the session tempo. | checked in Adobe Audition 26.5 (desktop app)
C207 | not in Audition | Audition has no MIDI implementation, so there are no MIDI clips to render for a click (forum answers; no Adobe statement). | https://community.adobe.com/t5/audition-discussions/does-audition-cc-have-midi-support/m-p/11345101 (weak)
C209 | differs | A clip can be made a unique copy and edited in the Waveform Editor, where effects are destructive; the multitrack effects rack stays nondestructive. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html
C210 | exists | Multitrack > Bounce To New Track renders clips and effects into a new track with one new clip. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html
C211 | exists | Multitrack > Insert Files places an audio file as a clip on the selected track at the playhead. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html
C214 | exists | The Essential Sound panel assigns a clip a type and shows tools for that type. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C216 | exists | Unify Loudness with Auto Match sets a uniform loudness and shows the LUFS it matched to. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C217 | exists | Dialogue Repair Sound offers sliders for noise, rumble, hum and DeEss (a reverb slider was not visible in the excerpts I read). | https://helpx.adobe.com/audition/using/essential-sound-panel.html (weak)
C218 | exists | Dialogue Improve Clarity has Dynamics, EQ with presets and a vocal enhancer (male/female). | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C219 | exists | Creative options include reverb presets for the clip. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C220 | exists | Music clips get Loudness, Ducking and a Remix duration setting (Remix is covered on its own help page); stereo width and dynamics were not confirmed in the excerpts. | https://helpx.adobe.com/audition/using/essential-sound-panel.html (weak)
C221 | exists | SFX clips get Loudness, reverb presets and panning; EQ was not confirmed in the excerpts. | https://helpx.adobe.com/audition/using/essential-sound-panel.html (weak)
C222 | exists | Ambience gets Loudness, reverb, stereo width and ducking. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C223 | exists | Ducking offers Duck Against, Sensitivity, Reduce By, Fades, Monitor Clip Changes and Reanalyze Clips. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C224 | exists | The panel has built-in presets per type. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C225 | exists | Panel settings can be saved as your own presets. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C226 | exists | Panel changes act on the clip and show in its advanced clip settings, without rewriting the file. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C228 | differs | Audition's panel has the older vocal enhancer (male/female); the AI Enhance Speech feature is documented in Adobe's Premiere Pro help, not Audition's. | https://helpx.adobe.com/uk/premiere-pro/using/enhance-speech-faq.html
