# Batch 12: Audition verification

Checked 2026-10-28 against Adobe's public Audition help (helpx.adobe.com) as found through web search; Adobe's pages refused direct fetches from my tool, so each line rests on the search excerpt of the page cited, not a full read. Format: `row id | verdict | what Audition does | source`. Verdicts: exists, differs (exists under another name or in part), not in Audition (documented as absent or removed, or absent from every page I searched), not found (I could not source it either way). The last two are not proof of absence.

Rows: 19.

C180 | differs | Clip envelopes belong to the clip so they travel with it; track envelopes stay put on the track, and I found no option that moves them with clips (inferred from the envelope description). | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html (weak)
C181 | not found | Nothing I found describes snapping keyframes to a grid; general snapping covers clips and playhead. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html
C182 | not found | I found no description of copying and pasting envelope keyframes between clips or tracks. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html
C183 | exists | Setting a track to Off ignores its envelopes in playback and mixdown while still showing them. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html
C184 | differs | Ducking is a panel feature (Duck Against, Sensitivity, Reduce By, Fades) on Music/Ambience clips, not a command that writes editable volume keyframes. | https://helpx.adobe.com/audition/using/essential-sound-panel.html
C185 | exists | File > Export > Multitrack Mixdown renders the session or a part of it to a file. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C186 | differs | Mixdown Options can write each track as its own file (and the master); I found no documented bus-stem option. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html (weak)
C187 | exists | Mixdown Options can output mono, stereo and 5.1 mixdowns at the same time. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C189 | exists | The mixdown covers Entire Session, Time Selection or Selected Clips. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C191 | differs | Dithering lives in the sample-type conversion options; I did not find it spelled out on the mixdown dialog itself. | https://helpx.adobe.com/audition/using/converting-sample-types.html (weak)
C192 | not in Audition | No bounce or mixdown report is documented; loudness is measured separately in Match Loudness and the Loudness meter. | https://helpx.adobe.com/audition/using/match-loudness.html (weak)
C194 | not found | I found nothing about a render-tail or tail-length option for effects at mixdown. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html
C195 | exists | Remix on a clip takes a Target Duration (by typing or by dragging the clip edge) and rearranges the music to fit. | https://helpx.adobe.com/audition/using/creating-remix.html
C196 | exists | Remix has Short and Long segment settings, Maximum Slack and Minimum Loop for tuning the result. | https://helpx.adobe.com/audition/using/creating-remix.html
C197 | not in Audition | No supported command-line bounce is documented; forum answers say the old command-line options were dropped. | https://community.adobe.com/t5/audition-discussions/a-list-of-command-line-options/m-p/14810348 (weak)
C198 | differs | The Match Loudness panel scans, corrects and exports files to a loudness target (default -23 LUFS); the mixdown dialog has no loudness target. | https://helpx.adobe.com/audition/using/match-loudness.html
C199 | exists | Tempo (BPM) and Time Signature are set per session in the Properties panel, with a Bars and Beats display. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html
C200 | not in Audition | The docs describe one tempo and time signature per session and no tempo map along the timeline (absence in the docs I searched). | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html (weak)
C202 | differs | A metronome exists (Edit > Metronome, pattern and sound editable); I found no count-in setting. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html (weak)
