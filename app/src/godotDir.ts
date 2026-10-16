// The Godot project folder, remembered between exports (music and sound
// effects share it). A convenience only: storage may be unavailable.

const STORAGE_KEY = "npt.godotProjectDir";

/** The Godot project folder last exported to (shared with the sound effects panel). */
export function rememberedGodotDir(): string {
  try {
    return window.localStorage.getItem(STORAGE_KEY) ?? "";
  } catch {
    return "";
  }
}

/** Remembers the Godot project folder for next time. */
export function rememberGodotDir(dir: string): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, dir);
  } catch {
    // Remembering the folder is a convenience only.
  }
}
