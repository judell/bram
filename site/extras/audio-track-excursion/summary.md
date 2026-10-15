At the end of post-production for [this screencast](https://jonudell.info/video/github-agent-coordination.mp4) I needed to clean up one scene with scratchy audio. I asked Claude to clean it up and this is how that went.

## Diagnosis

- **Clipping?** Half-second peak and RMS levels from ffmpeg's `astats` showed none: the loudest peak was −0.8 dB.

- **Crackle?** A high-frequency measure (energy above 4 kHz and 8 kHz, via `highpass` + `astats`) showed the opening line carrying 3–5 dB more than later speech, with bursts at 1.0 s and 4.0 s.

- **Bad source?** Transcribing the scene and its source recording with `whisper-cli` showed the voice wasn't in the source at all. Every line had been narrated later, in Studio. (Whisper's "(scissors snipping)" on the source turned out to be a hallucination over silence.)

- **Generation loss.** Studio's own log (`serve_media.log`) showed that every cut, pause and narration re-encodes the scene's whole audio track. The opening line, recorded first, had been through about 25 lossy encodes. The undo history in `media/.trash` worked as a time series: measured version by version, the opening's 8 kHz energy climbed from 27.2 to 20.1 dB below the voice while the voice level held steady. That confirmed it.

## Repair

For each line, the agent found the earliest saved version that contained it, checked that it lined up with the current scene, and spliced its audio back in at points of digital silence. The video was left untouched, and the files were backed up before every write.

Null tests did the checking. One version is phase-inverted (`volume=-1`) and mixed with the other, so whatever doesn't cancel is the difference. That measured the alignment, located the splice points and verified the result. The agent fell back on ffmpeg for this when `numpy` turned out not to be installed.

Offset searches swept that residual across candidate shifts in 20 ms steps. For line 2, the minimum was at 2.68 s, exactly the length of a cut made in between.

When Whisper's rough timestamps misled the search for lines 3 and 4, the agent wrote and compiled a small Swift normalized cross-correlation tool (`xcorr`, on 2 kHz mono). It then located each line in every saved version.

Verification after each splice: unchanged duration (93.1 s), the high-frequency measure back to its early level, and residual elsewhere at an inaudible −57.6 dB.

When I still heard scratch at 10–20s, the measures were ambiguous: no clicks on a sample-difference scan, and only mild wear. So the agent cut before/after clips and asked me to listen in Finder's Quick Look. The ear settled it: wear, the same cause. The agent also caught its own mistake there. Whisper heard "next session" in one version and "next round" in another, which the agent first took for a re-recording. A null test showed they were the same take, encoded differently. Each summary also said plainly that the agent had not listened to the result itself.

The opening line regained 7 dB, line 2 about 2 dB, and lines 3 and 4 only 1.0 and 0.7 dB. They had been recorded later and so had been through fewer edits. The agent said that any scratch remaining there was probably in the original recording, and that re-narrating was the remedy.

Before proposing a change to Studio, the agent searched the project's history through Bram's `/__search` for prior work on lossless audio, re-encoding and generation loss (it found none). It read the encoding paths in `serve_media.py`, `remix_take.py` and `overlay.py`, and established that burning in callouts and the final export add no loss. It then proposed `edits-lossless-audio-master` on the Worklist: keep a lossless (FLAC) copy of each scene's audio and encode once from it, instead of re-encoding the last lossy file on every edit.

The tools in play: `sqlite3` on Studio's database, `ffprobe`, ffmpeg (`astats`, `highpass`, `pan`, `amix`, `volume`, `asetnsamples`, `ametadata`), `whisper-cli`, `grep` over the server log, a Swift cross-correlator built on the spot, Finder Quick Look for the human A/B, and Bram's search and Worklist.

## Transcript

Here is the session as Bram rendered it in Bram's Transcript view. Be sure to expand the tool calls to see how the agent composed the solution from its kit of primitive tools.