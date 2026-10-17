# Sound effects

Make the little sounds your game needs: coins, jumps, lasers, explosions, hits, power-ups and menu blips. Each one is a small recipe saved in your project, so you (or your AI) can change it later and export it again. It works like the classic free tool *sfxr*.

Click **🔊 Sound FX** in the top bar (or **Export ▾ → Sound effects to Godot…**).

## Make one

- **Coin, Jump, Laser, Explosion, Hit, Power-up, Blip**: each click makes a **new** random sound of that kind and plays it. Don't like it? Click again: every click is a different take.
- **🎲 Randomize**: anything at all. Weird, sometimes wonderful.
- **✨ Vary**: a new take on the sound you have: every setting moves a little. Good for "like this, but not quite".
- **▶ Play** (or **Space** while the panel is open) plays it again. Sound effects play on top of the song, so you can hear them against your music. **Stop** silences one that's still playing.

The list on the left holds your sounds. Click one to choose it, **double-click** to rename it (the name becomes its file name: `coin` → `coin.wav`), **Delete** to remove it. Every change can be undone with **Ctrl+Z**.

## Shape it

The main sliders:

| Slider | What it does |
|---|---|
| **Pitch** | How high it starts. |
| **Length** | How long it lasts. |
| **Slide** | Pitch slides down (left: lasers, falling) or up (right: jumps, power-ups). Middle is steady. |
| **Punch** | A louder hit at the very start, for a "ping". |
| **Tone** | Duller (left) or brighter (right). |
| **Crunch** | Lo-fi, bit-crushed grit, like an old console. |
| **Volume** | Overall level. |

Each change plays the sound when you let go of the slider.

**More settings** opens the rest:

- **Wave**: square (retro, hollow), saw (buzzy), sine (pure), triangle (soft), noise (hiss: explosions, hits, wind).
- **Attack, Hold, Fade out**: the shape over time (Length moves Hold and Fade out together).
- **Lowest pitch**: the sound stops if it slides below this. If a sound goes silent after you lower **Pitch**, this is why: the panel shows a ⚠ warning, and lowering **Lowest pitch** brings it back.
- **Slide curve**: bends the slide as it goes.
- **Vibrato** and **Vibrato speed**: a pitch wobble.
- **Pitch jump** and **Jump timing**: a sudden jump up (the coin's "ding-*ding*") or down, and how soon it happens.
- **Square width** and **Width sweep** (square wave only): from hollow to thin and nasal.
- **Repeat**: restarts the slide and jump over and over: alarms, trills, rolling explosions.
- **Phaser** and **Phaser sweep**: a swirling, jet-like colour.
- **Resonance**, **Tone sweep**: a whistle at the Tone setting, and opening or closing the tone over time.
- **Thin** and **Thin sweep**: cuts the low end (a high-pass filter).

## Recipes

- **Softer coin**: Coin, then lower **Punch** and **Tone** a little.
- **Bigger explosion**: Explosion, more **Length**, lower **Pitch**, a little **Repeat**.
- **Retro laser**: Laser with the square wave, **Crunch** up a bit.
- **Menu click**: Blip, **Length** almost all the way down.
- **Heavier jump**: Jump with lower **Pitch** and less **Slide**.

## Export to Godot

At the bottom of the panel:

1. **Godot project**: browse to the folder with `project.godot` (shared with the music export, and remembered).
2. **Folder in the project**: `sfx` by default (`res://sfx/`).
3. **Export this sound** or **Export all**.

Each sound becomes a WAV file named after it (`res://sfx/coin.wav`). If you rename a sound and export again, the old file stays in the Godot folder (a scene might still use it), and the list names it so you can delete it there if the game no longer does. The list shows what was written and how long each is. A sound that would clip is turned down just enough, and the list says so. Godot imports them ready to use (sound effects don't loop).

To play one in Godot, add an **AudioStreamPlayer** node, drag the WAV onto its **Stream**, and call `play()`:

```gdscript
$CoinSound.play()
```

## Ask your AI

Your AI can do all of this: make sounds, read and change any setting, play them for you, and export them to Godot. Small local models connected with the short tool list can too. Try:

- "Make three jump sounds, one heavier than the others."
- "Make the coin softer and a bit lower."
- "Add a power-up sound that rises and repeats, then export all the sounds to my Godot project."

It can't hear them, so tell it how they sound.
