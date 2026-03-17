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

/// The [`Params`] derive macro gathers all of the information needed for the wrapper to know about
/// the plugin's parameters, persistent serializable fields, and nested parameter groups. You can
/// also easily implement [`Params`] by hand if you want to, for instance, have multiple instances
/// of a parameters struct for multiple identical oscillators/filters/envelopes.
#[derive(Params)]
struct DelayParams {
    /// The parameter's ID is used to identify the parameter in the wrapped plugin API. As long as
    /// these IDs remain constant, you can rename and reorder these fields as you wish. The
    /// parameters are exposed to the host in the same order they were defined. In this case, this
    /// gain parameter is stored as linear gain while the values are displayed in decibels.
    #[id = "delay_sec"]
    pub delay_sec: FloatParam,
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
            // This gain is stored as linear gain. NIH-plug comes with useful conversion functions
            // to treat these kinds of parameters as if we were dealing with decibels. Storing this
            // as decibels is easier to work with, but requires a conversion for every sample.
            delay_sec: FloatParam::new(
                "Delay in Seconds",
                1.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: MAX_DELAY,
                },
            ),
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

                *sample = (*sample * 0.5) + (delayed_sample * 0.5);

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
