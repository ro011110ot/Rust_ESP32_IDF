// ===============================================================================
// ESP32 Weather Station with MQTT Movement Detection
// ===============================================================================
// This application runs on an ESP32 and provides:
// - Real-time clock display with timezone support (Berlin/CEST/CET)
// - Weather information from OpenWeatherMap API
// - Movement detection logging via MQTT
// - ST7789 TFT display output
// ===============================================================================

use crate::secrets::Secrets;
use core::ptr::addr_of_mut;
use embedded_graphics::{
    mono_font::{iso_8859_1::FONT_10X20, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};
use embedded_hal::digital::OutputPin as OutputPinTrait;
use embedded_hal::spi::SpiDevice;
use embedded_svc::http::client::Client;

// === HAL Imports ===
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::{AnyIOPin, OutputPin, PinDriver},
    peripherals::Peripherals,
    prelude::*,
    spi::{config::Config, SpiDeviceDriver, SpiDriver, SpiDriverConfig},
};

use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::mqtt::client::{EspMqttClient, MqttClientConfiguration};
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use log::*;
use mipidsi::{
    models::ST7789,
    options::{ColorInversion, ColorOrder},
    Builder,
};
use profont::PROFONT_24_POINT;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

mod secrets;
mod time_utils;
mod weather_icons;

use weather_icons::get_weather_icon;

// ===============================================================================
// GLOBAL SHARED DATA
// ===============================================================================

/// Thread-safe queue for storing movement detection timestamps
/// Maximum 6 events are kept in memory (oldest are removed)
static MOVEMENT_EVENTS: Mutex<Option<Arc<Mutex<VecDeque<String>>>>> = Mutex::new(None);

/// Thread-safe storage for the most recent weather data
/// Updated every 15 minutes from OpenWeatherMap API
static LAST_WEATHER_DATA: Mutex<Option<WeatherResponse>> = Mutex::new(None);

// ===============================================================================
// DATA STRUCTURES
// ===============================================================================

/// Complete weather response from OpenWeatherMap API
#[derive(Deserialize, Serialize, Debug, Clone)]
struct WeatherResponse {
    weather: Vec<Weather>,
    main: Main,
    wind: Wind,
    name: String,
}

/// Weather condition details (description and icon code)
#[derive(Deserialize, Serialize, Debug, Clone)]
struct Weather {
    description: String,
    icon: String,
}

/// Main weather parameters (temperature and humidity)
#[derive(Deserialize, Serialize, Debug, Clone)]
struct Main {
    temp: f32,
    humidity: i32,
}

/// Wind information
#[derive(Deserialize, Serialize, Debug, Clone)]
struct Wind {
    speed: f32,
}

/// Display state structure for change detection
/// Used to minimize screen flicker by only redrawing when data changes
#[derive(Clone, PartialEq)]
struct DisplayState {
    time_str: String,
    date_str: String,
    weather_temp: String,
    weather_desc: String,
    weather_icon: String,
    wind_str: String,
    hum_str: String,
    city_name: String,
    movement_events: Vec<String>,
}

impl DisplayState {
    /// Create a new empty display state
    fn new() -> Self {
        Self {
            time_str: String::new(),
            date_str: String::new(),
            weather_temp: String::new(),
            weather_desc: String::new(),
            weather_icon: String::new(),
            wind_str: String::new(),
            hum_str: String::new(),
            city_name: String::new(),
            movement_events: Vec::new(),
        }
    }
}

// ===============================================================================
// WEATHER API FUNCTIONS
// ===============================================================================

