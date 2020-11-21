//! Provides a bridge between the main thread and the audio thread.

pub mod backend;
pub(crate) mod processor;

use crate::{
	command::{
		Command, InstanceCommand, MetronomeCommand, MixerCommand, ParameterCommand,
		SequenceCommand, SoundCommand,
	},
	error::{AudioError, AudioResult},
	instance::{InstanceId, InstanceSettings},
	metronome::MetronomeSettings,
	mixer::{
		effect::{Effect, EffectId, EffectSettings},
		effect_slot::EffectSlot,
		SubTrackId, Track, TrackIndex, TrackSettings,
	},
	parameter::{ParameterId, Tween},
	sequence::{Sequence, SequenceId},
	sound::{Sound, SoundId},
	tempo::Tempo,
	value::Value,
	Event,
};
use cpal::{
	traits::{DeviceTrait, HostTrait, StreamTrait},
	Stream,
};
use processor::Processor;
use ringbuf::{Consumer, Producer, RingBuffer};

use self::{
	backend::{Backend, CpalBackend},
	processor::ProcessorThreadChannels,
};

/// Settings for an `AudioManager`.
#[derive(Debug, Clone)]
pub struct AudioManagerSettings {
	/// The number of commands that be sent to the audio thread at a time.
	///
	/// Each action you take, like starting an instance or pausing a sequence,
	/// queues up one command.
	pub num_commands: usize,
	/// The number of events the audio thread can send at a time.
	pub num_events: usize,
	/// The maximum number of sounds that can be loaded at a time.
	pub num_sounds: usize,
	/// The maximum number of parameters that can exist at a time.
	pub num_parameters: usize,
	/// The maximum number of instances of sounds that can be playing at a time.
	pub num_instances: usize,
	/// The maximum number of sequences that can be running at a time.
	pub num_sequences: usize,
	/// The maximum number of mixer tracks that can be used at a time.
	pub num_tracks: usize,
	/// The maximum number of effects that can be running at a time on a mixer track.
	pub num_effects_per_track: usize,
	/// Settings for the metronome.
	pub metronome_settings: MetronomeSettings,
}

impl Default for AudioManagerSettings {
	fn default() -> Self {
		Self {
			num_commands: 100,
			num_events: 100,
			num_sounds: 100,
			num_parameters: 100,
			num_instances: 100,
			num_sequences: 25,
			num_tracks: 100,
			num_effects_per_track: 10,
			metronome_settings: MetronomeSettings::default(),
		}
	}
}

pub(crate) struct AudioManagerThreadChannels<CustomEvent: Copy + Send + 'static = ()> {
	pub quit_signal_producer: Producer<bool>,
	pub command_producer: Producer<Command<CustomEvent>>,
	pub event_consumer: Consumer<Event<CustomEvent>>,
	pub sounds_to_unload_consumer: Consumer<Sound>,
	pub sequences_to_unload_consumer: Consumer<Sequence<CustomEvent>>,
	pub tracks_to_unload_consumer: Consumer<Track>,
	pub effect_slots_to_unload_consumer: Consumer<EffectSlot>,
}

/**
Plays and manages audio.

The `AudioManager` is responsible for all communication between the gameplay thread
and the audio thread.
*/
pub struct AudioManager<CustomEvent: Copy + Send + 'static = ()> {
	thread_channels: AudioManagerThreadChannels<CustomEvent>,
}

