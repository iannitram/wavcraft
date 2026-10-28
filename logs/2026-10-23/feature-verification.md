# Feature verification: roadmap rows against Audition docs

One verdict line per row, merged from the batch logs in `feature-verification/`.

A015 | exists | The toolbar has buttons that switch between the Waveform Editor and Multitrack Editor, and the default shortcut is 1. | https://helpx.adobe.com/audition/desktop/keyboard-shortcuts/default-keyboard-shortcuts.html

A024 | exists | Audition has a Media Browser panel for finding files on drives and locations; Adobe's fixed-issues list mentions a fix for OneDrive not showing in it. | https://helpx.adobe.com/audition/desktop/introduction/fixed-issues.html (weak)

A040 | differs | The current docs have no Phase Analysis panel; the Phase Meter panel (Window > Phase Meter) compares the phase of two chosen channels and shows out-of-phase or mono-compatible audio. | https://helpx.adobe.com/audition/desktop/editing-audio-files/analyzing-phase-frequency-amplitude.html

A041 | differs | Audition's radar loudness view is an effect plug-in (Effects > Special > Loudness Radar Meter, and the Loudness Meter) showing momentary, history, true peak and range, not a dockable panel of its own. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

A044 | exists | Properties (clip and session settings, including tempo), Mixer and Playlist (Window > Playlist) panels all exist; Mixer and session properties belong to the Multitrack Editor, Playlist to the Waveform Editor. | https://helpx.adobe.com/audition/desktop/mixing-multitrack-sessions/multitrack-editor-overview.html

A053 | exists | The Lasso Selection tool makes free-form selections in the spectral display, and Shift-drag adds to one. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A054 | exists | The Paintbrush Selection tool makes free-form selections with a Size setting; its Opacity setting (or layered strokes) sets how strongly effects apply. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A055 | exists | The Spot Healing Brush repairs clicks and pops in the spectral display by running Favorites > Auto Heal on what you click or drag across (selections of four seconds or less). | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A056 | exists | Shift-drag adds to a lasso or paintbrush selection and Alt-drag subtracts from it. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

A067 | differs | The Spectral Pitch Display exists only as the pitch view inside the Manual Pitch Correction effect (fundamental in blue, overtones yellow to red); it is not a general alternate spectral view. | https://helpx.adobe.com/audition/desktop/applying-effects/manual-pitch-correction-effect.html

A068 | not in Audition | The current docs describe only the Spectral Frequency Display and the pitch view inside Manual Pitch Correction; no page offers a spectral phase display. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop/editing-audio-files/displaying-audio-waveform-editor.html (weak)

A077 | exists | Dragging the Fade In or Fade Out handle makes a linear fade (drag horizontally), logarithmic (drag up or down) or cosine (hold Ctrl or Command); in the Waveform Editor it changes the audio data directly. | https://helpx.adobe.com/audition/desktop/editing-audio-files/visually-fading-changing-amplitude.html

A078 | exists | The General preferences hold a Default Fade setting (cosine by default with modifier keys giving linear or log) and Show HUD for selection ranges only; View > Show HUD hides the gain HUD. | https://helpx.adobe.com/audition/desktop/editing-audio-files/visually-fading-changing-amplitude.html

A100 | exists | Samples is one of the time display formats, counting the actual samples elapsed since the start of the file; the amplitude ruler can also show sample values. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/navigating-time-playing-audio.html

A166 | exists | Edit > Frequency Band Splitter makes up to eight copies of the selection, each limited to a frequency band set by crossover values, each on its own track. | https://helpx.adobe.com/audition/desktop/editing-audio-files/frequency-band-splitter.html

A168 | exists | With the Bars and Beats time format, Edit Tempo opens tempo and time signature settings in the Properties panel. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/navigating-time-playing-audio.html

A187 | not in Audition | Audition 26.3 and later removed CD Layout, the CD marker type and the CD time display; old CD markers become Cue markers when a project opens. | https://helpx.adobe.com/audition/desktop/introduction/removal-of-cd-authoring.html

A189 | exists | The Playlist panel plays all or part of a list of marker ranges, in any order, with a loop count per item. | https://helpx.adobe.com/audition/desktop/editing-audio-files/markers.html

