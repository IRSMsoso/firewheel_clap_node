use clack_extensions::audio_ports::{
    HostAudioPorts, HostAudioPortsImpl, PluginAudioPorts, RescanType,
};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
    PluginParams,
};
use clack_host::entry::PluginEntryError;
use clack_host::prelude::*;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, Patch};
use firewheel_core::event::ProcEvents;
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, ProcBuffers,
    ProcExtra, ProcInfo,
};
use log::{debug, error, info, warn};
use std::ffi::{CString, NulError};
use std::path::PathBuf;
use std::sync::OnceLock;
use thiserror::Error;

/// Information about this host.
fn host_info() -> HostInfo {
    HostInfo::new(
        "Firewheel Clap Plugin Node Host",
        "Firewheel",
        "https://github.com/IRSMsoso/firewheel_clap_node",
        env!("CARGO_PKG_VERSION"),
    )
    .unwrap()
}

/// Errors that happened during finder.
#[derive(Error, Debug)]
pub enum ClapNodeError {
    #[error("Failed to load plugin: {0}")]
    LoadError(#[from] PluginEntryError),
    #[error("Plugin factory missing")]
    MissingPluginFactory,
    #[error("Plugin descriptor with ID not found")]
    IDNotFound,
    #[error("Plugin instance error: {0}")]
    PluginInstanceError(#[from] PluginInstanceError),
    #[error("Failed to parse provided ID: {0}")]
    ParseIDFailed(#[from] NulError),
    #[error("PluginInstance not found in custom data")]
    PluginInstanceCustomDataMissing,
}

/// A node that hosts a CLAP plugin
#[derive(Diff, Patch, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Component))]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClapPluginNode {}

impl Default for ClapPluginNode {
    fn default() -> Self {
        Self {}
    }
}

/// Configuration for a Clap Plugin Node. Both path and ID are required.
#[derive(Debug, Default, Clone, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Component))]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClapPluginNodeConfig {
    /// The path of the CLAP plugin
    pub path: PathBuf,

    /// The ID of the CLAP plugin
    pub id: String,
}

impl AudioNode for ClapPluginNode {
    type Configuration = ClapPluginNodeConfig;

    fn info(&self, configuration: &Self::Configuration) -> AudioNodeInfo {
        // Safety: Loading an external library object file is inherently unsafe
        let entry = unsafe {
            PluginEntry::load(configuration.path.as_os_str())
                .expect("Firewheel construction error handling not merged yet")
        };

        let plugin_factory = entry
            .get_plugin_factory()
            .ok_or_else(|| ClapNodeError::MissingPluginFactory)
            .expect("Firewheel construction error handling not merged yet");

        let id = CString::new(configuration.id.as_str())
            .expect("Firewheel construction error handling not merged yet");

        let _plugin_descriptor = plugin_factory
            .plugin_descriptors()
            .filter_map(|x| x.id())
            .find(|&plugin_id| plugin_id.eq(&id))
            .ok_or_else(|| ClapNodeError::IDNotFound)
            .expect("Firewheel construction error handling not merged yet");

        let plugin_instance = PluginInstance::<FirewheelClapHost>::new(
            |_| FirewheelClapShared::new(),
            |_| FirewheelClapMain,
            &entry,
            &id,
            &host_info(),
        )
        .expect("Firewheel construction error handling not merged yet");

        AudioNodeInfo::new()
            .debug_name("clap_plugin")
            .channel_config(ChannelConfig {
                // TODO: Dynamic channel count based on plugin?
                num_inputs: ChannelCount::STEREO,
                num_outputs: ChannelCount::STEREO,
            })
            .custom_state(plugin_instance)
    }

    fn construct_processor(
        &self,
        _configuration: &Self::Configuration,
        mut cx: ConstructProcessorContext,
    ) -> impl AudioNodeProcessor {
        let audio_config = PluginAudioConfiguration {
            sample_rate: f64::from(u32::from(cx.stream_info.sample_rate)),
            min_frames_count: 0,
            max_frames_count: u32::from(cx.stream_info.max_block_frames),
        };

        let plugin_instance = cx
            .custom_state_mut::<PluginInstance<FirewheelClapHost>>()
            .ok_or_else(|| ClapNodeError::PluginInstanceCustomDataMissing)
            .expect("Firewheel construction error handling not merged yet");

        // TODO: Configuration
        let input_channel_count = 2;
        let output_channel_count = 2;

        ClapPluginProcessor {
            audio_processor: plugin_instance
                .activate(|_, _| (), audio_config)
                .expect("Firewheel construction error handling not merged yet")
                .start_processing()
                .expect("Firewheel construction error handling not merged yet"),
            input_ports: AudioPorts::with_capacity(
                // TODO: Configuration
                input_channel_count,
                1,
            ),
            output_ports: AudioPorts::with_capacity(
                // TODO: Configuration
                output_channel_count,
                1,
            ),
            input_port_channels: Box::new([vec![
                0.0;
                input_channel_count
                    * audio_config.max_frames_count as usize
            ]]),
            output_port_channels: Box::new([vec![
                0.0;
                output_channel_count
                    * audio_config.max_frames_count as usize
            ]]),
            max_frames: audio_config.max_frames_count as usize,
        }
    }
}

pub struct ClapPluginProcessor {
    /// The started Clap audio processor
    audio_processor: StartedPluginAudioProcessor<FirewheelClapHost>,

    /// Buffers for the plugin's input ports information.
    input_ports: AudioPorts,
    /// Buffers for the plugin's output ports information.
    output_ports: AudioPorts,

    /// List of channel buffers for each input port.
    ///
    /// Note that all buffers for each channel are laid out continuously in a single allocation.
    input_port_channels: Box<[Vec<f32>]>,
    /// List of channel buffers for each output port.
    ///
    /// Note that all buffers for each channel are laid out continuously in a single allocation.
    output_port_channels: Box<[Vec<f32>]>,

    /// The max frames this processor can be called with
    max_frames: usize,
}

impl AudioNodeProcessor for ClapPluginProcessor {
    fn process(
        &mut self,
        info: &ProcInfo,
        buffers: ProcBuffers,
        events: &mut ProcEvents,
        _extra: &mut ProcExtra,
    ) -> firewheel_core::node::ProcessStatus {
        // for patch in events.drain_patches::<ClapPluginNode>() {
        //     match patch {}
        // }

        // Copy buffers host -> plugin
        let mut current_channel = 0;

        for port_buffer in self.input_port_channels.iter_mut() {
            // Each logical clap port is composed of a (frame * num_channels) sized buffer continuous in memory
            // Split it into chunks here.
            // We split on max_frames since that's how we initialized our clap buffers.
            for channel_buffer in port_buffer.chunks_exact_mut(self.max_frames) {
                if let Some(&host_input_slice) = buffers.inputs.get(current_channel) {
                    channel_buffer[..info.frames].copy_from_slice(host_input_slice);
                } else {
                    channel_buffer.fill(0.0);
                }
                current_channel += 1;
            }
        }

        // Setup input buffers for plugin processing
        let input = self
            .input_ports
            .with_input_buffers(self.input_port_channels.iter_mut().map(|port_buf| {
                AudioPortBuffer {
                    channels: AudioPortBufferType::f32_input_only(
                        port_buf
                            .chunks_exact_mut(info.frames)
                            .map(|buffer| InputChannel {
                                buffer: &mut buffer[..info.frames],
                                is_constant: true,
                            }),
                    ),
                    latency: 0,
                }
            }));

        // Setup output buffers for plugin processing
        let mut output =
            self.output_ports
                .with_output_buffers(self.output_port_channels.iter_mut().map(|port_buf| {
                    AudioPortBuffer {
                        latency: 0,
                        channels: AudioPortBufferType::f32_output_only(
                            port_buf
                                .chunks_exact_mut(info.frames)
                                .map(|buf| &mut buf[..info.frames]),
                        ),
                    }
                }));

        match self.audio_processor.process(
            &input,
            &mut output,
            &InputEvents::empty(),
            &mut OutputEvents::void(),
            None,
            None,
        ) {
            Ok(status) => {
                // Copy buffers plugin -> host
                let mut current_channel = 0;

                for port_buffer in self.output_port_channels.iter_mut() {
                    // Each logical clap port is composed of a (frame * num_channels) sized buffer continuous in memory
                    // Split it into chunks here.
                    // We split on max_frames since that's how we initialized our clap buffers.
                    for channel_buffer in port_buffer.chunks_exact_mut(self.max_frames) {
                        if let Some(host_output_slice) = buffers.outputs.get_mut(current_channel) {
                            host_output_slice.copy_from_slice(&channel_buffer[..info.frames]);
                        }
                        current_channel += 1;
                    }
                }

                match status {
                    ProcessStatus::Continue => firewheel_core::node::ProcessStatus::OutputsModified,
                    ProcessStatus::ContinueIfNotQuiet => {
                        firewheel_core::node::ProcessStatus::OutputsModified
                    }
                    ProcessStatus::Tail => firewheel_core::node::ProcessStatus::OutputsModified,
                    ProcessStatus::Sleep => firewheel_core::node::ProcessStatus::ClearAllOutputs,
                }
            }
            Err(_) => firewheel_core::node::ProcessStatus::ClearAllOutputs,
        }
    }
}

#[allow(dead_code)]
struct PluginCallbacks {
    /// A handle to the plugin's Audio Ports extension, if it supports it.
    audio_ports: Option<PluginAudioPorts>,
    params: Option<PluginParams>,
}

#[derive(Default)]
pub struct FirewheelClapShared {
    callbacks: OnceLock<PluginCallbacks>,
}

impl FirewheelClapShared {
    fn new() -> Self {
        Self {
            callbacks: OnceLock::new(),
        }
    }
}

// impl<'a> SharedHandler<'a> for MinimalShared {}
impl<'a> SharedHandler<'a> for FirewheelClapShared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.callbacks.set(PluginCallbacks {
            audio_ports: instance.get_extension(),
            params: instance.get_extension(),
        });
    }

