use firewheel::cpal::CpalStream;
use firewheel::nodes::sampler::{RepeatMode, SamplerConfig, SamplerNode};
use firewheel::FirewheelContext;
use firewheel_clap_node::clap_plugin::{ClapPluginNode, ClapPluginNodeConfig};
use std::time::Duration;
use symphonium::SymphoniumLoader;

const UPDATE_INTERVAL: Duration = Duration::from_millis(15);

fn main() {
    let mut cx = FirewheelContext::new(Default::default());
    let mut stream = CpalStream::new(&mut cx, Default::default()).unwrap();

    let mut loader = SymphoniumLoader::new();

    let sample_rate = cx.stream_info().unwrap().sample_rate;

    let sample = firewheel::load_audio_file(
        &mut loader,
        "assets/test_audio/what_is_freesound.mp3",
        Some(sample_rate),
        Default::default(),
    )
    .unwrap()
    .into_dyn_resource();

    let mut sampler_node = SamplerNode::default();

    sampler_node.set_sample(sample);
    sampler_node.repeat_mode = RepeatMode::RepeatEndlessly;
    *sampler_node.play = true;

    let sampler_node_id = cx
        .add_node(
            sampler_node,
            Some(SamplerConfig {
                channels: 1.into(),
                num_declickers: 2,
                speed_quality: Default::default(),
            }),
        )
        .expect("Failed to add sampler node");

    let clap_node = ClapPluginNode { enabled: true };

    let clap_node_id = cx
        .add_node(
            clap_node,
            Some(ClapPluginNodeConfig {
                path: "target/bundled/delay_plugin.clap".into(),
                id: "firewheel_clap_node.delay".to_string(),
            }),
        )
        .expect("Failed to add clap node");

    let graph_out_id = cx.graph_out_node_id();

    cx.connect(sampler_node_id, clap_node_id, &[(0, 0), (0, 1)], false)
        .unwrap();

    cx.connect(clap_node_id, graph_out_id, &[(0, 1), (0, 1)], false)
        .unwrap();

    loop {
        if let Err(e) = cx.update() {
            eprintln!("{:?}", &e);
        }

        if let Err(e) = stream.poll_status() {
            eprintln!("{:?}", &e);

            break;
        }

        std::thread::sleep(UPDATE_INTERVAL);
    }
}