A190 | exists | Playlist items are reordered by dragging up or down and can be removed from the list. | https://helpx.adobe.com/audition/desktop/editing-audio-files/markers.html

A191 | exists | A playlist is stored in the file only when the audio is saved as a WAV file. | https://helpx.adobe.com/audition/desktop/editing-audio-files/markers.html

A204 | exists | Play Spectral Selection Only (right-click the Play button) plays just the frequencies selected with the Marquee, Lasso or Paintbrush tools. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/navigating-time-playing-audio.html

A214 | exists | Right-clicking the Levels meters offers Dynamic or Static peaks, Show Valleys, Show Color Gradient, Show LED Meters and decibel range options. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/monitoring-recording-playback-levels.html

A215 | exists | Clip indicators are cleared by clicking them or by right-clicking the meters and choosing Reset indicators. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/monitoring-recording-playback-levels.html

A217 | not in Audition | The Levels panel documents peak, valley and clip indicators only; no RMS meter display is described. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/monitoring-recording-playback-levels.html (weak)

A218 | exists | The Levels panel can be docked horizontally (left channel on top) or vertically. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/monitoring-recording-playback-levels.html

A228 | exists | The Supported import formats page lists AAC/HE-AAC, AIFF/AIFC, AC-3, APE, AU, AVR, BWF, CAF, EC-3, FLAC, HTK, IFF, M4A, MAT, MPC, MP2, MP3, OGA/OGG, PAF, PCM, PVF, RAW, RF64, SD2, SDS, SF, SND, VOC, VOX, W64, WAV, WMA (Windows only), WVE and XI. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/supported-file-formats.html

A229 | not in Audition | AAC and M4A are listed as import formats, but no help page mentions gapless trimming or edit-list handling. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/supported-file-formats.html (weak)

A230 | differs | Audition shows channel names from iXML metadata on multichannel files and has BWF and RIFF tabs in the Metadata panel, but no field-recorder iXML editor is documented. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/multichannel-audio-workflow.html

A231 | exists | File > Open takes the audio of video files (AVI, DV, MOV, MPEG-1, MPEG-4, 3GPP/3GPP2, plus FLV, R3D, SWF and WMV with DLMS support) into the Waveform Editor. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/supported-file-formats.html

A232 | differs | Audition exchanges sequences with Premiere through a shared XML format (Multitrack > Export to Adobe Premiere, Edit > Edit in Adobe Audition > Sequence from Premiere, and Premiere Sequence XML in File > Open), not by reading .prproj files. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/video-applications.html

A233 | exists | File > Open loads Audition Session, Audition 3.0 XML, Premiere Sequence XML, Final Cut Pro XML Interchange and OMF into the Multitrack Editor; SES files from 3.0 and earlier are unsupported. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/creating-opening-files.html

A234 | not in Audition | From version 23.0 Audition no longer opens .asnd files directly; Adobe documents a manual workaround of renaming to .zip and importing the Mixdown.wav inside. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/creating-opening-files.html

A249 | exists | MP2 is a save format with channel mode (automatic, mono, dual mono, stereo, joint stereo) and bitrate settings. | https://helpx.adobe.com/audition/desktop/saving-and-exporting/saving-exporting-files1.html

A251 | exists | Monkey's Audio (.ape) is a lossless save format with a compression setting. | https://helpx.adobe.com/audition/desktop/saving-and-exporting/saving-exporting-files1.html

A258 | exists | File > Save All saves every open file in its current format; in the Multitrack Editor it saves the session and the audio files it contains. | https://helpx.adobe.com/audition/desktop/saving-and-exporting/saving-exporting-files1.html

A264 | exists | File > Export > Export with Adobe Media Encoder sends a multitrack session to Media Encoder, by default only the mix track, with a range choice. | https://helpx.adobe.com/audition/desktop/saving-and-exporting/saving-exporting-files1.html

A266 | exists | The Metadata panel has an ID3 tab for MP3 files, including an Album Art option. | https://helpx.adobe.com/audition/desktop/saving-and-exporting/viewing-editing-xmp-metadata.html

