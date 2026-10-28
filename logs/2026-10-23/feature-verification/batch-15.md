# Batch 15: Audition verification

Checked 2026-10-28 against Adobe's public Audition help (helpx.adobe.com) as found through web search; Adobe's pages refused direct fetches from my tool, so most lines rest on the search excerpt of the page cited, not a full read. Some rest on forum or press sources and are marked `(weak)`. C243 and C312 were checked in the Audition desktop app (26.5) instead. Format: `row id | verdict | what Audition does | source`. Verdicts: exists, differs (exists under another name or in part), not in Audition (documented as absent or removed, or absent from every page I searched), not found (I could not source it either way). The last two are not proof of absence. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

Rows: 19.

C281 | exists | In Premiere, Edit > Edit in Adobe Audition sends a clip or a sequence, and the saved result returns to the timeline. | https://helpx.adobe.com/premiere/desktop/use-premiere-with-other-apps/working-with-other-adobe-applications/edit-premiere-sequences-in-audition.html
C282 | exists | Sending a sequence can use Dynamic Link so Audition streams the video without rendering. | https://helpx.adobe.com/premiere/desktop/use-premiere-with-other-apps/working-with-other-adobe-applications/edit-premiere-sequences-in-audition.html
C283 | not found | I found no mention of OpenTimelineIO in Audition's documentation. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C284 | exists | A video clip can be inserted into a session and previews in the Video panel, one video clip at a time. | https://helpx.adobe.com/audition/using/importing-video-video-clips.html
C286 | exists | The time display can be set to the video's SMPTE rate, with Snap To Frames and a Video Frame Rate status readout. | https://helpx.adobe.com/audition/using/importing-video-video-clips.html
C288 | differs | No replace-audio command; the documented way is a mixdown re-imported into the video application, or overwriting the original file by name. | https://helpx.adobe.com/audition/using/video-applications.html
C290 | not in Audition | Forum reports show Pro Tools sessions are not opened directly; none of the docs list it (not an Adobe statement). | https://community.adobe.com/t5/audition-discussions/workflow-protools-to-audition/m-p/15025362/highlight/true (weak)
C291 | not in Audition | File > Import lists File, Raw Data, Application Settings and Markers from File; no MIDI import. | checked in Adobe Audition 26.5 (desktop app)
C292 | differs | SMPTE time formats and control-surface sync (Mackie HUI, FaderPort) exist; chasing external timecode is not documented. | https://helpx.adobe.com/audition/using/control-surface-support.html
C294 | differs | The Track Panner pans mono and stereo tracks; for 5.1 you send a track to a 5.1 bus and pan the send. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C297 | not found | I found no description of surround-specific meters in the pages I could read. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C299 | not found | I found no mention of Dolby Atmos or immersive formats in Audition's documentation. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C301 | exists | Audition 26.0 runs natively on Windows on ARM; native Apple silicon came in May 2021 (press report). | https://helpx.adobe.com/audition/using/whats-new/2026.html (weak)
C310 | not in Audition | File > Export lists file, markers, mixdown, Premiere, Media Encoder, session, FCP XML, OMF and settings; no share or upload. | checked in Adobe Audition 26.5 (desktop app)
C311 | not found | I found no cloud sync or library feature for Audition sessions. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C312 | exists | Scripts run from File > Browse and Run Script...; scripting exists in Audition 26.5. | checked in Adobe Audition 26.5 (desktop app)
C314 | differs | Audition has Generate Speech using the operating system's voices; AI Enhance Speech is documented in Adobe's Premiere Pro help, not Audition's, and no auto-tagging was found. | https://helpx.adobe.com/audition/desktop/using/text-to-speech.html
C323 | not in Audition | Not supported; a community request asks for DAWproject. | https://community.adobe.com/t5/audition-ideas/please-add-support-for-the-new-dawproject-session-exchange-format-it-s-free-and-open-source/idi-p/14127897 (weak)
C324 | not found | I found no public Adobe documentation of a crash-safe export finalize step in Audition. | not found in Adobe's public Audition docs (searched 2026-10-28); app check pending