/// Fetch current weather data from OpenWeatherMap API
///
/// # Arguments
/// * `api_key` - Your OpenWeatherMap API key
/// * `city` - City name to get weather for
///
/// # Returns
/// * `Ok(WeatherResponse)` - Parsed weather data
/// * `Err` - Network or parsing error
fn get_weather(api_key: &str, city: &str) -> anyhow::Result<WeatherResponse> {
    let url = format!(
        "https://api.openweathermap.org/data/2.5/weather?q={}&appid={}&units=metric&lang=en",
        city, api_key
    );

    // Create HTTPS connection with certificate bundle
    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_sys::esp_crt_bundle_attach),
        timeout: Some(core::time::Duration::from_secs(30)),
        ..Default::default()
    })?;

    let mut client = Client::wrap(connection);
    let request = client.get(&url)?;
    let mut response = request.submit()?;

    let status = response.status();
    info!("Weather API response status: {}", status);

    // Read response body
    let mut body_buf = vec![0u8; 4096];
    let bytes_read = response.read(&mut body_buf)?;
    let body_str = std::str::from_utf8(&body_buf[..bytes_read])?;

    // Parse JSON response
    let weather: WeatherResponse = serde_json::from_str(body_str)?;
    Ok(weather)
}

/// Map OpenWeatherMap icon codes to emoji symbols
/// Used as fallback when bitmap icons are not available
fn get_weather_symbol(icon_code: &str) -> &'static str {
    match icon_code {
        "01d" => "☀",         // Clear sky day
        "01n" => "🌙",        // Clear sky night
        "02d" => "🌤",         // Few clouds day
        "02n" => "☁",         // Few clouds night
        "03d" | "03n" => "☁", // Scattered clouds
        "04d" | "04n" => "☁", // Broken clouds
        "09d" | "09n" => "🌧", // Shower rain
        "10d" => "🌦",         // Rain day
        "10n" => "🌧",         // Rain night
        "11d" | "11n" => "⛈", // Thunderstorm
        "13d" | "13n" => "❄", // Snow
        "50d" | "50n" => "🌫", // Mist
        _ => "❓",            // Unknown
    }
}

/// Determine icon color based on weather condition
fn get_weather_icon_color(icon_code: &str) -> Rgb565 {
    match &icon_code[..2] {
        "01" | "02" | "11" => Rgb565::YELLOW,   // Sun/Thunder
        "09" | "10" => Rgb565::BLUE,            // Rain
        "13" => Rgb565::WHITE,                  // Snow
        "03" | "04" | "50" => Rgb565::CSS_GRAY, // Clouds/Mist
        _ => Rgb565::WHITE,
    }
}

// ===============================================================================
// WIFI SETUP
// ===============================================================================

/// Initialize and connect to Wi-Fi
///
/// # Arguments
/// * `peripherals` - ESP32 peripherals
/// * `secrets` - Configuration containing Wi-Fi credentials
///
/// # Returns
/// * `Ok(BlockingWifi)` - Connected Wi-Fi instance
fn setup_wifi(
    modem: impl esp_idf_hal::peripheral::Peripheral<P = esp_idf_hal::modem::Modem> + 'static,
    secrets: &Secrets,
) -> anyhow::Result<BlockingWifi<EspWifi<'static>>> {
    info!("Initializing WiFi...");

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(EspWifi::new(modem, sys_loop.clone(), Some(nvs))?, sys_loop)?;

    // Configure WiFi credentials
    let wifi_config = Configuration::Client(ClientConfiguration {
        ssid: secrets.wifi.ssid.as_str().try_into().unwrap(),
        password: secrets.wifi.password.as_str().try_into().unwrap(),
        auth_method: if secrets.wifi.password.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        },
        ..Default::default()
    });

    wifi.set_configuration(&wifi_config)?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;

    info!("WiFi connected successfully!");
    Ok(wifi)
}

// ===============================================================================
// MQTT SETUP
// ===============================================================================

