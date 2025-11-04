use bevy::{prelude::*, window::WindowResolution, window::PrimaryWindow};

const PIXEL_RATIO: f32 = 1.0;

fn main() {
    App::new()
    .add_plugins(
        DefaultPlugins
        .set(WindowPlugin {
                primary_window: Some(Window {
                    title: String::from("Flappy Bird"),
                    position: WindowPosition::Centered(MonitorSelection::Primary),
                    resolution: WindowResolution::new(800, 400),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .set(ImagePlugin::default_nearest())
    )
    .add_systems(Startup, setup_app)
    .add_systems(Update, update_man)
    .run();
}

#[derive(Component)]
struct Man {
    position: Vec2
}

fn setup_app(
    mut commands: Commands,
    assets_server: Res<AssetServer>,
    window_query: Query<&Window, With<PrimaryWindow>>,
) {
    let window = window_query.single().expect("Can't get window");

    commands.spawn(Camera2d::default());

    commands.spawn((
        Sprite {
            image: assets_server.load("world_location_map.png"),
            ..Default::default()
        },
        Transform::from_xyz(0., 0., -1.),
    ));

    commands.spawn((
        Sprite {
            image: assets_server.load("man.png"),
            ..Default::default()
        },
        Transform::IDENTITY.with_scale(Vec3::splat(PIXEL_RATIO)),
        Man { position: Vec2::new(window.width() / 2.0, window.height() / 2.0) }
    ));
}

fn update_man(
    mut man_query: Query<(&mut Transform, &mut Man)>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>
) {
    if let Ok((mut transform, mut man)) = man_query.single_mut() {
        let mut direction = Vec2::ZERO;

        if keys.pressed(KeyCode::KeyW) {
            direction.y += 1.0;
        }
        if keys.pressed(KeyCode::KeyS) {
            direction.y -= 1.0;
        }
        if keys.pressed(KeyCode::KeyA) {
            direction.x -= 1.0;
        }
        if keys.pressed(KeyCode::KeyD) {
            direction.x += 1.0;
        }

        if direction != Vec2::ZERO {
            direction = direction.normalize();
        }

        let speed = 200.0;

        transform.translation += direction.extend(0.0) * speed * time.delta_secs();
        man.position = transform.translation.truncate();
    }
}