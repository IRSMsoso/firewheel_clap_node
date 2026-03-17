use nih_plug::prelude::*;
use std::iter::zip;
use std::sync::Arc;

const MAX_DELAY: f32 = 10.0;

#[derive(Clone)]
struct DelayBuffer {
    buffer: Vec<f32>,
    index: usize,
}

impl DelayBuffer {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            index: 0,
        }
    }
}

struct Delay {
    params: Arc<DelayParams>,
    delay_buffers: Vec<DelayBuffer>,
    sample_rate: f32,
}

#[derive(Params)]
struct DelayParams {
    #[id = "delay_sec"]
    pub delay_sec: FloatParam,

    #[id = "bypass"]
    pub bypass: BoolParam,
}

impl Default for Delay {
    fn default() -> Self {
        Self {
            params: Arc::new(DelayParams::default()),
            delay_buffers: Vec::new(),
            sample_rate: 0.0,
        }
    }
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            delay_sec: FloatParam::new(
                "Delay in Seconds",
                1.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: MAX_DELAY,
                },
            ),
            bypass: BoolParam::new("Bypass", false),
        }
    }
}

impl Plugin for Delay {
    const NAME: &'static str = "Delay";
    const VENDOR: &'static str = "Firewheel Clap Node";
    // You can use `env!("CARGO_PKG_HOMEPAGE")` to reference the homepage field from the
    // `Cargo.toml` file here
    const URL: &'static str = "https://github.com/IRSMsoso/firewheel_clap_node";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),

        aux_input_ports: &[],
        aux_output_ports: &[],

        // Individual ports and the layout as a whole can be named here. By default, these names
        // are generated as needed. This layout will be called 'Stereo', while the other one is
        // given the name 'Mono' based no the number of input and output channels.
        names: PortNames::const_default(),
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();

    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.delay_buffers = (0..2)
            .map(|_| DelayBuffer {
                buffer: vec![0.0; (buffer_config.sample_rate * MAX_DELAY) as usize],
                index: 0,
            })
            .collect();

        self.sample_rate = buffer_config.sample_rate;

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for channel_samples in buffer.iter_samples() {
            let delay_sec = self.params.delay_sec.value();
            let num_delay_samples = ((delay_sec * self.sample_rate) as usize).max(1);

            for (delay_buffer, sample) in zip(self.delay_buffers.iter_mut(), channel_samples) {
                let index = delay_buffer.index % num_delay_samples;
                let next_index = (delay_buffer.index + 1) % num_delay_samples;

                delay_buffer.buffer[index] = *sample;

                let delayed_sample = delay_buffer.buffer[next_index];

                if !self.params.bypass.value() {
                    *sample = (*sample * 0.5) + (delayed_sample * 0.5);
                }

                delay_buffer.index = next_index;
            }
        }

        ProcessStatus::Normal
    }

    // This can be used for cleaning up special resources like socket connections whenever the
    // plugin is deactivated. Most plugins won't need to do anything here.
    fn deactivate(&mut self) {}
}

impl ClapPlugin for Delay {
    const CLAP_ID: &'static str = "firewheel_clap_node.delay";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A delay example plugin");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

nih_export_clap!(Delay);
