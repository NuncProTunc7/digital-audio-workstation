use super::{Command, CommandError, check_value};
use crate::effect::{Effect, EffectKind, effect_spec};
use crate::project::{EffectId, Project, TrackId};

/// Maximum effects per chain; keeps chains understandable and cheap.
pub const MAX_EFFECTS_PER_CHAIN: usize = 16;

pub(super) fn chain_mut(
    project: &mut Project,
    track_id: Option<TrackId>,
) -> Result<&mut Vec<Effect>, CommandError> {
    match track_id {
        None => Ok(&mut project.master.effects),
        // A bus's effects are addressed by its id.
        Some(id) if project.buses.iter().any(|b| b.id == id) => Ok(project
            .buses
            .iter_mut()
            .find(|b| b.id == id)
            .map(|b| &mut b.mixer.effects)
            .ok_or(CommandError::UnknownTrack(id))?),
        Some(id) => project
            .track_mut(id)
            .map(|t| &mut t.mixer.effects)
            .ok_or(CommandError::UnknownTrack(id)),
    }
}

fn find(chain: &[Effect], effect_id: EffectId) -> Result<usize, CommandError> {
    chain
        .iter()
        .position(|e| e.id == effect_id)
        .ok_or(CommandError::UnknownEffect(effect_id))
}

pub(super) fn add(
    project: &mut Project,
    track_id: Option<TrackId>,
    kind: EffectKind,
    index: Option<usize>,
) -> Result<Command, CommandError> {
    if kind == EffectKind::Plugin {
        return Err(super::invalid(
            "effect",
            "plugin effects are added with add_plugin_effect",
        ));
    }
    insert(project, track_id, index, |id| Effect::new(id, kind))
}

pub(super) fn add_plugin(
    project: &mut Project,
    track_id: Option<TrackId>,
    plugin: crate::plugin::PluginRef,
    index: Option<usize>,
) -> Result<Command, CommandError> {
    if plugin.uid.len() != 32 || plugin.path.is_empty() {
        return Err(super::invalid(
            "plugin",
            "needs an installed plugin's uid and path (use add_plugin_effect)",
        ));
    }
    insert(project, track_id, index, |id| Effect {
        plugin: Some(Box::new(plugin)),
        params: Default::default(),
        ..Effect::new(id, EffectKind::Plugin)
    })
}

/// Adds a new effect (made from its new id) to a chain.
fn insert(
    project: &mut Project,
    track_id: Option<TrackId>,
    index: Option<usize>,
    make: impl FnOnce(crate::project::Id) -> Effect,
) -> Result<Command, CommandError> {
    // Check the target exists before spending an id.
    if chain_mut(project, track_id)?.len() >= MAX_EFFECTS_PER_CHAIN {
        return Err(super::invalid(
            "effect chain",
            format!("can hold at most {MAX_EFFECTS_PER_CHAIN} effects"),
        ));
    }
    let id = project.allocate_id();
    let chain = chain_mut(project, track_id)?;
    let index = index.unwrap_or(chain.len()).min(chain.len());
    chain.insert(index, make(id));
    Ok(Command::RemoveEffect {
        track_id,
        effect_id: id,
    })
}

pub(super) fn remove(
    project: &mut Project,
    track_id: Option<TrackId>,
    effect_id: EffectId,
) -> Result<Command, CommandError> {
    let chain = chain_mut(project, track_id)?;
    let index = find(chain, effect_id)?;
    let effect = chain.remove(index);
    Ok(Command::RestoreEffect {
        track_id,
        index,
        effect,
    })
}

/// Replaces a chain with a rig's effects (new ids).
pub(super) fn load_rig(
    project: &mut Project,
    track_id: Option<TrackId>,
    name: String,
) -> Result<Command, CommandError> {
    let rig = crate::effect::rig(&name).ok_or_else(|| {
        let names: Vec<&str> = crate::effect::RIGS.iter().map(|r| r.name).collect();
        super::invalid(
            "rig",
            format!(
                "there is no rig called \"{name}\"; rigs: {}",
                names.join(", ")
            ),
        )
    })?;
    // Check the target exists before spending ids.
    chain_mut(project, track_id)?;
    let effects: Vec<Effect> = rig
        .chain
        .iter()
        .map(|pedal| {
            let mut e = Effect::new(project.allocate_id(), pedal.kind);
            for (id, value) in pedal.params {
                e.params.insert((*id).to_owned(), *value);
            }
            e
        })
        .collect();
    let old = std::mem::replace(chain_mut(project, track_id)?, effects);
    Ok(Command::RestoreEffectChain {
        track_id,
        effects: old,
    })
}