A279 | exists | Preferences > Control Surface adds PreSonus FaderPort V1 (not FaderPort 8) or HUI-compatible devices with button assignments. | https://helpx.adobe.com/audition/desktop/workspace-and-setup/control-surface-support.html

A283 | exists | The Multitrack Editor mixes clips on tracks in .sesx session files that point to the source audio. | https://helpx.adobe.com/audition/desktop/mixing-multitrack-sessions/multitrack-editor-overview.html

A287 | exists | Multitrack sessions hold one video clip with a Video panel preview, a spot-frame preview while moving clips (a Video preference) and a timecode overlay. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/importing-video-video-clips.html

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

B008 | exists | A Favorites menu exists (Fade In, Repair DC Offset, Auto Heal and others) and the noise-reduction page speaks of creating a quick key there; the fetched pages do not describe recording your own actions. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html (weak)

B009 | differs | Normalize presets are -0.1 dB and -3 dB; there is no 0 dB preset and no Dynamics presets (fade, DC offset and Auto Heal Favorites are documented). | checked in Adobe Audition 26.5 (desktop app)

B014 | exists | Each effect slot has a Power button and the rack has a main Power button, plus Toggle Power State of Selected Effects. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B015 | exists | Effects are reordered by dragging between slots, replaced by choosing another effect in the slot menu, and removed with Remove Effect or Delete. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B016 | exists | The Effects Rack has Input and Output level meters and sliders to set levels. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B017 | differs | The rack has a single Mix slider (100% wet to 0% dry); a per-effect wet control is not described for the rack itself. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B019 | differs | The Waveform Editor rack has a Process menu (selection or entire file) and an Apply button; no help page describes extending the file for effect tails. | https://helpx.adobe.com/audition/desktop/applying-effects/applying-effects-waveform-editor.html (weak)

B020 | exists | In the Multitrack Editor the rack has FX Pre/Post-Fader buttons and a Pre-render Track button. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B021 | exists | The rack's Presets menu stores whole groups of effects and settings, with Save and Delete preset buttons. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B022 | exists | The main Power button bypasses the whole rack so processed and unprocessed audio can be compared; there is no separate A/B setting switch. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B023 | not in Audition | No help page describes saving a rack as a Favorite. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html (weak)

B024 | exists | Choosing an effect in a slot lets you adjust its settings in the effect's own controls, and effect settings can be edited later. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html (weak)

B026 | not in Audition | The Amplify page lists gain sliders and link only; no clip-guard or calculate-now option is described. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html (weak)

B030 | exists | The Dynamics Processing graph plots input level against output level for the compressor, limiter and expander, with level and gain-reduction meters. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html

B035 | exists | The Multiband Compressor works on four bands with draggable crossovers, solo and bypass per band, output gain and a limiter. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html

B039 | exists | The Tube-modeled Compressor adds warm, analog-style compression and harmonic coloring. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html

B040 | not in Audition | No sidechain or key-input option appears for any dynamics effect in the docs. No Adobe page states its absence. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html (weak)

B041 | differs | Audition has the Match Loudness panel (scan, choose a standard, run) and Clip > Match Clip Volume in multitrack; there is no gain match from a selection to a reference. | https://helpx.adobe.com/audition/desktop/editing-audio-files/match-loudness.html

B044 | not in Audition | The delay and echo effects are Analog Delay, Delay and Echo; there is no Dynamic Delay or Echo Chamber. | https://helpx.adobe.com/audition/desktop/effects-reference/delay-echo-effects.html

B058 | exists | The Parametric Equalizer has an interactive graph with draggable control points. | https://helpx.adobe.com/audition/desktop/effects-reference/filter-equalizer-effects.html

B061 | exists | Effect graphs can show a real-time Frequency Spectrum behind the curve to spot imbalances. | https://helpx.adobe.com/audition/desktop/applying-effects/effects-controls.html

B068 | exists | The Noise Reduction graph has control points that vary reduction by frequency range and shows low and high amplitude noise against the threshold. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html

B069 | exists | DeNoise is a real-time effect with an amount slider from 0% to 100% for steady background noise. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html

