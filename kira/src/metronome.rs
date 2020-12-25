use crate::{
	command::MetronomeCommand, parameter::Parameters, tempo::Tempo, value::CachedValue, Value,
};
use std::vec::Drain;

#[derive(Debug, Clone)]
/// Settings for the metronome.
pub struct MetronomeSettings {
	/// The tempo of the metronome (in beats per minute).
	pub tempo: Value<Tempo>,
	/// Which intervals (in beats) the metronome should emit events for.
	///
	/// For example, if this is set to `vec![0.25, 0.5, 1.0]`, then
	/// the audio manager will receive `MetronomeIntervalPassed` events
	/// every quarter of a beat, half of a beat, and beat.
	pub interval_events_to_emit: Vec<f64>,
}

impl Default for MetronomeSettings {
	fn default() -> Self {
		Self {
			tempo: Tempo(120.0).into(),
			interval_events_to_emit: vec![],
		}
	}
}

#[derive(Debug, Clone)]
pub(crate) struct Metronome {
	tempo: CachedValue<Tempo>,
	interval_events_to_emit: Vec<f64>,
	ticking: bool,
	time: f64,
	previous_time: f64,
	interval_event_queue: Vec<f64>,
}

impl Metronome {
	pub fn new(settings: MetronomeSettings) -> Self {
		let num_interval_events = settings.interval_events_to_emit.len();
		Self {
			tempo: CachedValue::new(settings.tempo, Tempo(120.0)),
			interval_events_to_emit: settings.interval_events_to_emit,
			ticking: false,
			time: 0.0,
			previous_time: 0.0,
			interval_event_queue: Vec::with_capacity(num_interval_events),
		}
	}

	pub fn effective_tempo(&self) -> Tempo {
		if self.ticking {
			self.tempo.value()
		} else {
			Tempo(0.0)
		}
	}

	pub fn start(&mut self) {
		self.ticking = true;
	}

	pub fn pause(&mut self) {
		self.ticking = false;
	}

	pub fn stop(&mut self) {
		self.ticking = false;
		self.time = 0.0;
		self.previous_time = 0.0;
	}

	#[cfg_attr(feature = "trace", tracing::instrument)]
	pub fn run_command(&mut self, command: MetronomeCommand) {
		match command {
			MetronomeCommand::SetMetronomeTempo(tempo) => self.tempo.set(tempo),
			MetronomeCommand::StartMetronome => self.start(),
			MetronomeCommand::PauseMetronome => self.pause(),
			MetronomeCommand::StopMetronome => self.stop(),
		}
	}

	#[cfg_attr(feature = "trace", tracing::instrument)]
	pub fn update(&mut self, dt: f64, parameters: &Parameters) -> Drain<f64> {
		self.tempo.update(parameters);
		if self.ticking {
			self.previous_time = self.time;
			self.time += (self.tempo.value().0 / 60.0) * dt;
			for interval in &self.interval_events_to_emit {
				if self.interval_passed(*interval) {
					self.interval_event_queue.push(*interval);
				}
			}
		}
		self.interval_event_queue.drain(..)
	}

	pub fn interval_passed(&self, interval: f64) -> bool {
		if !self.ticking {
			return false;
		}
		if self.previous_time == 0.0 {
			return true;
		}
		(self.previous_time % interval) > (self.time % interval)
	}
}
