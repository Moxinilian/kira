use cpal::{
	traits::{DeviceTrait, HostTrait, StreamTrait},
	Stream,
};
use ringbuf::{Consumer, RingBuffer};

use crate::{AudioError, AudioResult};

use super::{
	processor::{Processor, ProcessorThreadChannels},
	AudioManagerSettings,
};

const WRAPPER_THREAD_SLEEP_DURATION: f64 = 1.0 / 60.0;

pub(crate) trait Backend<CustomEvent: Copy + Send + 'static + std::fmt::Debug> {
	fn start(
		&mut self,
		settings: AudioManagerSettings,
		processor_thread_channels: ProcessorThreadChannels<CustomEvent>,
		quit_signal_consumer: Consumer<bool>,
	) -> AudioResult<()>;
}

pub struct CpalBackend {}

impl CpalBackend {
	pub fn new() -> Self {
		Self {}
	}
}

impl<CustomEvent: Copy + Send + 'static + std::fmt::Debug> Backend<CustomEvent> for CpalBackend {
	fn start(
		&mut self,
		settings: AudioManagerSettings,
		processor_thread_channels: ProcessorThreadChannels<CustomEvent>,
		mut quit_signal_consumer: Consumer<bool>,
	) -> AudioResult<()> {
		let (mut setup_result_producer, mut setup_result_consumer) =
			RingBuffer::<AudioResult<()>>::new(1).split();
		// set up a cpal stream on a new thread. we could do this on the main thread,
		// but that causes issues with LÖVE.
		std::thread::spawn(move || {
			let setup_result = || -> AudioResult<Stream> {
				let host = cpal::default_host();
				let device = match host.default_output_device() {
					Some(device) => device,
					None => return Err(AudioError::NoDefaultOutputDevice),
				};
				let config = match device.supported_output_configs()?.next() {
					Some(config) => config,
					None => return Err(AudioError::NoSupportedAudioConfig),
				}
				.with_max_sample_rate()
				.config();
				let sample_rate = config.sample_rate.0;
				let channels = config.channels;
				let mut processor =
					Processor::new(sample_rate, settings, processor_thread_channels);
				let stream = device.build_output_stream(
					&config,
					move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
						for frame in data.chunks_exact_mut(channels as usize) {
							let out = processor.process();
							frame[0] = out.left;
							frame[1] = out.right;
						}
					},
					move |_| {},
				)?;
				stream.play()?;
				Ok(stream)
			}();
			match setup_result {
				Ok(_stream) => {
					setup_result_producer.push(Ok(())).unwrap();
					// wait for a quit message before ending the thread and dropping
					// the stream
					while let None = quit_signal_consumer.pop() {
						std::thread::sleep(std::time::Duration::from_secs_f64(
							WRAPPER_THREAD_SLEEP_DURATION,
						));
					}
				}
				Err(error) => {
					setup_result_producer.push(Err(error)).unwrap();
				}
			}
		});
		// wait for the audio thread to report back a result
		loop {
			if let Some(result) = setup_result_consumer.pop() {
				match result {
					Ok(_) => break,
					Err(error) => return Err(error),
				}
			}
		}
		Ok(())
	}
}