B070 | exists | Adaptive Noise Reduction is a real-time effect for variable broadband noise such as rumble and wind. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html

B071 | exists | Hiss Reduction (Waveform Editor only) lowers frequency ranges that fall below a noise-floor threshold. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html

B075 | exists | Sound Remover builds a sound model from a selected portion and removes that source from the recording. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html

B077 | exists | Favorites > Auto Heal, run by the Spot Healing Brush, repairs short artifacts; Automatic Click Remover and the DeClicker handle clicks. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

B080 | exists | Noise Reduction has an Output Noise Only option that previews only the removed noise. | https://helpx.adobe.com/audition/desktop/effects-reference/noise-reduction-restoration-effects.html

B082 | exists | Reverb > Full Reverb makes customizable reverberation with decay time, pre-delay and coloration; it is processing-heavy. | https://helpx.adobe.com/audition/desktop/effects-reference/reverb-effects.html

B083 | exists | Studio Reverb is a Reverb effect (decay time, coloration, pre-delay) listed among the effects shared with Premiere. | https://helpx.adobe.com/audition/desktop/effects-reference/reverb-effects.html

B085 | exists | Reverb > Surround Reverb is meant for 5.1 sources but also adds ambience to mono and stereo. | https://helpx.adobe.com/audition/desktop/effects-reference/reverb-effects.html

B086 | not in Audition | The reverb effects are Convolution Reverb, Full Reverb, Studio Reverb and Surround Reverb; no plate reverb effect is listed (an impulse file could imitate one). | https://helpx.adobe.com/audition/desktop/effects-reference/reverb-effects.html

B089 | exists | Effects > Special > Doppler Shifter imitates a sound source moving past the listener. | https://helpx.adobe.com/audition/desktop/applying-effects/doppler-shifter-effect---waveform.html

B090 | exists | Special > Guitar Suite chains compressor, filter, distortion and box modeler stages for guitar tracks. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

B092 | exists | Special > Mastering is a mastering effect that can also sit in the Effects Rack. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

B093 | exists | Special > Vocal Enhancer has voice modes that reduce sibilance, plosives and handling rumble, with mic modeling and compression. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

B096 | differs | There is no Dynamic EQ; Dynamics Processing has Band Limiting (low and high cutoff) and the Multiband Compressor works per band. | https://helpx.adobe.com/audition/desktop/effects-reference/amplitude-compression-effects.html

B097 | exists | Stereo Imagery > Center Channel Extractor keeps or removes center-panned sound such as vocals or bass. | https://helpx.adobe.com/audition/desktop/effects-reference/stereo-imagery-effects.html

B098 | exists | The Stereo Expander positions and widens the stereo image and works in the Effects Rack. | https://helpx.adobe.com/audition/desktop/effects-reference/stereo-imagery-effects.html

B102 | exists | Time and Pitch > Automatic Pitch Correction retunes vocals or instruments to a chosen key, in both editors. | https://helpx.adobe.com/audition/desktop/effects-reference/time-pitch-manipulation-effects.html

B104 | exists | Manual Pitch Correction (Waveform Editor only) adjusts pitch visually in the Spectral Pitch Display. | https://helpx.adobe.com/audition/desktop/applying-effects/manual-pitch-correction-effect.html

B106 | exists | Pitch Bender varies pitch over time with a keyframe envelope across the waveform. | https://helpx.adobe.com/audition/desktop/effects-reference/time-pitch-manipulation-effects.html

B109 | exists | Stretch and Pitch offers the Audition algorithm or iZotope Radius (slower, fewer artifacts) with a precision setting. | https://helpx.adobe.com/audition/desktop/effects-reference/time-pitch-manipulation-effects.html

B111 | differs | The Pitch Shifter has no formant control; it sets transposition only, and no named octave or formant presets exist. | checked in Adobe Audition 26.5 (desktop app)

B112 | exists | Stretch and Pitch has a New Duration value for fitting a length, and multitrack clip stretching and Remix fit clips to a duration. | https://helpx.adobe.com/audition/desktop/effects-reference/time-pitch-manipulation-effects.html

B130 | exists | The Loudness Meter shows range with EBU +9 and +18 scales, and Match Loudness describes loudness range (LRA) per EBU. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