    fn request_restart(&self) {}

    fn request_process(&self) {}

    fn request_callback(&self) {}
}

impl HostParamsImplShared for FirewheelClapShared {
    fn request_flush(&self) {}
}

impl HostLogImpl for FirewheelClapShared {
    fn log(&self, severity: LogSeverity, message: &str) {
        // TODO: Make this realtime safe with a MPSC ring buffer?

        // From clack cpal example:
        // Note: writing to stdout isn't realtime-safe, and should ideally be avoided.
        // This is only "good enough™" for an example.
        // A mpsc ringbuffer with support for dynamically-sized messages (`?Sized`) should be used to
        // send the logs the main thread without allocating or blocking.

        match severity {
            LogSeverity::Debug => debug!("{}", message),
            LogSeverity::Info => info!("{}", message),
            LogSeverity::Warning => warn!("{}", message),
            LogSeverity::Error => error!("{}", message),
            LogSeverity::Fatal => error!("[FATAL] {}", message),
            LogSeverity::HostMisbehaving => warn!("[HOST MISBEHAVING] {}", message),
            LogSeverity::PluginMisbehaving => warn!("[PLUGIN MISBEHAVING] {}", message),
        }
    }
}

#[derive(Default)]
pub struct FirewheelClapMain;

impl<'a> MainThreadHandler<'a> for FirewheelClapMain {}

impl HostAudioPortsImpl for FirewheelClapMain {
    fn is_rescan_flag_supported(&self, _flag: RescanType) -> bool {
        false
    }

    fn rescan(&mut self, _flag: RescanType) {
        // We don't support audio ports changing
    }
}

impl HostParamsImplMainThread for FirewheelClapMain {
    fn rescan(&mut self, _flags: ParamRescanFlags) {}

    fn clear(&mut self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

pub struct FirewheelClapHost;

impl HostHandlers for FirewheelClapHost {
    type Shared<'a> = FirewheelClapShared;
    type MainThread<'a> = FirewheelClapMain;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostAudioPorts>()
            .register::<HostParams>();
    }
}
