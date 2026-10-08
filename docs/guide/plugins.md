# Plugins (VST3)

Plugins are instruments and effects made by other companies that work in many music apps. Lots of great ones are free. Nunc Pro Tune plays **VST3 instruments** on tracks; plugin effects come later.

## Getting plugins

1. Download and install a plugin with its own installer. Free favourites for game music:
   - **Spitfire LABS**: orchestral, piano and atmospheric sounds (install the Spitfire app, then the LABS sounds you want).
   - **Surge XT**: a big free synthesizer.
   - **Vital**: a modern free synthesizer.
2. Installers put VST3 plugins in `C:\Program Files\Common Files\VST3`, where Nunc Pro Tune looks. Some ask where to install: choose that folder.
3. Nunc Pro Tune looks for new plugins each time it starts. Already running? Choose **Look for new plugins…** in the **Plugin** menu (below).

The first look can take a minute if you have many plugins: each one is opened in a separate helper so a broken plugin can't crash the app. Plugins that can't be used are listed in the **diagnostic report** (Report, in the status bar).

## Playing a track with a plugin

1. Click a track (any instrument track, not an audio track). Its instrument panel opens at the bottom.
2. In the **Plugin** menu, choose the plugin. The track now plays it (**Ctrl+Z** puts the old instrument back).
3. Big plugins take a few seconds to load; the panel says *Loading the plugin…* meanwhile.
4. **Open plugin window** shows the plugin's own controls, exactly as its maker designed them. Pick sounds there (e.g. in LABS, the instrument and its articulation). The window stays in front of Nunc Pro Tune; close it any time, the sound stays.
5. Below, **Controls** lists the plugin's settings as simple sliders with the plugin's own wording ("-6.0 dB", "Saw"). Use the search box when a plugin has hundreds.

Every change, in the plugin window or on the sliders, is undoable, and a whole knob turn is one undo step. Picking one of the plugin's own presets is one undo step too.

## Saving and sharing

- The song saves each plugin's settings, so it sounds the same when you open it again.
- To open the song on another computer, that computer needs the same plugins installed. If one is missing, its track is silent and the panel says which plugin it needs.
- Exports (WAV, Godot) and versions/A-B use exactly what you hear.

## Claude and plugins

Claude can list your plugins, put one on a track, and change its controls: "put Surge XT on the bass track and make it darker". It reads the same control names and values you see. Claude can't see the plugin's window, so for picking sounds by ear, open the window yourself (or ask Claude which control to try).

## If something goes wrong

- **Silent plugin track**: check the panel. *Loading* means wait; an error usually means the plugin isn't installed any more. Reinstall it, then **Look for new plugins…**.
- **A plugin isn't in the menu**: make sure it's the **VST3** version (not VST2/.dll or AAX) and installed in the VST3 folder, then **Look for new plugins…**.
- **The app closed while using a plugin**: plugins run inside the app, so a broken one can close it. Your work is kept: when you reopen, the app offers to recover it. Tell Claude or the plugin's maker which plugin it was.
