use bevy::prelude::*;
use bevy_inspector_egui::bevy_egui::EguiPlugin;
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_seedling::prelude::*;
use firewheel_clap_node::clap_plugin::{ClapPluginNode, ClapPluginNodeConfig};

fn main() {
    App::default()
        .add_plugins((DefaultPlugins, SeedlingPlugin::default()))
        .register_node::<ClapPluginNode>()
        .add_plugins(EguiPlugin::default())
        .add_plugins(WorldInspectorPlugin::new())
        .add_systems(Startup, camera)
        .add_systems(Startup, play_sound)
        .run();
}

fn camera(mut commands: Commands) {
    // Camera needed to show EGUI inspector
    commands.spawn(Camera2d);
}

fn play_sound(mut commands: Commands, server: Res<AssetServer>) {
    commands.spawn((
        Name::new("Poem Sound"),
        SamplePlayer::new(server.load("poetry.wav")).looping(),
        sample_effects![(
            Name::new("Clap Delay for Poem"),
            ClapPluginNode::default(),
            ClapPluginNodeConfig {
                path: "target/bundled/delay_plugin.clap".into(),
                id: "firewheel_clap_node.delay".to_string()
            }
        )],
    ));
}
