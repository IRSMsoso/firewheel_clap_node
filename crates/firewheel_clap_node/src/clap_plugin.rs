use clack_extensions::audio_ports::{
    HostAudioPorts, HostAudioPortsImpl, PluginAudioPorts, RescanType,
};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamInfoBuffer,
    ParamRescanFlags, PluginParams,
};
use clack_host::entry::PluginEntryError;
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use clack_host::utils::Cookie;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, EventQueue, Patch, PatchError, PathBuilder};
use firewheel_core::event::{ParamData, ProcEvents};
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, ProcBuffers,
    ProcExtra, ProcInfo,
};
use log::{debug, error, info, warn};
use std::collections::HashMap;
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

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Component))]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClapPluginParams {
    // Mapping between CLAP parameter ID and that parameter's value
    pub mapping: HashMap<u32, f64>,
}

impl ClapPluginParams {
    fn new() -> Self {
        Self {
            mapping: HashMap::new(),
        }
    }
}

impl Patch for ClapPluginParams {
    type Patch = (u32, f64);

    fn patch(data: &ParamData, path: &[u32]) -> Result<Self::Patch, PatchError> {
        match data {
            ParamData::F64(value) => Ok((
                *path
                    .iter()
                    .next_back()
                    .ok_or_else(|| PatchError::InvalidPath)?,
                *value,
            )),
            _ => Err(PatchError::InvalidData),
        }
    }

    fn apply(&mut self, patch: Self::Patch) {
        self.mapping.insert(patch.0, patch.1);
    }
}

impl Diff for ClapPluginParams {
    fn diff<E: EventQueue>(&self, baseline: &Self, path: PathBuilder, event_queue: &mut E) {
        for param in &self.mapping {
            let base_line_value = baseline.mapping.get(param.0);

            match base_line_value {
                Some(base_line_value) => {
                    // If we already have that key
                    param
                        .1
                        .diff(base_line_value, path.with(*param.0), event_queue)
                }
                None => {
                    // New value
                    event_queue.push_param(*param.1, path.with(*param.0));
                }
            }
        }
    }
}

/// A node that hosts a CLAP plugin
#[derive(Diff, Patch, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Component))]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClapPluginNode {
    pub params: ClapPluginParams,
}

impl Default for ClapPluginNode {
    fn default() -> Self {
        Self {
            params: ClapPluginParams::new(),
        }
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

        let mut plugin_instance = PluginInstance::<FirewheelClapHost>::new(
            |_| FirewheelClapShared::new(),
            |_| FirewheelClapMain,
            &entry,
            &id,
            &host_info(),
        )
        .expect("Firewheel construction error handling not merged yet");

        let name = match plugin_instance.plugin_shared_handle().descriptor() {
            Some(descriptor) => match descriptor.name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => "Unknown".into(),
            },
            None => "Unknown".into(),
        };

        let params = plugin_instance.access_shared_handler(|shared| {
            shared
                .extensions
                .get()
                .expect("Plugin extensions should be initialized")
                .params
        });

        if let Some(params) = params {
            let mut plugin_handle = plugin_instance.plugin_handle();

            // Get the total number of parameters
            let param_count = params.count(&mut plugin_handle);

            info!("Clap plugin \"{}\" loaded", name);
            let mut parameters_log = "[Parameters]\n".to_string();

            // Iterate through all parameters
            for i in 0..param_count {
                let mut buffer = ParamInfoBuffer::new();

                // Get parameter info for this index
                if let Some(param_info) =
                    params.get_info(&mut plugin_instance.plugin_handle(), i, &mut buffer)
                {
                    // Extract the parameter name
                    let param_name = String::from_utf8(Vec::from(param_info.name))
                        .unwrap_or("Unknown".to_string());

                    parameters_log += &format!("- {}: {:?}\n", param_info.id.get(), param_name);
                }
            }

            info!("{}", parameters_log);
        }

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
        let mut clap_events = Vec::new();

        for patch in events.drain_patches::<ClapPluginNode>() {
            match patch {
                ClapPluginNodePatch::Params(param) => {
                    clap_events.push(ParamValueEvent::new(
                        0,
                        ClapId::from(param.0),
                        Pckn::match_all(),
                        param.1,
                        Cookie::default(),
                    ));
                }
            };
        }

        if !clap_events.is_empty() {
            // Retrieve params extension
            let params = self.audio_processor.access_shared_handler(|shared| {
                shared
                    .extensions
                    .get()
                    .expect("Extensions should be initialized")
                    .params
            });

            // Flush param changes to processor
            if let Some(params) = params {
                params.flush_active(
                    &mut self.audio_processor.plugin_handle(),
                    &InputEvents::from_buffer(&clap_events),
                    &mut OutputEvents::void(),
                )
            }
        }

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
struct CachedExtensions {
    /// A handle to the plugin's Audio Ports extension, if it supports it.
    audio_ports: Option<PluginAudioPorts>,
    params: Option<PluginParams>,
}

#[derive(Default)]
pub struct FirewheelClapShared {
    extensions: OnceLock<CachedExtensions>,
}

impl FirewheelClapShared {
    fn new() -> Self {
        Self {
            extensions: OnceLock::new(),
        }
    }
}

// impl<'a> SharedHandler<'a> for MinimalShared {}
impl<'a> SharedHandler<'a> for FirewheelClapShared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.extensions.set(CachedExtensions {
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
