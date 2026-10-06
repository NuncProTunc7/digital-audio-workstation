use super::{Command, CommandError, check_value};
use crate::effect::{Effect, EffectKind, effect_spec};
use crate::project::{EffectId, Project, TrackId};

/// Maximum effects per chain; keeps chains understandable and cheap.
pub const MAX_EFFECTS_PER_CHAIN: usize = 16;

fn chain_mut(
    project: &mut Project,
    track_id: Option<TrackId>,
) -> Result<&mut Vec<Effect>, CommandError> {
    match track_id {
        None => Ok(&mut project.master.effects),
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
    chain.insert(index, Effect::new(id, kind));
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
