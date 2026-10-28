# Batch 14: Audition verification

Checked 2026-10-28 against Adobe's public Audition help (helpx.adobe.com) as found through web search; Adobe's pages refused direct fetches from my tool, so each line rests on the search excerpt of the page cited, not a full read. Format: `row id | verdict | what Audition does | source`. Verdicts: exists, differs (exists under another name or in part), not in Audition (documented as absent or removed, or absent from every page I searched), not found (I could not source it either way). The last two are not proof of absence.

Rows: 19.

C230 | not found | I could not find a Reset Panel control in the Essential Sound documentation I could read. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C235 | exists | Batch Process (File > Save All Audio as Batch Process) writes processed files with chosen format, sample type and output location and names. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C237 | exists | Batch conversion can change sample rate and bit depth through the sample-type options. | https://helpx.adobe.com/audition/using/converting-sample-types.html
C239 | not in Audition | No documented command-line batch; forum answers say batch work is done in the Batch Process panel with Favorites. | https://community.adobe.com/t5/audition-discussions/a-list-of-command-line-options/m-p/14810348 (weak)
C243 | differs | Built-in CD burning was removed in Audition 26.3, but Extract Audio from CD is still there; users export WAV and burn with operating-system tools. | checked in Adobe Audition 26.5 (desktop app)
C245 | not in Audition | Track gaps came from the CD Layout view, which is gone from 26.3. | https://helpx.adobe.com/audition/desktop/introduction/removal-of-cd-authoring.html
C246 | differs | Session templates exist (sample rate, bit depth, sources), and there is a podcast guide, but I could not confirm a shipped podcast template. | https://helpx.adobe.com/audition/using/creating-opening-files.html (weak)
C250 | differs | Markers can be written into saved files; Adobe's podcast guide says AAC supports chapter markers and MP3 does not. | https://helpx.adobe.com/audition/using/creating-podcasts.html
C252 | not in Audition | No built-in speech-to-text; users go to outside services, and the request is still open. | https://community.adobe.com/feature-requests-545/speech-to-text-transcription-1436007 (weak)
C258 | differs | AAC and M4A open directly; export of such formats goes through Adobe Media Encoder. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/supported-file-formats.html
C260 | not found | Opus appears in none of the import or export lists I could read. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/supported-file-formats.html
C261 | differs | WMA (Windows), APE and CAF open; AMR is not listed; other formats export through Media Encoder. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/supported-file-formats.html
C271 | differs | Export with Adobe Media Encoder hands the job to a separate queue app; Audition has no export queue of its own. | https://helpx.adobe.com/audition/desktop/kb/export-ame.html
C274 | differs | Documented route is to export a mixdown and bring it into the video app, or use Media Encoder; no direct video export. | https://helpx.adobe.com/audition/using/video-applications.html
C275 | not found | I found no mention of ADM or immersive export in Audition's documentation. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html
C277 | not found | I found no documentation on streamed export for large files. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C278 | exists | Final Cut Pro Interchange (XML) is a documented exchange format for sessions. | https://helpx.adobe.com/audition/using/saving-exporting-files.html
C279 | not in Audition | Forum reports say Audition cannot import AAF, and none of the docs I read list it (not an Adobe statement). | https://community.adobe.com/t5/audition-discussions/workflow-protools-to-audition/m-p/15025362/highlight/true (weak)
C280 | exists | File > Export > OMF in the Multitrack Editor, encapsulated or referenced, capped at 2 GB when encapsulated. | https://helpx.adobe.com/audition/using/saving-exporting-files.html