B140 | exists | A spectral selection limits editing and effects to the selected frequencies; Dynamics Processing also has low and high cutoffs. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

B142 | exists | Marquee, Lasso and Paintbrush spectral selections let effects act only on the selected time and frequency range, with brush opacity scaling the intensity. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

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

B163 | exists | Under another name: Favorites > Start Recording Favorite records a step sequence for replay (the older Scripts feature did the same); only community pages describe it. | https://community.adobe.com/t5/audition-discussions/unable-to-make-recorded-favorite-act-relative-to-playhead/m-p/11363451 (weak)

B168 | exists | Process effect dialogs such as Noise Reduction have a Preview button that plays the processed selection before it is applied. | https://helpx.adobe.com/audition/using/effects-controls.html

B169 | differs | Effects can be bypassed or toggled with Preview to compare before and after; I found no stored A and B settings slot. | https://helpx.adobe.com/audition/using/effects-controls.html (weak)

C011 | exists | With ASIO or Core Audio and a small buffer, input can be monitored through track effects live; Adobe says under 10 ms is usable. | https://helpx.adobe.com/audition/using/connecting-audio-hardware.html

C015 | differs | Audio Hardware preferences offer Clock and Sync Reference settings to choose which device other hardware follows; there is no word-clock generator or external clock lock of its own. | https://helpx.adobe.com/audition/using/connecting-audio-hardware.html

C036 | exists | Each track has an Arm for Record button (R) that readies it for input and shows its meters. | https://helpx.adobe.com/audition/using/recording-audio.html

C040 | exists | Punch and Roll plays a pre-roll (default 5 seconds, set in Playback and Recording preferences) before recording starts at the punch point. | https://helpx.adobe.com/audition/using/recording-audio.html

C041 | not in Audition | I found no repeat-punch command; a punch is repeated by recording over the range again. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C042 | differs | Repeated recording over the same range stacks takes on the track; there is no cycle (loop) recording mode that records successive passes automatically. | https://helpx.adobe.com/audition/using/recording-audio.html

C044 | differs | Takes are layered and the preferred one is dragged into place and trimmed or split by hand; there is no comp lane that assembles takes. | https://helpx.adobe.com/audition/using/recording-audio.html

C045 | exists | Input monitoring is toggled per track with the I button, and smart monitoring hears input while stopped and track content during playback. | https://helpx.adobe.com/audition/using/recording-audio.html

C046 | exists | The recorded clip's waveform grows in the editor while recording. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C047 | exists | R arms a track and the transport has default keyboard shortcuts including Record. | https://helpx.adobe.com/audition/desktop/keyboard-shortcuts/default-keyboard-shortcuts.html

C048 | exists | Arming several tracks records on all of them at once, given enough hardware inputs. | https://helpx.adobe.com/audition/using/recording-audio.html

C051 | exists | Recording writes to disk-backed temporary files (the media cache folder), not only RAM; I found no sentence on this specifically. | https://helpx.adobe.com/audition/using/recording-audio.html (weak)

C052 | differs | Device and driver errors surface as dialogs and Adobe has troubleshooting articles; there is no dedicated recording diagnostics report. | https://helpx.adobe.com/audition/kb/troubleshoot-recording-playback-monitoring-audition-1.html (weak)

C060 | exists | Track meters show in the Mixer and Editor for each track, and right-click sets range, peaks and display style. | https://helpx.adobe.com/audition/using/monitoring-recording-playback-levels.html

C061 | exists | The Level Meters panel shows the mix output, and the Mix track has its own meter in the Mixer. | https://helpx.adobe.com/audition/using/monitoring-recording-playback-levels.html

C069 | exists | Under another name: the Frequency Analysis and Spectral Frequency displays update during playback and recording; I found no page that ties them to recording. | https://helpx.adobe.com/audition/using/analyzing-phase-frequency-amplitude.html (weak)

C076 | differs | Session files store file paths only; Export can save copies of associated files (archive style), but I found no documented relink dialog. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html (weak)

