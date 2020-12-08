use crate::{
	arrangement::{Arrangement, ArrangementId},
	instance::{InstanceId, InstanceSettings},
	mixer::{
		effect::{Effect, EffectId, EffectSettings},
		SubTrackId, TrackIndex, TrackSettings,
	},
	parameter::{ParameterId, Tween},
	playable::Playable,
	sequence::{SequenceId, SequenceInstance},
	sound::{Sound, SoundId},
	tempo::Tempo,
	value::Value,
};

#[derive(Debug, Clone)]
pub(crate) enum ResourceCommand {
	AddSound(SoundId, Sound),
	RemoveSound(SoundId),
	AddArrangement(ArrangementId, Arrangement),
	RemoveArrangement(ArrangementId),
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum InstanceCommand {
	Play(InstanceId, Playable, Option<SequenceId>, InstanceSettings),
	SetInstanceVolume(InstanceId, Value<f64>),
	SetInstancePitch(InstanceId, Value<f64>),
	SetInstancePanning(InstanceId, Value<f64>),
	PauseInstance(InstanceId, Option<Tween>),
	ResumeInstance(InstanceId, Option<Tween>),
	StopInstance(InstanceId, Option<Tween>),
	PauseInstancesOf(Playable, Option<Tween>),
	ResumeInstancesOf(Playable, Option<Tween>),
	StopInstancesOf(Playable, Option<Tween>),
	PauseInstancesOfSequence(SequenceId, Option<Tween>),
	ResumeInstancesOfSequence(SequenceId, Option<Tween>),
	StopInstancesOfSequence(SequenceId, Option<Tween>),
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum MetronomeCommand {
	SetMetronomeTempo(Value<Tempo>),
	StartMetronome,
	PauseMetronome,
	StopMetronome,
}

pub(crate) enum SequenceCommand {
	StartSequence(SequenceId, SequenceInstance),
	MuteSequence(SequenceId),
	UnmuteSequence(SequenceId),
	PauseSequence(SequenceId),
	ResumeSequence(SequenceId),
	StopSequence(SequenceId),
}

#[derive(Debug)]
pub(crate) enum MixerCommand {
	AddSubTrack(SubTrackId, TrackSettings),
	RemoveSubTrack(SubTrackId),
	AddEffect(TrackIndex, EffectId, Box<dyn Effect>, EffectSettings),
	RemoveEffect(EffectId),
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum ParameterCommand {
	AddParameter(ParameterId, f64),
	RemoveParameter(ParameterId),
	SetParameter(ParameterId, f64, Option<Tween>),
}

pub(crate) enum Command {
	Resource(ResourceCommand),
	Instance(InstanceCommand),
	Metronome(MetronomeCommand),
	Sequence(SequenceCommand),
	Mixer(MixerCommand),
	Parameter(ParameterCommand),
}

impl From<ResourceCommand> for Command {
	fn from(command: ResourceCommand) -> Self {
		Self::Resource(command)
	}
}

impl From<InstanceCommand> for Command {
	fn from(command: InstanceCommand) -> Self {
		Self::Instance(command)
	}
}

impl From<MetronomeCommand> for Command {
	fn from(command: MetronomeCommand) -> Self {
		Self::Metronome(command)
	}
}

impl From<SequenceCommand> for Command {
	fn from(command: SequenceCommand) -> Self {
		Self::Sequence(command)
	}
}

impl From<MixerCommand> for Command {
	fn from(command: MixerCommand) -> Self {
		Self::Mixer(command)
	}
}

impl From<ParameterCommand> for Command {
	fn from(command: ParameterCommand) -> Self {
		Self::Parameter(command)
	}
}
