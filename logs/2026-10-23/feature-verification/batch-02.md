# Feature verification, batch 02

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page I read, and the verdict rests on my reading of it. Rows cited as 'checked in Adobe Audition 26.5 (desktop app)' were confirmed in the app's menus on 2026-10-28.

A204 | exists | Play Spectral Selection Only (right-click the Play button) plays just the frequencies selected with the Marquee, Lasso or Paintbrush tools. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/navigating-time-playing-audio.html

A214 | exists | Right-clicking the Levels meters offers Dynamic or Static peaks, Show Valleys, Show Color Gradient, Show LED Meters and decibel range options. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/monitoring-recording-playback-levels.html

A215 | exists | Clip indicators are cleared by clicking them or by right-clicking the meters and choosing Reset indicators. | https://helpx.adobe.com/audition/desktop/importing-recording-and-playing/monitoring-recording-playback-levels.html

A217 | not in Audition | View > Metering offers peaks, valleys, LED, colour gradient and dB ranges; no RMS meter option. | checked in Adobe Audition 26.5 (desktop app)

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
