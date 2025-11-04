use bevy::{prelude::*, window::{WindowResolution, PrimaryWindow}};
use serde::Deserialize;
use tokio::sync::mpsc;

const WIDTH_WINDOW: u32 = 1200;
const HEIGHT_WINDOW: u32 = 600;

const PIXEL_RATIO: f32 = 1.0;
const WIDTH_MAP: f32 = 800.;
const HEIGHT_MAP: f32 = 400.;
const UI_PANEL_WIDTH: f32 = 300.;

const MIN_LONGITUDE: f32 = -180.0;
const MAX_LONGITUDE: f32 = 180.0;
const MIN_LATITUDE: f32 = -90.0;
const MAX_LATITUDE: f32 = 90.0;

fn get_weather_api_key() -> String {
    std::env::var("WEATHER_API_KEY").unwrap_or_else(|_| "YOUR_API_KEY".to_string())
}

fn main() {
    App::new()
    .add_plugins(
        DefaultPlugins
        .set(WindowPlugin {
                primary_window: Some(Window {
                    title: String::from("Weather and locations on the map"),
                    position: WindowPosition::Centered(MonitorSelection::Primary),
                    resolution: WindowResolution::new(WIDTH_WINDOW, HEIGHT_WINDOW),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .set(ImagePlugin::default_nearest())
    )
    .init_resource::<ApiChannel>()
    .add_systems(Startup, setup_app)
    .add_systems(Update, scale_map_to_window)
    .add_systems(Update, (update_man, update_coordinates_text).chain())
    .add_systems(Update, check_space_key)
    .add_systems(Update, process_api_responses)
    .run();
}

#[derive(Component)]
struct Man {
    position: Vec2,
    latitude: f32,
    longitude: f32,
}

#[derive(Component)]
struct CoordinatesText;

#[derive(Component)]
struct WeatherText;

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct GameMap;

#[derive(Debug, Deserialize)]
struct WeatherResponse {
    main: MainWeather,
    weather: Vec<WeatherDescription>,
    wind: Wind,
}

#[derive(Debug, Deserialize)]
struct MainWeather {
    temp: f64,
}

#[derive(Debug, Deserialize)]
struct WeatherDescription {
    main: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct Wind {
    speed: f64,
}

#[derive(Debug, Deserialize)]
struct GeocodeResponse {
    name: String,
    country: String,
    state: Option<String>,
}

#[derive(Debug, Clone)]
struct WeatherData {
    city: String,
    region: String,
    country: String,
    temperature: f64,
    wind_speed: f64,
    weather: String,
    description: String,
}

#[derive(Resource)]
struct ApiChannel {
    sender: mpsc::UnboundedSender<Result<WeatherData, String>>,
    receiver: mpsc::UnboundedReceiver<Result<WeatherData, String>>,
}

impl Default for ApiChannel {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel::<Result<WeatherData, String>>();
        Self { sender, receiver }
    }
}

fn setup_app(
    mut commands: Commands,
    assets_server: Res<AssetServer>,
) {
    commands.spawn(Camera2d::default());

    let ui_font = assets_server.load("fonts/NotoSans-Regular.ttf");

    commands.spawn((
        Sprite {
            image: assets_server.load("world_location_map.png"),
            custom_size: Some(Vec2::new(WIDTH_MAP, HEIGHT_MAP)),
            ..Default::default()
        },
        Transform::from_xyz(0., 0., -1.),
        GameMap,
    ));

    commands.spawn((
        Sprite {
            image: assets_server.load("man.png"),
            ..Default::default()
        },
        Transform::from_xyz(0., 0., 0.).with_scale(Vec3::splat(PIXEL_RATIO)),
        Man { 
            position: Vec2::new(0., 0.),
            latitude: 0.0,
            longitude: 0.0,
        }
    ));

    commands.spawn(
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Px(300.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(20.0)),
            ..Default::default()
        }
    ).with_children(|parent| {
        parent.spawn(
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(15.0)),
                margin: UiRect::bottom(Val::Px(10.0)),
                ..Default::default()
            }
        ).with_children(|parent| {
            parent.spawn((
                Text::new("X: 0.0\nY: 0.0\n\nlat:  0.0\nlong: 0.0"),
                TextFont {
                    font: ui_font.clone(),
                    font_size: 18.0,
                    ..default()
                },
                CoordinatesText,
            ));
        }).insert(BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.8)));

        parent.spawn(
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(15.0)),
                margin: UiRect::bottom(Val::Px(10.0)),
                ..Default::default()
            }
        ).with_children(|parent| {
            parent.spawn((
                Text::new("Press SPACE\nto fetch weather"),
                TextFont {
                    font: ui_font.clone(),
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.8, 0.8)),
                StatusText,
            ));
        }).insert(BackgroundColor(Color::srgba(0.1, 0.1, 0.15, 0.8)));

        parent.spawn(
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(15.0)),
                ..Default::default()
            }
        ).with_children(|p| {
            p.spawn((
                Text::new("Weather data will\nappear here..."),
                TextFont {
                    font: ui_font.clone(),
                    font_size: 16.0,
                    ..default()
                },
                WeatherText,
            ));
        }).insert(BackgroundColor(Color::srgba(0.1, 0.15, 0.1, 0.8)));
    });
}

