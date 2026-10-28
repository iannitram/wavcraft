# Feature verification, batch 05

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page I read, and the verdict rests on my reading of it.

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
