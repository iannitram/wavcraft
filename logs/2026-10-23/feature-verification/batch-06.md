# Feature verification, batch 06

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page or a community page, and the verdict rests on my reading of it. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

B144 | exists | The Spot Healing Brush and Auto Heal in the spectral display repair clicks and short artifacts by rebuilding the area from its surroundings (selections of about four seconds or less); Audition names it Spot Healing, not Spectral Repair. | https://helpx.adobe.com/audition/using/selecting-audio.html

B149 | exists | Third-party effects open their own editor window from the effect's slot in the Effects Rack or effect dialog. | https://helpx.adobe.com/audition/using/effects-controls.html (weak)

B150 | not in Audition | Supported third-party formats are VST 2.4, VST 3 (Windows and macOS) and Audio Units (macOS); CLAP is not listed. | https://helpx.adobe.com/audition/desktop/applying-effects/adding-third-party-plug-ins.html

B151 | exists | Audio Units are scanned and hosted on macOS through the Audio Plug-in Manager. | https://helpx.adobe.com/audition/desktop/applying-effects/adding-third-party-plug-ins.html

B152 | exists | 64-bit VST 2.4 effects are scanned and hosted on Windows and macOS (VST 3 too). | https://helpx.adobe.com/audition/desktop/applying-effects/adding-third-party-plug-ins.html

B153 | exists | The Audio Plug-in Manager lists plug-ins, scans or rescans folders, and has per-plug-in Enable checkboxes and a Reload action for ones marked Not Working. | https://helpx.adobe.com/audition/desktop/applying-effects/adding-third-party-plug-ins.html

B154 | differs | Each plug-in is scanned in its own process and one that crashes is marked disabled, which acts as an automatic blacklist; there is no user-edited blacklist. | https://helpx.adobe.com/audition/desktop/applying-effects/adding-third-party-plug-ins.html

B155 | exists | Effects have saved presets, the Effects Rack stores rack presets, and the Favorites menu holds saved effect settings; Adobe's presets page documents the rack and effect presets. | https://helpx.adobe.com/audition/using/effects-controls.html

B156 | differs | Adobe's fixed-issues notes mention latency compensation (an echo bug when sending to busses pre-fader); I found no page describing plug-in tail handling. | https://helpx.adobe.com/audition/desktop/introduction/fixed-issues.html (weak)

B157 | exists | Track effect parameters can be automated with envelopes and the Read, Write, Latch and Touch modes. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html

B160 | exists | Under another name: a Favorite (which can hold a recorded effect sequence) is chosen in the Batch Process panel and run over a list of files. | https://community.adobe.com/questions-544/is-there-a-way-to-add-more-then-1-favorite-in-the-batch-processing-164399 (weak)

B161 | exists | The Match Loudness panel scans a list of files and runs them to a chosen loudness standard, true-peak limit and tolerance. | https://helpx.adobe.com/audition/using/match-loudness.html

B162 | exists | Noise Reduction is a process effect, so it can be applied to files through a recorded Favorite in Batch Process using a saved noise print; I found only community confirmation. | https://community.adobe.com/questions-544/is-there-a-way-to-add-more-then-1-favorite-in-the-batch-processing-164399 (weak)

B163 | exists | Favorites > Start Recording Favorite / Stop Recording Favorite record step sequences. | checked in Adobe Audition 26.5 (desktop app)

B168 | exists | Process effect dialogs such as Noise Reduction have a Preview button that plays the processed selection before it is applied. | https://helpx.adobe.com/audition/using/effects-controls.html

B169 | differs | Effects can be bypassed or toggled with Preview to compare before and after; I found no stored A and B settings slot. | https://helpx.adobe.com/audition/using/effects-controls.html (weak)

C011 | exists | With ASIO or Core Audio and a small buffer, input can be monitored through track effects live; Adobe says under 10 ms is usable. | https://helpx.adobe.com/audition/using/connecting-audio-hardware.html

C015 | differs | Audio Hardware preferences offer Clock and Sync Reference settings to choose which device other hardware follows; there is no word-clock generator or external clock lock of its own. | https://helpx.adobe.com/audition/using/connecting-audio-hardware.html

C036 | exists | Each track has an Arm for Record button (R) that readies it for input and shows its meters. | https://helpx.adobe.com/audition/using/recording-audio.html

C040 | exists | Punch and Roll plays a pre-roll (default 5 seconds, set in Playback and Recording preferences) before recording starts at the punch point. | https://helpx.adobe.com/audition/using/recording-audio.html