async fn fetch_weather_data(lat: f32, lon: f32) -> Result<WeatherData, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    
    let (weather_result, geocode_result) = tokio::join!(
        fetch_weather(&client, lat, lon),
        fetch_geocode(&client, lat, lon)
    );
    
    let weather = weather_result?;
    let geocode = geocode_result?;

    let region = geocode.state.unwrap_or_else(|| String::from("-"));
    
    Ok(WeatherData {
        city: geocode.name,
        region,
        country: geocode.country,
        temperature: weather.main.temp - 273.15,
        wind_speed: weather.wind.speed,
        weather: weather.weather[0].main.clone(),
        description: weather.weather[0].description.clone(),
    })
}

async fn fetch_weather(client: &reqwest::Client, lat: f32, lon: f32) -> Result<WeatherResponse, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!(
        "https://api.openweathermap.org/data/2.5/weather?lat={}&lon={}&appid={}",
        lat, lon, get_weather_api_key()
    );
    
    let response = client.get(&url).send().await?;
    let weather = response.json::<WeatherResponse>().await?;
    Ok(weather)
}

async fn fetch_geocode(client: &reqwest::Client, lat: f32, lon: f32) -> Result<GeocodeResponse, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!(
        "https://api.openweathermap.org/geo/1.0/reverse?lat={}&lon={}&limit=1&appid={}",
        lat, lon, get_weather_api_key()
    );
    
    let response = client.get(&url).send().await?;
    let mut geocode_list = response.json::<Vec<GeocodeResponse>>().await?;
    
    geocode_list.pop().ok_or_else(|| "No location found".into())
}

fn check_space_key(
    keys: Res<ButtonInput<KeyCode>>,
    man_query: Query<&Man>,
    api_channel: Res<ApiChannel>,
    mut status_query: Query<&mut Text, With<StatusText>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        if let Ok(man) = man_query.single() {
            let lat = man.latitude;
            let lon = man.longitude;
            let sender = api_channel.sender.clone();

            if let Ok(mut text) = status_query.single_mut() {
                **text = "Loading...".to_string();
            }

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        println!("Failed to build runtime: {}", e);
                        return;
                    }
                };

                match rt.block_on(fetch_weather_data(lat, lon)) {
                    Ok(data) => {
                        println!("Weather data received for {}, {}", data.city, data.country);
                        let _ = sender.send(Ok(data));
                    }
                    Err(e) => {
                        println!("Error: {}", e);
                        let _ = sender.send(Err(e.to_string()));
                    }
                }
            });
            
            println!("Fetching weather for ({:.4}, {:.4})", lat, lon);
        }
    }
}

