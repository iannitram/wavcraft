# Feature verification, batch 04

Checked 2026-10-28 against Adobe's public Audition help pages. Each line: `row | verdict | what Audition does | source`. A trailing `(weak)` means no Adobe page stated it directly: the source is the closest page I read, and the verdict rests on my reading of it.

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
