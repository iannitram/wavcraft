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

B009 | differs | The docs name Fade In, Fade Out, Repair DC Offset and Auto Heal as built-in Favorites; Normalize to 0 dB and Dynamics presets are not named. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html (weak)

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

B111 | differs | The Pitch Shifter sets transposition and a Formant Shift value (zero keeps timbre); named octave and formant presets are not described. | https://helpx.adobe.com/audition/desktop/effects-reference/time-pitch-manipulation-effects.html (weak)

B112 | exists | Stretch and Pitch has a New Duration value for fitting a length, and multitrack clip stretching and Remix fit clips to a duration. | https://helpx.adobe.com/audition/desktop/effects-reference/time-pitch-manipulation-effects.html

B130 | exists | The Loudness Meter shows range with EBU +9 and +18 scales, and Match Loudness describes loudness range (LRA) per EBU. | https://helpx.adobe.com/audition/desktop/effects-reference/special-effects.html

B140 | exists | A spectral selection limits editing and effects to the selected frequencies; Dynamics Processing also has low and high cutoffs. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

B142 | exists | Marquee, Lasso and Paintbrush spectral selections let effects act only on the selected time and frequency range, with brush opacity scaling the intensity. | https://helpx.adobe.com/audition/desktop/editing-audio-files/selecting-audio.html