/// Initialize MQTT client and start listening thread
///
/// # Arguments
/// * `secrets` - MQTT broker credentials
/// * `movement_events` - Shared queue for movement timestamps
///
/// # Returns
/// * `Ok(EspMqttClient)` - MQTT client for publishing
fn setup_mqtt(
    secrets: &Secrets,
    movement_events: Arc<Mutex<VecDeque<String>>>,
) -> anyhow::Result<EspMqttClient<'static>> {
    info!("Initializing MQTT client...");

    let mut mqtt_config = MqttClientConfiguration::default();
    mqtt_config.username = Some(secrets.mqtt.mqtt_user.as_str());
    mqtt_config.password = Some(secrets.mqtt.mqtt_pw.as_str());
    mqtt_config.client_id = Some("esp32-weather-client-rust");

    let (mut client, mut connection) =
        EspMqttClient::new(secrets.mqtt.broker_url.as_str(), &mqtt_config)?;

    // Spawn MQTT event handling thread
    std::thread::Builder::new()
        .stack_size(6000)
        .spawn(move || {
            info!("MQTT event loop started");
            let mut subscribed = false;

            while let Ok(event) = connection.next() {
                use esp_idf_svc::mqtt::client::EventPayload;

                match event.payload() {
                    EventPayload::Connected(_) => {
                        info!("MQTT Connected to broker");
                        subscribed = false;
                    }
                    EventPayload::BeforeConnect => {
                        info!("MQTT connecting to broker...");
                    }
                    EventPayload::Subscribed(msg_id) => {
                        info!("MQTT subscription confirmed (ID: {})", msg_id);
                        subscribed = true;
                    }
                    EventPayload::Received {
                        id, topic, data, ..
                    } => {
                        if !subscribed {
                            continue;
                        }

                        info!(
                            "MQTT message received on '{}' (ID: {})",
                            topic.unwrap_or("unknown"),
                            id
                        );

                        if !data.is_empty() {
                            if let Ok(received_data) = std::str::from_utf8(data) {
                                info!("MQTT data: {:?}", received_data);

                                // Handle movement detection message
                                if let Some(t) = topic {
                                    if t == "Bewegung" && received_data == "1" {
                                        handle_movement_event(&movement_events);
                                    }
                                }
                            }
                        }
                    }
                    EventPayload::Disconnected => {
                        info!("MQTT disconnected from broker");
                        subscribed = false;
                    }
                    EventPayload::Error(e) => {
                        error!("MQTT error: {:?}", e);
                    }
                    _ => {}
                }
            }
            info!("MQTT event loop ended");
            Ok::<(), anyhow::Error>(())
        })?;

    // Wait for MQTT connection to establish
    info!("Waiting for MQTT connection...");
    FreeRtos::delay_ms(2000);

    // Subscribe to movement detection topic
    let movement_topic = "Bewegung";
    match client.subscribe(movement_topic, embedded_svc::mqtt::client::QoS::AtLeastOnce) {
        Ok(_) => info!("Subscribed to topic: {}", movement_topic),
        Err(e) => error!("Failed to subscribe: {:?}", e),
    }

    Ok(client)
}

/// Handle a movement detection event
/// Converts current time to Berlin timezone and adds to event queue
fn handle_movement_event(movement_events: &Arc<Mutex<VecDeque<String>>>) {
    let now = SystemTime::now();
    if let Ok(since_the_epoch) = now.duration_since(UNIX_EPOCH) {
        let utc_timestamp = since_the_epoch.as_secs();
        let (_year, _month, _day, hour, minute, second) =
            time_utils::utc_to_berlin(utc_timestamp as i64);
        let formatted_time = time_utils::format_time(hour, minute, second);

        // Add to queue (max 6 events, FIFO)
        let mut events = movement_events.lock().unwrap();
        events.push_front(formatted_time.clone());
        if events.len() > 6 {
            events.pop_back();
        }
        info!("Movement detected at: {}", formatted_time);
    }
}

// ===============================================================================
// DISPLAY SETUP
// ===============================================================================

/// Custom error type for SPI and GPIO operations
#[derive(Debug)]
struct CustomError;

impl embedded_hal::spi::Error for CustomError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

impl embedded_hal::digital::Error for CustomError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

/// Wrapper for ESP-IDF SPI driver to work with embedded-hal traits
struct SpiWrapper<'a> {
    spi: SpiDeviceDriver<'a, SpiDriver<'a>>,
}

