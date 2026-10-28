# Feature verification, batch 10

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page or a community page, and the verdict rests on my reading of it.

C131 | exists | Window > Properties shows clip settings for one or several selected clips. | https://helpx.adobe.com/audition/using/multitrack-clip-stretching.html

C132 | exists | Lock in Time keeps a clip fixed in time while allowing moves between tracks. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C133 | exists | A colour swatch and name field in Properties set a clip's colour and name. | https://helpx.adobe.com/audition/using/multitrack-clip-stretching.html

C134 | exists | Clips can be grouped to move together and ungrouped. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C135 | exists | Properties has a clip Mute that is independent of the track mute. | https://helpx.adobe.com/audition/using/multitrack-clip-stretching.html

C136 | exists | Tools include Move, Razor (selected clips or all), Slip and Time Selection. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C137 | exists | The Slip tool shifts a clip's contents without moving the clip. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C138 | differs | Ripple Delete closes the gap after a delete; the phrase heal separation is not used. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html (weak)

C139 | not in Audition | I found no key command that nudges clips by a set amount; start time can be typed in Properties. | https://helpx.adobe.com/audition/desktop/keyboard-shortcuts/default-keyboard-shortcuts.html (weak)

C140 | exists | Snap to Clips aligns clip edges and ruler snapping covers grid divisions. | https://helpx.adobe.com/audition/using/selecting-audio.html

C141 | exists | Overlapping clips stack with the top clip playing, and crossfades blend overlaps. | https://helpx.adobe.com/audition/using/clip-volume-matching-fading-mixing.html

C143 | differs | Strip Silence and Split Silence (Deleting Silence) cut at silent regions; I found nothing that splits at transients. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/strip-silence.html

C146 | exists | Clip Effects is a rack separate from the track rack. | https://helpx.adobe.com/audition/using/applying-effects-multitrack-editor.html

C148 | differs | Sessions keep file paths; I found no relink or replace-missing dialog documented. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html (weak)

C149 | exists | Video files can be inserted in a session; the audio portion becomes a clip and the video shows in the Video panel. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/importing-files-panel.html

C150 | exists | Merge Clips joins selected clips, and Convert to Unique Copy makes an independent source file for a clip. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C151 | not in Audition | I found no playlist or variation feature; takes only stack on the track. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C153 | exists | Each track has a Mute button, and clips have their own. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C154 | differs | Solo exists with a Track Solo: Exclusive preference (Ctrl-click does it once); there is no Solo Safe. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