C077 | exists | File > Export > Session As Template saves tracks and clips; a template is chosen when creating a new session. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C078 | exists | Sample rate and bit depth are set when a session is created (bit depth cannot be changed later) and all files in a session share the sample rate. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C079 | exists | The Files panel imports files, which can be inserted into the session on separate tracks at the current time. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/importing-files-panel.html

C080 | exists | File > Open Recent lists recent files and sessions; I found no Adobe page that documents it. | https://helpx.adobe.com/audition/using/creating-opening-files.html (weak)

C081 | exists | Edit > Undo and the History panel cover multitrack edits, and Adobe says multitrack clip edits are non-destructive and undoable. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C082 | exists | Zoom buttons, scroll bar and mouse wheel zoom and scroll the timeline. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C083 | exists | View > Time Display switches the ruler between formats including Bars and Beats; the full list was not on pages I could read. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html (weak)

C085 | exists | The Markers panel adds, edits and selects point and range markers (also called cues). | https://helpx.adobe.com/audition/using/markers.html

C086 | exists | A time selection and the current-time indicator work in the multitrack editor, including selection of ranges across tracks. | https://helpx.adobe.com/audition/using/selecting-audio.html

C087 | differs | Audition has one current-time indicator that is both cursor and playhead; I found no separate edit cursor. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html (weak)

C088 | exists | Snapping can be toggled in the editor and pulls selection edges and the current-time indicator to targets. | https://helpx.adobe.com/audition/using/selecting-audio.html

C089 | exists | Snap targets are markers, ruler ticks (coarse or fine), zero crossings, clips, loops and frames. | https://helpx.adobe.com/audition/using/selecting-audio.html

C090 | exists | Dragging the current-time indicator or using J, K, L previews audio as you scrub. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C091 | exists | Transport controls play, pause, stop and loop playback in the multitrack editor. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C092 | exists | Loop Playback repeats a selected range. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C093 | exists | Meters show playback levels as well as recording levels. | https://helpx.adobe.com/audition/using/monitoring-recording-playback-levels.html

C094 | differs | Audition has a Metadata panel for file-level fields, not free-text session notes; I found no Adobe page on it. | https://helpx.adobe.com/audition/using/creating-opening-files.html (weak)

C099 | exists | Bus tracks group several tracks or sends under one control. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C102 | exists | I believe multitrack sessions can hold MIDI tracks with virtual instruments; the pages I could read did not describe them. | https://helpx.adobe.com/audition/using/multitrack-editor-overview.html (weak)

C103 | exists | Tracks are added singly or several at a time, deleted (including all empty tracks) and duplicated with clips, effects, EQ and automation. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C104 | exists | Tracks can be reordered by dragging; I did not find this stated on a page. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html (weak)

C105 | differs | Tracks are renamed in the Editor or Mixer and the track colour is set from a swatch; I found no per-track notes field. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html (weak)

C106 | differs | Track height is changed by dragging the divider or with zoom tools; I found no named height presets. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C107 | differs | The Track Visibility panel hides tracks by eye icon and saves visibility presets; I found no track folders or groups. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C108 | exists | Bounce to New Track creates a track holding a clip that reflects a bus or mix track (also available for clips). | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C109 | not in Audition | I found no Freeze Track command; bouncing to a new track is the nearest feature. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html (weak)

C110 | exists | A Polarity Reverse (phase invert) button in the Mixer input controls flips a track's phase. | https://helpx.adobe.com/audition/using/analyzing-phase-frequency-amplitude.html

C111 | exists | Under another name: multichannel files are split to mono with Extract Channels to Mono Files; sessions use mono or stereo track types. | https://helpx.adobe.com/audition/using/multichannel-audio-workflow.html (weak)

C112 | exists | Each track's output is set in the Editor or Mixer to hardware, a bus or the Mix track. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C113 | exists | Each track's input is chosen from hardware inputs in the Editor or Mixer. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C114 | exists | Tracks have an Effects Rack of up to 16 effects, and Clip Effects are a separate rack. | https://helpx.adobe.com/audition/using/applying-effects-multitrack-editor.html

C115 | exists | Clips are conformed to the track's channel layout (mono, stereo, 5.1) on insertion; I found no page describing every conversion. | https://helpx.adobe.com/audition/using/multichannel-audio-workflow.html (weak)

