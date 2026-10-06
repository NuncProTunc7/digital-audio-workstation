# Godot 4.6 audio import formats (verified)

Notes for the Godot exporter, read from the Godot source at tag `4.6-stable`.

## `.ogg.import` (importer `oggvorbisstr`)
`modules/vorbis/resource_importer_ogg_vorbis.cpp`: options `loop` (false), `loop_offset` (0 s), `bpm` (0), `beat_count` (0), `bar_beats` (4, min 2).
A pre-written `.import` is honored: on first import Godot keeps `[params]`, fills defaults, and adds `uid`, `path`, `dest_files` (`editor/file_system/editor_file_system.cpp`). Always write `importer=`; without it, project importer defaults override the params. Minimal file:

```
[remap]
importer="oggvorbisstr"
type="AudioStreamOggVorbis"

[params]
loop=true
loop_offset=0.0
bpm=120.0
beat_count=16
bar_beats=4
```

With `loop` on and `bpm`/`beat_count` set, the loop length is `beat_count*60/bpm` (not the file length) and Godot crossfades 256 frames of what follows the loop end into the restart, so export the loop plus its tail.

## `.wav.import` (importer `wav`)
`edit/loop_mode`: 0 Detect From WAV (reads the `smpl` chunk's first loop), 1 Disabled, 2 Forward, 3 Ping-Pong, 4 Backward; `edit/loop_begin`/`edit/loop_end` in frames; `compress/mode` 0 PCM, 1 IMA ADPCM, 2 Quite OK Audio (default). Godot treats loop end as exclusive: write the exclusive end frame.

## Adaptive music resources (`modules/interactive_music/`)
- `AudioStreamSynchronized`: `stream_count`, `stream_N/stream`, `stream_N/volume` (dB), up to 32.
- `AudioStreamInteractive`: `clip_count` (write first), `clip_N/name` (`&"name"`), `clip_N/stream`, `clip_N/auto_advance`, `clip_N/next_clip`, `initial_clip`, `_transitions = { Vector2i(from, to): {"from_time", "to_time", "fade_mode", "fade_beats"} }` (-1 = any clip).
- `ext_resource` needs only `type`, `path`, `id`; `uid` optional. `format=3`.