impl embedded_hal::spi::ErrorType for SpiWrapper<'_> {
    type Error = CustomError;
}

impl SpiDevice for SpiWrapper<'_> {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                embedded_hal::spi::Operation::Write(data) => {
                    if !data.is_empty() {
                        self.spi.write(data).map_err(|_| CustomError)?;
                    }
                }
                embedded_hal::spi::Operation::Transfer(read, write) => {
                    if !write.is_empty() {
                        self.spi.transfer(read, write).map_err(|_| CustomError)?;
                    }
                }
                embedded_hal::spi::Operation::TransferInPlace(data) => {
                    if !data.is_empty() {
                        let temp = data.to_vec();
                        self.spi.transfer(data, &temp).map_err(|_| CustomError)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Wrapper for Data/Command pin
struct DcPinWrapper<'a> {
    pin: PinDriver<'a, esp_idf_hal::gpio::AnyOutputPin, esp_idf_hal::gpio::Output>,
}

impl embedded_hal::digital::ErrorType for DcPinWrapper<'_> {
    type Error = CustomError;
}

impl OutputPinTrait for DcPinWrapper<'_> {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.pin.set_low().map_err(|_| CustomError)
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.pin.set_high().map_err(|_| CustomError)
    }
}

// ===============================================================================
// DISPLAY RENDERING
// ===============================================================================

/// Render all display content
/// Only redraws if state has changed to minimize flicker
fn render_display(
    display: &mut impl DrawTarget<Color = Rgb565>,
    current_state: &DisplayState,
    text_style: &MonoTextStyle<Rgb565>,
    symbol_style: &MonoTextStyle<Rgb565>,
) {
    // Clear entire display
    display.clear(Rgb565::BLACK).ok();

    // === Render Date and Time ===
    Text::new(&current_state.date_str, Point::new(10, 20), *text_style)
        .draw(display)
        .ok();

    Text::new(&current_state.time_str, Point::new(10, 40), *text_style)
        .draw(display)
        .ok();

    // === Render Weather Data ===
    if !current_state.city_name.is_empty() {
        // City name
        Text::new(&current_state.city_name, Point::new(10, 60), *text_style)
            .draw(display)
            .ok();

        // Temperature
        Text::new(&current_state.weather_temp, Point::new(10, 90), *text_style)
            .draw(display)
            .ok();

        // Description
        Text::new(
            &current_state.weather_desc,
            Point::new(10, 120),
            *text_style,
        )
        .draw(display)
        .ok();

        // Wind speed
        Text::new(&current_state.wind_str, Point::new(10, 150), *text_style)
            .draw(display)
            .ok();

        // Humidity
        Text::new(&current_state.hum_str, Point::new(10, 180), *text_style)
            .draw(display)
            .ok();

        // Weather icon
        render_weather_icon(display, &current_state.weather_icon, symbol_style);
    }

    // === Render Movement Events ===
    render_movement_events(display, &current_state.movement_events, text_style);
}

/// Render weather icon (bitmap or emoji fallback)
fn render_weather_icon(
    display: &mut impl DrawTarget<Color = Rgb565>,
    icon_code: &str,
    symbol_style: &MonoTextStyle<Rgb565>,
) {
    let icon_color = get_weather_icon_color(icon_code);

    // Try to render bitmap icon
    if let Some(icon_data) = get_weather_icon(icon_code) {
        let icon_width: usize = 40;
        let icon_height: usize = 40;
        let mut pixels = Vec::with_capacity(icon_width * icon_height);

        for y in 0..icon_height {
            for x in 0..icon_width {
                let byte_index = y * (icon_width / 8) + (x / 8);
                let bit_index = 7 - (x % 8);

                if byte_index < icon_data.len() {
                    if (icon_data[byte_index] >> bit_index) & 1 == 1 {
                        pixels.push(Pixel(Point::new(160 + x as i32, 70 + y as i32), icon_color));
                    }
                }
            }
        }
        display.draw_iter(pixels.iter().cloned()).ok();
    } else {
        // Fallback to emoji symbol
        let symbol = get_weather_symbol(icon_code);
        Text::new(symbol, Point::new(160, 70), *symbol_style)
            .draw(display)
            .ok();
    }
}