C116 | exists | Imported files are inserted as clips on tracks from the Files panel. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/importing-files-panel.html

C117 | exists | The Move tool moves clips in time and between tracks. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C118 | exists | Dragging a clip edge trims or extends it; Clip > Trim also trims to selection or playhead. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C119 | exists | The Razor tool splits at the click and Clip > Split splits all clips at the playhead. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html

C120 | exists | Clips can be deleted, and Ripple Delete closes the gap; the ripple option was not on the pages I could read. | https://helpx.adobe.com/audition/using/arranging-editing-multitrack-clips.html (weak)

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

C151 | exists | A Playlist panel exists, so alternate takes can be kept as playlists. | checked in Adobe Audition 26.5 (desktop app)

C153 | exists | Each track has a Mute button, and clips have their own. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C154 | differs | Solo exists with a Track Solo: Exclusive preference (Ctrl-click does it once); there is no Solo Safe. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C155 | exists | Each track has a volume fader in the Editor and the Mixer (drag it, or Alt/Option-click to jump). | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C156 | exists | Each track has a Pan knob, equal-power by default with a logarithmic option in Multitrack preferences. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C157 | exists | Each track, clip and bus takes up to 16 effects in its Effects Rack, placed before or after the fader (pre-fader is the default). | https://helpx.adobe.com/audition/using/applying-effects-multitrack-editor.html

C158 | exists | Each track has up to 16 sends, each pre- or post-fader, with its own level and pan, aimed at a bus. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C159 | differs | The help describes sends and buses but no sidechain feature; users route sends to a bus feeding a plug-in's sidechain input, and forum posts report this is flaky in 25.0. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html (weak)

C160 | not in Audition | No track groups or VCA faders are documented; buses are the way to control several tracks with one fader (Ctrl/Cmd+Shift applies one setting to several tracks). | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html (weak)

C161 | exists | The Effects Rack panel shows Clip Effects or Track Effects for the selected clip or track, saved with the session. | https://helpx.adobe.com/audition/using/applying-effects-multitrack-editor.html

C163 | exists | Audition ships EQ, dynamics, reverb, delay and other effects that can be placed in a track's rack. | https://helpx.adobe.com/audition/desktop/effects-reference/filter-equalizer-effects.html

C164 | exists | Each track has a parametric EQ section in the Mixer/Editor; double-clicking its graph opens the Track EQ window. | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html

C165 | exists | The Inputs/Outputs area picks a hardware input, and an output of a bus, the Mix track or a hardware port. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C166 | exists | The Mixer's Show/Hide triangles, and Editor buttons for Inputs/Outputs, Effects, Sends and EQ, show or hide each control set. | https://helpx.adobe.com/audition/using/multitrack-editor-overview.html

C169 | not found | I could not find Adobe documentation of delay compensation across tracks; only old forum posts that mention VST delay being compensated. | https://community.adobe.com/t5/audition-discussions/how-to-make-latency-compensation-work-or-suggestions-for-a-small-interface-that-does-work/m-p/3784808/highlight/true (weak)

C170 | not in Audition | No control-room or separate monitor section is documented; outputs are a bus, the Mix track or hardware ports (absence in the docs I searched, not a statement from Adobe). | https://helpx.adobe.com/audition/using/multitrack-routing-eq-controls.html (weak)

C171 | exists | The session's Master/Mix track is mono, stereo or 5.1, and track and bus layouts are chosen to match it. | https://helpx.adobe.com/audition/using/basic-multitrack-controls.html

C172 | differs | The Effects Rack saves and applies rack presets (groups of effects) across tracks; there is no whole-channel-strip preset, and Track Visibility presets only store the display layout. | https://helpx.adobe.com/audition/using/effects-controls.html

C175 | exists | Clips have clip envelopes and tracks have track envelopes (volume, pan, effect parameters), drawn as lines with keyframes; they are nondestructive. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html

C176 | exists | Track automation modes are Off, Read (default), Write, Latch and Touch, with Touch easing back over a set time. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html

