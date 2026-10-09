# Troubleshooting

| Problem | Try |
|---|---|
| No sound | Check the output device in the status bar; check the track isn't muted, or another soloed; check the master fader. |
| Crackles or dropouts | Pick a bigger **Buffer** in the status bar (the app offers one when it hears the computer struggling). Close other audio apps; remove heavy effects (reverb) from tracks that don't need them. |
| Crackles with lots of tracks or heavy sounds (big reverbs, samplers) | **Freeze** the tracks you're not working on: click **❄** on the track header. The track's sound is rendered once and played back, which costs almost no CPU. |
| Playing the keyboard feels laggy | Pick a smaller **Buffer** in the status bar (256 or 128). If that crackles, go back up one. |
| Recording is silent | Pick the right **Input** in the Audio tab; check Windows privacy settings allow microphone access. |
| Recording hears the song | Wear headphones. |
| Recordings land late | Common with Bluetooth headsets. Click **Calibrate…** in the Audio tab and clap along ([how](audio.md)). |
| The app crashed | Reopen it and answer **Yes** to recover unsaved changes. Older versions of a saved song are in `Song.nptune.bak1`–`bak3` beside it. |
| The window froze ("Not Responding") | Wait about 10 seconds; if it stays frozen, close it (from Task Manager if needed), reopen, and answer **Yes** to recover. Then copy the **Report**: it says the last run didn't close normally and shows what the app was doing when it froze. If a plugin was starting, that plugin is switched off for next time. |
| "Microphone unavailable" | The microphone didn't answer within a few seconds. Check it's connected (Bluetooth headsets can be slow to wake) and not in use by another app, then select the audio track again or pick another **Input** in the Audio tab. |
| "Missing audio" when opening a song | The `<Song name> Audio` folder wasn't moved with the `.nptune` file. Put it back next to the song. |
| Sampler track is silent | Load a sample pack; wait for *loading…* to finish; check the panel for an error. |
| Claude can't find the app | Open Nunc Pro Tune first. In Claude Desktop, quit fully and reopen after setup. |
| Windows warns about the installer | It's unsigned: **More info → Run anyway**. |
| Getting new versions | When a new version is published, the app shows **Update and restart** at the bottom a few seconds after it opens. It saves your song first, installs, and reopens. **Later** hides it until next time. |

When reporting a problem, say what you did, what you heard, and what you expected ("it crackles when I add reverb to the drums"). Claude turns that into a test and a fix.

Then click **Report** in the status bar (or **Copy diagnostic report** in the Claude panel) and paste it into the same message. It lists your sound card, buffer, CPU load, how many crackles the app noticed, your microphone and its delay, and the last errors and events (including any moment the window stopped responding, and what it was busy with). If the last run froze or crashed, the report also shows the end of that run's log. It contains no folder paths. If Claude is connected to the app, it can fetch the same report itself.

The app also keeps a log of each of its last five runs in `%APPDATA%\io.github.nuncprotunc7.nuncprotune\logs` (`log.txt` is the current run, `log-1.txt` the one before), in case you're asked for one.