/// Puts a chain back exactly as given (undo of a rig, or its redo).
pub(super) fn restore_chain(
    project: &mut Project,
    track_id: Option<TrackId>,
    effects: Vec<Effect>,
) -> Result<Command, CommandError> {
    if effects.len() > MAX_EFFECTS_PER_CHAIN {
        return Err(super::invalid(
            "effect chain",
            format!("can hold at most {MAX_EFFECTS_PER_CHAIN} effects"),
        ));
    }
    for e in &effects {
        for (id, value) in &e.params {
            if e.kind == EffectKind::Plugin {
                continue;
            }
            let spec = effect_spec(e.kind, id).ok_or_else(|| CommandError::UnknownParam {
                kind: format!("{:?}", e.kind),
                param: id.clone(),
            })?;
            check_value(spec, *value)?;
        }
    }
    let old = std::mem::take(chain_mut(project, track_id)?);
    // The incoming ids must be free once the old chain is gone.
    if let Some(e) = effects.iter().find(|e| project.id_in_use(e.id)) {
        let id = e.id;
        *chain_mut(project, track_id)? = old;
        return Err(CommandError::IdInUse(id));
    }
    *chain_mut(project, track_id)? = effects;
    Ok(Command::RestoreEffectChain {
        track_id,
        effects: old,
    })
}

pub(super) fn restore(
    project: &mut Project,
    track_id: Option<TrackId>,
    index: usize,
    mut effect: Effect,
) -> Result<Command, CommandError> {
    if project.id_in_use(effect.id) {
        return Err(CommandError::IdInUse(effect.id));
    }
    for (id, value) in &effect.params {
        let spec = effect_spec(effect.kind, id).ok_or_else(|| CommandError::UnknownParam {
            kind: format!("{:?}", effect.kind),
            param: id.clone(),
        })?;
        check_value(spec, *value)?;
    }
    for spec in crate::effect::effect_params(effect.kind) {
        effect
            .params
            .entry(spec.id.to_owned())
            .or_insert(spec.default);
    }
    let effect_id = effect.id;
    let chain = chain_mut(project, track_id)?;
    if chain.len() >= MAX_EFFECTS_PER_CHAIN {
        return Err(super::invalid(
            "effect chain",
            format!("can hold at most {MAX_EFFECTS_PER_CHAIN} effects"),
        ));
    }
    let index = index.min(chain.len());
    chain.insert(index, effect);
    project.reserve_id(effect_id);
    Ok(Command::RemoveEffect {
        track_id,
        effect_id,
    })
}

pub(super) fn set_param(
    project: &mut Project,
    track_id: Option<TrackId>,
    effect_id: EffectId,
    param: String,
    value: f64,
) -> Result<Command, CommandError> {
    let chain = chain_mut(project, track_id)?;
    let index = find(chain, effect_id)?;
    let effect = &mut chain[index];
    let spec = effect_spec(effect.kind, &param).ok_or_else(|| CommandError::UnknownParam {
        kind: format!("{:?}", effect.kind),
        param: param.clone(),
    })?;
    check_value(spec, value)?;
    let old = effect.value(&param).unwrap_or(spec.default);
    effect.params.insert(param.clone(), value);
    Ok(Command::SetEffectParam {
        track_id,
        effect_id,
        param,
        value: old,
    })
}

pub(super) fn set_sidechain(
    project: &mut Project,
    track_id: Option<TrackId>,
    effect_id: EffectId,
    source: Option<TrackId>,
) -> Result<Command, CommandError> {
    if let Some(s) = source
        && project.track(s).is_none()
    {
        return Err(CommandError::UnknownTrack(s));
    }
    let chain = chain_mut(project, track_id)?;
    let index = find(chain, effect_id)?;
    if chain[index].kind != crate::effect::EffectKind::Compressor {
        return Err(super::invalid(
            "sidechain",
            "only compressors can listen to another track",
        ));
    }
    let old = std::mem::replace(&mut chain[index].sidechain, source);
    Ok(Command::SetEffectSidechain {
        track_id,
        effect_id,
        source: old,
    })
}

pub(super) fn set_enabled(
    project: &mut Project,
    track_id: Option<TrackId>,
    effect_id: EffectId,
    enabled: bool,
) -> Result<Command, CommandError> {
    let chain = chain_mut(project, track_id)?;
    let index = find(chain, effect_id)?;
    let old = std::mem::replace(&mut chain[index].enabled, enabled);
    Ok(Command::SetEffectEnabled {
        track_id,
        effect_id,
        enabled: old,
    })
}
