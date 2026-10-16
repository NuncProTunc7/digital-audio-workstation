//! Game sound effects: recipes kept in the project (see `crate::sound`).

use super::{Command, CommandError, invalid};
use crate::project::{Id, Project};
use crate::sound::{SoundCategory, SoundEffect, SoundRecipe, generate, vary};

/// Longest sound name, in characters.
const MAX_NAME_CHARS: usize = 40;

/// Seed used when a category is given without one, so the Command is pure.
const DEFAULT_SEED: u64 = 1;

fn check_name(name: &str) -> Result<String, CommandError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("sound name", "can't be empty"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid(
            "sound name",
            format!("must be at most {MAX_NAME_CHARS} characters"),
        ));
    }
    Ok(name)
}

fn taken(project: &Project, name: &str, except: Option<Id>) -> bool {
    project
        .sounds
        .iter()
        .any(|s| Some(s.id) != except && s.name == name)
}

/// `name`, or `name_2`, `name_3`... whichever is free.
fn unique_name(project: &Project, name: String) -> String {
    if !taken(project, &name, None) {
        return name;
    }
    (2..)
        .map(|n| format!("{name}_{n}"))
        .find(|n| !taken(project, n, None))
        .unwrap_or(name)
}

fn index_of(project: &Project, sound_id: Id) -> Result<usize, CommandError> {
    project
        .sounds
        .iter()
        .position(|s| s.id == sound_id)
        .ok_or_else(|| invalid("sound", format!("there is no sound with id {sound_id}")))
}

pub(super) fn add(
    project: &mut Project,
    name: Option<String>,
    recipe: Option<SoundRecipe>,
    category: Option<SoundCategory>,
    seed: Option<u64>,
) -> Result<Command, CommandError> {
    let recipe = match (recipe, category) {
        (Some(r), _) => r.clamped()?,
        (None, Some(c)) => generate(c, seed.unwrap_or(DEFAULT_SEED)),
        (None, None) => SoundRecipe::default(),
    };
    let base = match name {
        Some(n) => check_name(&n)?,
        None => category
            .map_or("sound", SoundCategory::default_name)
            .to_owned(),
    };
    let name = unique_name(project, base);
    let id = project.allocate_id();
    project.sounds.push(SoundEffect { id, name, recipe });
    Ok(Command::RemoveSound { sound_id: id })
}

pub(super) fn set(
    project: &mut Project,
    sound_id: Id,
    recipe: SoundRecipe,
) -> Result<Command, CommandError> {
    let recipe = recipe.clamped()?;
    let index = index_of(project, sound_id)?;
    let old = std::mem::replace(&mut project.sounds[index].recipe, recipe);
    Ok(Command::SetSound {
        sound_id,
        recipe: old,
    })
}

pub(super) fn vary_sound(
    project: &mut Project,
    sound_id: Id,
    seed: u64,
) -> Result<Command, CommandError> {
    let index = index_of(project, sound_id)?;
    let new = vary(&project.sounds[index].recipe, seed);
    set(project, sound_id, new)
}

pub(super) fn rename(
    project: &mut Project,
    sound_id: Id,
    name: String,
) -> Result<Command, CommandError> {
    let name = check_name(&name)?;
    let index = index_of(project, sound_id)?;
    if taken(project, &name, Some(sound_id)) {
        return Err(invalid(
            "sound name",
            format!("another sound is already called \"{name}\""),
        ));
    }
    let old = std::mem::replace(&mut project.sounds[index].name, name);
    Ok(Command::RenameSound {
        sound_id,
        name: old,
    })
}

pub(super) fn remove(project: &mut Project, sound_id: Id) -> Result<Command, CommandError> {
    let index = index_of(project, sound_id)?;
    let sound = project.sounds.remove(index);
    Ok(Command::RestoreSound { sound, index })
}

pub(super) fn restore(
    project: &mut Project,
    sound: SoundEffect,
    index: usize,
) -> Result<Command, CommandError> {
    let name = check_name(&sound.name)?;
    if project.id_in_use(sound.id) {
        return Err(CommandError::IdInUse(sound.id));
    }
    if taken(project, &name, None) {
        return Err(invalid(
            "sound name",
            format!("another sound is already called \"{name}\""),
        ));
    }
    let recipe = sound.recipe.clamped()?;
    project.reserve_id(sound.id);
    let id = sound.id;
    let index = index.min(project.sounds.len());
    project
        .sounds
        .insert(index, SoundEffect { id, name, recipe });
    Ok(Command::RemoveSound { sound_id: id })
}