fn process_api_responses(
    mut api_channel: ResMut<ApiChannel>,
    mut weather_query: Query<&mut Text, (With<WeatherText>, Without<StatusText>)>,
    mut status_query: Query<&mut Text, With<StatusText>>,
) {
    while let Ok(result) = api_channel.receiver.try_recv() {
        match result {
            Ok(data) => {
                if let Ok(mut weather_text) = weather_query.single_mut() {
                    **weather_text = format!(
                        "{},\n{},\n{}\n\n{}\n{}\n{:.1}\n{:.1} m/s",
                        data.country,
                        data.region,
                        data.city,
                        data.weather,
                        data.description,
                        data.temperature,
                        data.wind_speed,
                    );
                }

                if let Ok(mut status_text) = status_query.single_mut() {
                    **status_text = "Loaded!\nPress SPACE".to_string();
                }
            }
            Err(err_msg) => {
                if let Ok(mut status_text) = status_query.single_mut() {
                    **status_text = format!("Error: {}\nPress SPACE", err_msg);
                }

                if let Ok(mut weather_text) = weather_query.single_mut() {
                    **weather_text = "Weather data will\nappear here...".to_string();
                }
            }
        }
    }
}

fn update_man(
    mut man_query: Query<(&mut Transform, &mut Man)>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>
) {
    let Ok((mut transform, mut man)) = man_query.single_mut() else {
        return;
    };
    let Ok(window) = window_query.single() else {
        return;
    };
    
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
    
    let available_width = window.width() - UI_PANEL_WIDTH;
    let available_height = window.height();
    let scale_x = available_width / WIDTH_MAP;
    let scale_y = available_height / HEIGHT_MAP;
    let scale = scale_x.min(scale_y) * 0.9;
    
    let scaled_width = WIDTH_MAP * scale;
    let scaled_height = HEIGHT_MAP * scale;
    let offset_x = -(UI_PANEL_WIDTH / 2.0);

    transform.translation.x = transform.translation.x.clamp(
        offset_x - scaled_width / 2.,
        offset_x + scaled_width / 2.
    );
    transform.translation.y = transform.translation.y.clamp(
        -scaled_height / 2.,
        scaled_height / 2.
    );
    
    let map_x = (transform.translation.x - offset_x) / scale + WIDTH_MAP / 2.;
    let map_y = transform.translation.y / scale + HEIGHT_MAP / 2.;

    let longitude = MIN_LONGITUDE + (map_x / WIDTH_MAP) * (MAX_LONGITUDE - MIN_LONGITUDE);
    let latitude = MIN_LATITUDE + (map_y / HEIGHT_MAP) * (MAX_LATITUDE - MIN_LATITUDE);
    
    man.position = Vec2::new(map_x, map_y);
    man.longitude = longitude;
    man.latitude = latitude;
}

fn update_coordinates_text(
    man_query: Query<&Man>,
    mut text_query: Query<&mut Text, With<CoordinatesText>>
) {
    if let Ok(man) = man_query.single() {
        if let Ok(mut text) = text_query.single_mut() {
            **text = format!(
                "X: {:.3}\nY: {:.3}\n\nlat:  {:.6}\nlong: {:.6}",
                man.position.x,
                man.position.y,
                man.latitude,
                man.longitude
            );
        }
    }
}

fn scale_map_to_window(
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut map_query: Query<(&mut Transform, &mut Sprite), With<GameMap>>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Ok((mut map_transform, mut map_sprite)) = map_query.single_mut() else {
        return;
    };
    
    let available_width = window.width() - UI_PANEL_WIDTH;
    let available_height = window.height();
    
    let scale_x = available_width / WIDTH_MAP;
    let scale_y = available_height / HEIGHT_MAP;
    let scale = scale_x.min(scale_y) * 0.9;
    
    map_sprite.custom_size = Some(Vec2::new(WIDTH_MAP * scale, HEIGHT_MAP * scale));

    let offset_x = -(UI_PANEL_WIDTH / 2.0);
    map_transform.translation.x = offset_x;
}