impl<CustomEvent: Copy + Send + 'static + std::fmt::Debug> AudioManager<CustomEvent> {
	fn create_thread_channels(
		settings: &AudioManagerSettings,
	) -> (
		AudioManagerThreadChannels<CustomEvent>,
		ProcessorThreadChannels<CustomEvent>,
		Consumer<bool>,
	) {
		let (quit_signal_producer, quit_signal_consumer) = RingBuffer::new(1).split();
		let (command_producer, command_consumer) = RingBuffer::new(settings.num_commands).split();
		let (sounds_to_unload_producer, sounds_to_unload_consumer) =
			RingBuffer::new(settings.num_sounds).split();
		let (sequences_to_unload_producer, sequences_to_unload_consumer) =
			RingBuffer::new(settings.num_sequences).split();
		let (tracks_to_unload_producer, tracks_to_unload_consumer) =
			RingBuffer::new(settings.num_tracks).split();
		let (effect_slots_to_unload_producer, effect_slots_to_unload_consumer) =
			RingBuffer::new(settings.num_tracks * settings.num_effects_per_track).split();
		let (event_producer, event_consumer) = RingBuffer::new(settings.num_events).split();
		let audio_manager_thread_channels = AudioManagerThreadChannels {
			quit_signal_producer,
			command_producer,
			event_consumer,
			sounds_to_unload_consumer,
			sequences_to_unload_consumer,
			tracks_to_unload_consumer,
			effect_slots_to_unload_consumer,
		};
		let processor_thread_channels = ProcessorThreadChannels {
			command_consumer,
			event_producer,
			sounds_to_unload_producer,
			sequences_to_unload_producer,
			tracks_to_unload_producer,
			effect_slots_to_unload_producer,
		};
		(
			audio_manager_thread_channels,
			processor_thread_channels,
			quit_signal_consumer,
		)
	}

	fn new_with_backend<B: Backend<CustomEvent>>(
		mut backend: B,
		settings: AudioManagerSettings,
	) -> AudioResult<Self> {
		let (audio_manager_thread_channels, processor_thread_channels, quit_signal_consumer) =
			Self::create_thread_channels(&settings);
		backend.start(settings, processor_thread_channels, quit_signal_consumer)?;
		Ok(Self {
			thread_channels: audio_manager_thread_channels,
		})
	}

	/// Creates a new audio manager and starts an audio thread.
	pub fn new(settings: AudioManagerSettings) -> AudioResult<Self> {
		let backend = CpalBackend::new();
		Self::new_with_backend(backend, settings)
	}

	fn send_command_to_processor(&mut self, command: Command<CustomEvent>) -> AudioResult<()> {
		match self.thread_channels.command_producer.push(command) {
			Ok(_) => Ok(()),
			Err(_) => Err(AudioError::CommandQueueFull),
		}
	}

	/// Sends a sound to the audio thread and returns a handle to the sound.
	pub fn add_sound(&mut self, sound: Sound) -> AudioResult<SoundId> {
		let id = SoundId::new(sound.duration(), sound.default_track(), sound.metadata());
		self.send_command_to_processor(Command::Sound(SoundCommand::LoadSound(id, sound)))?;
		Ok(id)
	}

	/// Removes a sound from the audio thread, allowing its memory to be freed.
	pub fn remove_sound(&mut self, id: SoundId) -> AudioResult<()> {
		self.send_command_to_processor(Command::Sound(SoundCommand::UnloadSound(id)))
	}

	/// Plays a sound.
	pub fn play_sound(
		&mut self,
		sound_id: SoundId,
		settings: InstanceSettings,
	) -> Result<InstanceId, AudioError> {
		let instance_id = InstanceId::new();
		self.send_command_to_processor(Command::Instance(InstanceCommand::PlaySound(
			instance_id,
			sound_id,
			None,
			settings,
		)))?;
		Ok(instance_id)
	}

	/// Sets the volume of an instance.
	pub fn set_instance_volume<V: Into<Value<f64>>>(
		&mut self,
		id: InstanceId,
		volume: V,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::SetInstanceVolume(
			id,
			volume.into(),
		)))
	}

	/// Sets the pitch of an instance.
	pub fn set_instance_pitch<V: Into<Value<f64>>>(
		&mut self,
		id: InstanceId,
		pitch: V,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::SetInstancePitch(
			id,
			pitch.into(),
		)))
	}

	/// Pauses a currently playing instance of a sound with an optional fade-out tween.
	pub fn pause_instance(
		&mut self,
		instance_id: InstanceId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::PauseInstance(
			instance_id,
			fade_tween,
		)))
	}

	/// Resumes a currently paused instance of a sound with an optional fade-in tween.
	pub fn resume_instance(
		&mut self,
		instance_id: InstanceId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::ResumeInstance(
			instance_id,
			fade_tween,
		)))
	}

	/// Stops a currently playing instance of a sound with an optional fade-out tween.
	///
	/// Once the instance is stopped, it cannot be restarted.
	pub fn stop_instance(
		&mut self,
		instance_id: InstanceId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::StopInstance(
			instance_id,
			fade_tween,
		)))
	}

	/// Pauses all currently playing instances of a sound with an optional fade-out tween.
	pub fn pause_instances_of_sound(
		&mut self,
		sound_id: SoundId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::PauseInstancesOfSound(
			sound_id, fade_tween,
		)))
	}

	/// Resumes all currently playing instances of a sound with an optional fade-in tween.
	pub fn resume_instances_of_sound(
		&mut self,
		sound_id: SoundId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::ResumeInstancesOfSound(
			sound_id, fade_tween,
		)))
	}

	/// Stops all currently playing instances of a sound with an optional fade-out tween.
	pub fn stop_instances_of_sound(
		&mut self,
		sound_id: SoundId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Instance(InstanceCommand::StopInstancesOfSound(
			sound_id, fade_tween,
		)))
	}

	/// Sets the tempo of the metronome.
	pub fn set_metronome_tempo<T: Into<Value<Tempo>>>(
		&mut self,
		tempo: T,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Metronome(MetronomeCommand::SetMetronomeTempo(
			tempo.into(),
		)))
	}

	/// Starts or resumes the metronome.
	pub fn start_metronome(&mut self) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Metronome(MetronomeCommand::StartMetronome))
	}

	/// Pauses the metronome.
	pub fn pause_metronome(&mut self) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Metronome(MetronomeCommand::PauseMetronome))
	}

	/// Stops and resets the metronome.
	pub fn stop_metronome(&mut self) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Metronome(MetronomeCommand::StopMetronome))
	}

	/// Starts a sequence.
	pub fn start_sequence(
		&mut self,
		sequence: Sequence<CustomEvent>,
	) -> Result<SequenceId, AudioError> {
		sequence.validate()?;
		let id = SequenceId::new();
		self.send_command_to_processor(Command::Sequence(SequenceCommand::StartSequence(
			id, sequence,
		)))?;
		Ok(id)
	}

	/// Mutes a sequence.
	///
	/// When a sequence is muted, it will continue waiting for durations and intervals,
	/// but it will not play sounds, emit events, or perform any other actions.
	pub fn mute_sequence(&mut self, id: SequenceId) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::MuteSequence(id)))
	}

	/// Unmutes a sequence.
	pub fn unmute_sequence(&mut self, id: SequenceId) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::UnmuteSequence(id)))
	}

	/// Pauses a sequence.
	pub fn pause_sequence(&mut self, id: SequenceId) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::PauseSequence(id)))
	}

	/// Resumes a sequence.
	pub fn resume_sequence(&mut self, id: SequenceId) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::ResumeSequence(id)))
	}

	/// Stops a sequence.
	pub fn stop_sequence(&mut self, id: SequenceId) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::StopSequence(id)))
	}

	/// Pauses a sequence and any instances played by that sequence.
	pub fn pause_sequence_and_instances(
		&mut self,
		id: SequenceId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::PauseSequence(id)))?;
		self.send_command_to_processor(Command::Instance(
			InstanceCommand::PauseInstancesOfSequence(id, fade_tween),
		))
	}

	/// Resumes a sequence and any instances played by that sequence.
	pub fn resume_sequence_and_instances(
		&mut self,
		id: SequenceId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::ResumeSequence(id)))?;
		self.send_command_to_processor(Command::Instance(
			InstanceCommand::ResumeInstancesOfSequence(id, fade_tween),
		))
	}

	/// Stops a sequence and any instances played by that sequence.
	pub fn stop_sequence_and_instances(
		&mut self,
		id: SequenceId,
		fade_tween: Option<Tween>,
	) -> Result<(), AudioError> {
		self.send_command_to_processor(Command::Sequence(SequenceCommand::StopSequence(id)))?;
		self.send_command_to_processor(Command::Instance(InstanceCommand::StopInstancesOfSequence(
			id, fade_tween,
		)))
	}

	/// Creates a parameter with the specified starting value.
	pub fn add_parameter(&mut self, value: f64) -> AudioResult<ParameterId> {
		let id = ParameterId::new();
		self.send_command_to_processor(Command::Parameter(ParameterCommand::AddParameter(
			id, value,
		)))?;
		Ok(id)
	}

	/// Removes a parameter.
	pub fn remove_parameter(&mut self, id: ParameterId) -> AudioResult<()> {
		self.send_command_to_processor(Command::Parameter(ParameterCommand::RemoveParameter(id)))
	}

	/// Sets the value of a parameter with an optional tween to smoothly change the value.
	pub fn set_parameter(
		&mut self,
		id: ParameterId,
		value: f64,
		tween: Option<Tween>,
	) -> AudioResult<()> {
		self.send_command_to_processor(Command::Parameter(ParameterCommand::SetParameter(
			id, value, tween,
		)))
	}

	/// Creates a mixer sub-track.
	pub fn add_sub_track(&mut self, settings: TrackSettings) -> AudioResult<SubTrackId> {
		let id = SubTrackId::new();
		self.send_command_to_processor(Command::Mixer(MixerCommand::AddSubTrack(id, settings)))?;
		Ok(id)
	}

	/// Removes a sub-track from the mixer.
	pub fn remove_sub_track(&mut self, id: SubTrackId) -> AudioResult<()> {
		self.send_command_to_processor(Command::Mixer(MixerCommand::RemoveSubTrack(id)))
	}

	/// Adds an effect to a track.
	pub fn add_effect_to_track<T: Into<TrackIndex> + Copy>(
		&mut self,
		track_index: T,
		effect: Box<dyn Effect>,
		settings: EffectSettings,
	) -> AudioResult<EffectId> {
		let effect_id = EffectId::new(track_index.into());
		self.send_command_to_processor(Command::Mixer(MixerCommand::AddEffect(
			track_index.into(),
			effect_id,
			effect,
			settings,
		)))?;
		Ok(effect_id)
	}

	/// Removes an effect from the mixer.
	pub fn remove_effect(&mut self, effect_id: EffectId) -> AudioResult<()> {
		self.send_command_to_processor(Command::Mixer(MixerCommand::RemoveEffect(effect_id)))
	}

	/// Returns a list of all of the new events created by the audio thread
	/// (since the last time `events` was called).
	pub fn events(&mut self) -> Vec<Event<CustomEvent>> {
		let mut events = vec![];
		while let Some(event) = self.thread_channels.event_consumer.pop() {
			events.push(event);
		}
		events
	}

	/// Frees resources that are no longer in use, such as unloaded sounds
	/// or finished sequences.
	pub fn free_unused_resources(&mut self) {
		while let Some(_) = self.thread_channels.sounds_to_unload_consumer.pop() {}
		while let Some(_) = self.thread_channels.sequences_to_unload_consumer.pop() {}
		while let Some(_) = self.thread_channels.tracks_to_unload_consumer.pop() {}
		while let Some(_) = self.thread_channels.effect_slots_to_unload_consumer.pop() {}
	}
}

impl<CustomEvent: Copy + Send + 'static> Drop for AudioManager<CustomEvent> {
	fn drop(&mut self) {
		self.thread_channels
			.quit_signal_producer
			.push(true)
			.unwrap();
	}
}
