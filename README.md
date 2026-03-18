# Firewheel CLAP Node

An audio node for [Firewheel](https://github.com/BillyDM/Firewheel) that loads a [CLAP](https://cleveraudio.org/)
plugin. It supports bevy_seedling as well, just be sure to enable the `bevy` and maybe `bevy_reflect` features.

It is a very new crate, and there are sure to be issues and missing features.

## Features

- [X] Load arbitrary CLAP plugins by path and ID
- [X] Process audio with the plugin
- [X] Works with bevy_seedling
- [X] Set CLAP parameters
- [ ] Send MIDI data

## TODO

- [X] CLAP parameters
- [X] Add bevy example
    - [X] Add features for bevy, bevy_reflect, and serde
- [X] Support for arbitrary audio port + channel configurations
- [ ] Tests
    - [ ] For various channel configurations
- [ ] MIDI support?
- [ ] Realtime logging

# Usage

## Initialization

This node requires configuration to initialize successfully.

In firewheel that looks like

```rust
fn main() {
    // Initialize firewheel context...

    let mut clap_node = Memo::new(ClapPluginNode::default());
    let clap_node_id = cx.add_node(
        (*clap_node).clone(),
        Some(ClapPluginNodeConfig {
            path: "target/bundled/delay_plugin.clap".into(),
            id: "firewheel_clap_node.delay".to_string(),
            num_input_channels: 2.into(),
            num_output_channels: 2.into(),
        }),
    );

    // Connect up nodes like normal
}
```

and in bevy_seedling that looks like

```rust
fn play_sound(mut commands: Commands, server: Res<AssetServer>) {
    commands.spawn((
        Name::new("Poem Sound"),
        SamplePlayer::new(server.load("poetry.wav")).looping(),
        sample_effects![(
            Name::new("Clap Delay for Poem"),
            ClapPluginNode::default(),
            ClapPluginNodeConfig {
                path: "target/bundled/delay_plugin.clap".into(),
                id: "firewheel_clap_node.delay".to_string(),
                num_input_channels: 2.into(),
                num_output_channels: 2.into(),
            }
        )],
    ));
}
```

where path is an absolute or relative `path` to a clap bundle, and `id` is the CLAP id of the plugin in that bundle.

## Parameters

You can modify CLAP plugin parameters by knowing their parameter ID (u32) and modifying the `ClapPluginNode`'s parameter
mapping

Firewheel

```rust
fn main() {
    // Initialization

    clap_node.params.mapping.insert(95467907, 0.007);
    clap_node.params.mapping.insert(773352680, 0.0);

    clap_node.update_memo(&mut cx.event_queue(clap_node_id));
}
```

bevy_seedling takes care of diffing for you, so you just need to modify the `ClapPluginNode` component, similar to other
`AudioNode`s

### How can I find these parameter IDs? Why is it like this?

Due to some awkwardness in how firewheel initialization currently is, it doesn't seem like I can populate this map
beforehand with human-readable friendly parameter names and such.

To help with finding the parameter ID you want to insert into the map, I `info!()` log every parameter with its name and
CLAP parameter ID when a plugin is loaded. These IDs are stable across runs and are usually even stable across versions
of a CLAP plugin.

## License

Licensed under either of

* Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0), or
* MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.