# Troubleshooting

| Problem | Try |
|---|---|
| No sound | Check the output device in the status bar; check the track isn't muted, or another soloed; check the master fader. |
| Crackles or dropouts | Close other audio apps; look at the CPU meter in the status bar; remove heavy effects (reverb) from tracks that don't need them. |
| Recording is silent | Pick the right **Input** in the Audio tab; check Windows privacy settings allow microphone access. |
| Recording hears the song | Wear headphones. |
| Recordings land late | Common with Bluetooth headsets. Click **Calibrate…** in the Audio tab and clap along ([how](audio.md)). |
| The app crashed | Reopen it and answer **Yes** to recover unsaved changes. Older versions of a saved song are in `Song.nptune.bak1`–`bak3` beside it. |
| "Missing audio" when opening a song | The `<Song name> Audio` folder wasn't moved with the `.nptune` file. Put it back next to the song. |
| Sampler track is silent | Load a sample pack; wait for *loading…* to finish; check the panel for an error. |
| Claude can't find the app | Open Nunc Pro Tune first. In Claude Desktop, quit fully and reopen after setup. |
| Windows warns about the installer | It's unsigned: **More info → Run anyway**. |

When reporting a problem, say what you did, what you heard, and what you expected ("it crackles when I add reverb to the drums"). Claude turns that into a test and a fix.