C177 | exists | Effect parameters are automated by showing an envelope for the parameter, or by recording while a mode is armed. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html

C179 | exists | Keyframe transitions are Linear (default) or Hold per keyframe, and a spline curve can be applied to a whole envelope. | https://helpx.adobe.com/audition/using/automating-mixes-envelopes1.html

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

C203 | exists | Bars and Beats is a time display format, and snapping works against it. | https://helpx.adobe.com/audition/using/navigating-time-playing-audio.html

C206 | not found | I could not confirm that clips follow the session tempo; the docs cover manual clip stretching. | https://helpx.adobe.com/audition/desktop/mixing-multitrack-sessions/multitrack-clip-stretching.html

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

C281 | exists | In Premiere, Edit > Edit in Adobe Audition sends a clip or a sequence, and the saved result returns to the timeline. | https://helpx.adobe.com/premiere/desktop/use-premiere-with-other-apps/working-with-other-adobe-applications/edit-premiere-sequences-in-audition.html

C282 | exists | Sending a sequence can use Dynamic Link so Audition streams the video without rendering. | https://helpx.adobe.com/premiere/desktop/use-premiere-with-other-apps/working-with-other-adobe-applications/edit-premiere-sequences-in-audition.html

C283 | not found | I found no mention of OpenTimelineIO in Audition's documentation. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C284 | exists | A video clip can be inserted into a session and previews in the Video panel, one video clip at a time. | https://helpx.adobe.com/audition/using/importing-video-video-clips.html

C286 | exists | The time display can be set to the video's SMPTE rate, with Snap To Frames and a Video Frame Rate status readout. | https://helpx.adobe.com/audition/using/importing-video-video-clips.html

C288 | differs | No replace-audio command; the documented way is a mixdown re-imported into the video application, or overwriting the original file by name. | https://helpx.adobe.com/audition/using/video-applications.html

C290 | not in Audition | Forum reports show Pro Tools sessions are not opened directly; none of the docs list it (not an Adobe statement). | https://community.adobe.com/t5/audition-discussions/workflow-protools-to-audition/m-p/15025362/highlight/true (weak)

C291 | not in Audition | Audition has no MIDI implementation, so no MIDI file import or export (forum answers; no Adobe statement). | https://community.adobe.com/t5/audition-discussions/does-audition-cc-have-midi-support/m-p/11345101 (weak)

C292 | differs | SMPTE time formats and control-surface sync (Mackie HUI, FaderPort) exist; chasing external timecode is not documented. | https://helpx.adobe.com/audition/using/control-surface-support.html

C294 | differs | The Track Panner pans mono and stereo tracks; for 5.1 you send a track to a 5.1 bus and pan the send. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html

C297 | not found | I found no description of surround-specific meters in the pages I could read. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html

C299 | not found | I found no mention of Dolby Atmos or immersive formats in Audition's documentation. | https://helpx.adobe.com/audition/desktop/video-and-surround-sound/5-1-surround-sound.html

C301 | exists | Audition 26.0 runs natively on Windows on ARM; native Apple silicon came in May 2021 (press report). | https://helpx.adobe.com/audition/using/whats-new/2026.html (weak)

C310 | not found | I found no share or upload command in Audition's documentation. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C311 | not found | I found no cloud sync or library feature for Audition sessions. | https://helpx.adobe.com/audition/using/saving-exporting-files1.html

C312 | exists | Scripts run from File > Browse and Run Script...; scripting exists in Audition 26.5. | checked in Adobe Audition 26.5 (desktop app)

C314 | differs | Audition has Generate Speech using the operating system's voices; AI Enhance Speech is documented in Adobe's Premiere Pro help, not Audition's, and no auto-tagging was found. | https://helpx.adobe.com/audition/desktop/using/text-to-speech.html

C323 | not in Audition | Not supported; a community request asks for DAWproject. | https://community.adobe.com/t5/audition-ideas/please-add-support-for-the-new-dawproject-session-exchange-format-it-s-free-and-open-source/idi-p/14127897 (weak)

C324 | not found | I found no public Adobe documentation of a crash-safe export finalize step in Audition. | not found in Adobe's public Audition docs (searched 2026-10-28); app check pending