/// Render movement detection events in two columns
fn render_movement_events(
    display: &mut impl DrawTarget<Color = Rgb565>,
    events: &[String],
    text_style: &MonoTextStyle<Rgb565>,
) {
    let mut y_offset = 220;

    for (i, event) in events.iter().enumerate() {
        let x_pos = if i % 2 == 0 { 10 } else { 120 };
        Text::new(event, Point::new(x_pos, y_offset), *text_style)
            .draw(display)
            .ok();

        // Move to next row after every two events
        if i % 2 != 0 {
            y_offset += 20;
        }
    }
}

// ===============================================================================
// MAIN PROGRAM
// ===============================================================================

fn main() -> anyhow::Result<()> {
    // Initialize ESP-IDF services
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== ESP32 Weather Station Starting ===");

    // Load configuration from secrets.toml
    let secrets = Secrets::load()?;
    let peripherals = Peripherals::take()?;

    // === Initialize WiFi ===
    let mut wifi = setup_wifi(peripherals.modem, &secrets)?;

    // === Initialize Movement Events Queue ===
    *MOVEMENT_EVENTS.lock().unwrap() = Some(Arc::new(Mutex::new(VecDeque::new())));
    info!("Movement events queue initialized");

    // === Initialize MQTT ===
    let movement_events_arc = MOVEMENT_EVENTS
        .lock()
        .unwrap()
        .as_ref()
        .expect("MOVEMENT_EVENTS should be initialized")
        .clone();

    let mut mqtt_client = setup_mqtt(&secrets, movement_events_arc)?;

    // === Initialize SNTP (Network Time Protocol) ===
    let sntp = EspSntp::new_default()?;
    info!("Waiting for time synchronization...");
    while sntp.get_sync_status() != SyncStatus::Completed {
        FreeRtos::delay_ms(100);
    }
    info!("Time synchronized!");

    // === Initialize Display ===
    info!("Initializing display...");

    // Pin assignments
    let sclk = peripherals.pins.gpio18;
    let mosi = peripherals.pins.gpio23;
    let cs = peripherals.pins.gpio15;
    let dc = peripherals.pins.gpio21;
    let mut rst = PinDriver::output(peripherals.pins.gpio22)?;

    // Hardware reset sequence
    rst.set_low()?;
    FreeRtos::delay_ms(50);
    rst.set_high()?;
    FreeRtos::delay_ms(200);

    // Configure SPI bus
    let spi_config = Config::new().baudrate(26.MHz().into());
    let spi_driver = SpiDriver::new(
        peripherals.spi2,
        sclk,
        mosi,
        None::<AnyIOPin>,
        &SpiDriverConfig::new(),
    )?;
    let spi_device = SpiDeviceDriver::new(spi_driver, Some(cs), &spi_config)?;
    let spi_wrapper = SpiWrapper { spi: spi_device };
    let dc_wrapper = DcPinWrapper {
        pin: PinDriver::output(dc.downgrade_output())?,
    };

    // Initialize display driver
    static mut DISPLAY_BUFFER: [u8; 240 * 10 * 2] = [0u8; 240 * 10 * 2];
    let di = unsafe {
        mipidsi::interface::SpiInterface::new(
            spi_wrapper,
            dc_wrapper,
            &mut *addr_of_mut!(DISPLAY_BUFFER),
        )
    };

    let mut display = Builder::new(ST7789, di)
        .display_size(240, 320)
        .display_offset(0, 0)
        .color_order(ColorOrder::Rgb)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut FreeRtos)
        .map_err(|e| anyhow::anyhow!("Display initialization failed: {:?}", e))?;

    display.clear(Rgb565::BLACK).ok();
    info!("Display initialized successfully");

    // === Define Text Styles ===
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .background_color(Rgb565::BLACK)
        .build();

    let symbol_style = MonoTextStyle::new(&PROFONT_24_POINT, Rgb565::YELLOW);

    // === Main Loop ===
    info!("Entering main loop");

    let mut last_weather_fetch = 0u64;
    let weather_interval = 15 * 60; // 15 minutes in seconds
    let mut previous_state = DisplayState::new();
    let mut force_redraw = true; // Force initial render

    loop {
        // Get current timestamp
        let now = SystemTime::now();
        let since_the_epoch = now.duration_since(UNIX_EPOCH)?;
        let utc_timestamp = since_the_epoch.as_secs();

        // Convert UTC to Berlin time
        let (year, month, day, hour, minute, second) =
            time_utils::utc_to_berlin(utc_timestamp as i64);

        // === Weather Update Logic ===
        if utc_timestamp >= last_weather_fetch + weather_interval || last_weather_fetch == 0 {
            info!("Fetching weather update...");

            // Ensure Wi-Fi is connected
            if !wifi.is_connected()? {
                info!("WiFi disconnected, reconnecting...");
                wifi.connect()?;
                wifi.wait_netif_up()?;
            }

            // Fetch weather data
            match get_weather(&secrets.openweather.api_key, &secrets.openweather.city) {
                Ok(weather) => {
                    info!(
                        "Weather data received: {} - {}°C",
                        weather.name, weather.main.temp
                    );

                    // Store weather data globally
                    *LAST_WEATHER_DATA.lock().unwrap() = Some(weather);

                    // Publish weather data via MQTT
                    if let Ok(payload) =
                        serde_json::to_string(&LAST_WEATHER_DATA.lock().unwrap().as_ref().unwrap())
                    {
                        let topic = format!("weather/{}", secrets.openweather.city);
                        match mqtt_client.publish(
                            topic.as_str(),
                            embedded_svc::mqtt::client::QoS::AtLeastOnce,
                            false,
                            payload.as_bytes(),
                        ) {
                            Ok(_) => info!("Weather data published to MQTT: {}", topic),
                            Err(e) => error!("MQTT publish error: {:?}", e),
                        }
                    }

                    last_weather_fetch = utc_timestamp;
                    force_redraw = true; // Trigger display update
                }
                Err(e) => {
                    error!("Weather fetch error: {}", e);
                }
            }
        }

        // === Build Current Display State ===
        let mut current_state = DisplayState::new();

        // Time and date
        current_state.time_str = time_utils::format_time(hour, minute, second);
        current_state.date_str = format!(
            "{} {}",
            time_utils::format_date(day, month, year),
            time_utils::get_timezone_str(year, month, day, hour)
        );

        // Weather data
        if let Some(weather) = LAST_WEATHER_DATA.lock().unwrap().as_ref() {
            current_state.city_name = weather.name.clone();
            current_state.weather_temp = format!("{:.1}°C", weather.main.temp);
            current_state.weather_desc = weather.weather[0].description.clone();
            current_state.weather_icon = weather.weather[0].icon.clone();
            current_state.wind_str = format!("W: {:.1}m/s", weather.wind.speed);
            current_state.hum_str = format!("H: {}%", weather.main.humidity);
        }

        // Movement events
        let movement_events_guard = MOVEMENT_EVENTS.lock().unwrap();
        if let Some(events_arc) = movement_events_guard.as_ref() {
            let events = events_arc.lock().unwrap();
            current_state.movement_events = events.iter().cloned().collect();
        }

        // === Render Display (only if changed) ===
        let state_changed = current_state != previous_state;

        if state_changed || force_redraw {
            render_display(&mut display, &current_state, &text_style, &symbol_style);
            previous_state = current_state;
            force_redraw = false;
        }

        // Wait for next update cycle
        FreeRtos::delay_ms(1000);
    }
}
