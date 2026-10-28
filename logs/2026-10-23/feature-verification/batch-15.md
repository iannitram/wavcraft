# Batch 15: Audition verification

Checked 2026-10-28 against Adobe's public Audition help (helpx.adobe.com) as found through web search; Adobe's pages refused direct fetches from my tool, so each line rests on the search excerpt of the page cited, not a full read. Format: `row id | verdict | what Audition does | source`. Verdicts: exists, differs (exists under another name or in part), not in Audition (documented as absent or removed, or absent from every page I searched), not found (I could not source it either way). The last two are not proof of absence.

Rows: 19.

C281 | exists | In Premiere, Edit > Edit in Adobe Audition sends a clip or a sequence, and the saved result returns to the timeline. | https://helpx.adobe.com/premiere/desktop/use-premiere-with-other-apps/working-with-other-adobe-applications/edit-premiere-sequences-in-audition.html
C282 | exists | Sending a sequence can use Dynamic Link so Audition streams the video without rendering. | https://helpx.adobe.com/premiere/desktop/use-premiere-with-other-apps/working-with-other-adobe-applications/edit-premiere-sequences-in-audition.html
C283 | not found | I found no mention of OpenTimelineIO in Audition's documentation. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C284 | exists | A video clip can be inserted into a session and previews in the Video panel, one video clip at a time. | https://helpx.adobe.com/audition/using/importing-video-video-clips.html
C286 | exists | The time display can be set to the video's SMPTE rate, with Snap To Frames and a Video Frame Rate status readout. | https://helpx.adobe.com/audition/using/importing-video-video-clips.html
C288 | differs | No replace-audio command; the documented way is a mixdown re-imported into the video application, or overwriting the original file by name. | https://helpx.adobe.com/audition/using/video-applications.html
C290 | not in Audition | Forum reports show Pro Tools sessions are not opened directly; none of the docs list it (not an Adobe statement). | https://community.adobe.com/t5/audition-discussions/workflow-protools-to-audition/m-p/15025362/highlight/true
C291 | not in Audition | Audition has no MIDI implementation, so no MIDI file import or export (forum answers; no Adobe statement). | https://community.adobe.com/t5/audition-discussions/does-audition-cc-have-midi-support/m-p/11345101
C292 | differs | SMPTE time formats and control-surface sync (Mackie HUI, FaderPort) exist; chasing external timecode is not documented. | https://helpx.adobe.com/audition/using/control-surface-support.html ; https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html
C294 | differs | The Track Panner pans mono and stereo tracks; for 5.1 you send a track to a 5.1 bus and pan the send. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C297 | not found | I found no description of surround-specific meters in the pages I could read. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C299 | not found | I found no mention of Dolby Atmos or immersive formats in Audition's documentation. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C301 | exists | Audition 26.0 runs natively on Windows on ARM; native Apple silicon came in May 2021 (press report). | https://helpx.adobe.com/audition/using/whats-new/2026.html ; https://appleinsider.com/articles/21/05/11/adobe-audition-now-runs-natively-on-apple-silicon
C310 | not found | I found no share or upload command in Audition's documentation. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C311 | not found | I found no cloud sync or library feature for Audition sessions. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C312 | not found | I found no documented scripting API; forum answers say the old scripting was never restored. | https://community.adobe.com/t5/audition-discussions/a-list-of-command-line-options/m-p/14810348
C314 | differs | Audition has Generate Speech using the operating system's voices; AI Enhance Speech is documented for Premiere Pro, and no auto-tagging was found. | https://helpx.adobe.com/audition/desktop/using/text-to-speech.html ; https://helpx.adobe.com/uk/premiere-pro/using/enhance-speech-faq.html
C323 | not in Audition | Not supported; a community request asks for DAWproject. | https://community.adobe.com/t5/audition-ideas/please-add-support-for-the-new-dawproject-session-exchange-format-it-s-free-and-open-source/idi-p/14127897
C324 | not in Audition | A WavCraft-only item from reuse research; Audition's docs describe no such step. | (no Adobe page; internal reuse note)
