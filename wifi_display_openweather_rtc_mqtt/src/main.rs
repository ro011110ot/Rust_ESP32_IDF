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

// Shared data structure for movement events
static MOVEMENT_EVENTS: Mutex<Option<Arc<Mutex<VecDeque<String>>>>> = Mutex::new(None);

// Shared data structure for last fetched weather data
static LAST_WEATHER_DATA: Mutex<Option<WeatherResponse>> = Mutex::new(None);

// === OPENWEATHERMAP DATA STRUCTURES ===

#[derive(Deserialize, Serialize, Debug)]
struct WeatherResponse {
    weather: Vec<Weather>,
    main: Main,
    wind: Wind,
    name: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct Weather {
    description: String,
    icon: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct Main {
    temp: f32,
    humidity: i32,
}

#[derive(Deserialize, Serialize, Debug)]
struct Wind {
    speed: f32,
}

// === WEATHER SYMBOL MAPPING ===

fn get_weather_symbol(icon_code: &str) -> &'static str {
    match icon_code {
        "01d" => "☀",
        "01n" => "🌙",
        "02d" => "🌤",
        "02n" => "☁",
        "03d" | "03n" => "☁",
        "04d" | "04n" => "☁",
        "09d" | "09n" => "🌧",
        "10d" => "🌦",
        "10n" => "🌧",
        "11d" | "11n" => "⛈",
        "13d" | "13n" => "❄",
        "50d" | "50n" => "🌫",
        _ => "❓",
    }
}

// === WEATHER FETCH FUNCTION ===

fn get_weather(api_key: &str, city: &str) -> anyhow::Result<WeatherResponse> {
    let url = format!(
        "https://api.openweathermap.org/data/2.5/weather?q={}&appid={}&units=metric&lang=en",
        city, api_key
    );

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

    let mut body_buf = vec![0u8; 4096];
    let bytes_read = response.read(&mut body_buf)?;

    let body_str = std::str::from_utf8(&body_buf[..bytes_read])?;

    let weather: WeatherResponse = serde_json::from_str(body_str)?;
    Ok(weather)
}

// === CUSTOM ERROR TYPE & SPI WRAPPER ===

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

// === DC PIN WRAPPER ===

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

// === MAIN PROGRAM ===
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== Starting WiFi + OpenWeather + Clock + MQTT ===");

    // Load secrets from secrets.toml
    let secrets = Secrets::load()?;
    // Take peripherals
    let peripherals = Peripherals::take()?;

    // === WiFi Setup ===
    info!("Starting WiFi...");
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    // Configure WiFi
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
    // Start WiFi and connect
    wifi.start()?;
    wifi.connect()?;
    // Wait for connection
    wifi.wait_netif_up()?;
    info!("WiFi connected!");

    // ==================== INITIALIZE MOVEMENT_EVENTS FIRST ====================
    *MOVEMENT_EVENTS.lock().unwrap() = Some(Arc::new(Mutex::new(VecDeque::new())));
    info!("Movement events queue initialized");

    // ==================== MQTT SETUP ====================
    info!("Starting MQTT client...");

    let mut mqtt_config = MqttClientConfiguration::default();
    mqtt_config.username = Some(secrets.mqtt.mqtt_user.as_str());
    mqtt_config.password = Some(secrets.mqtt.mqtt_pw.as_str());
    mqtt_config.client_id = Some("esp32-weather-client-rust");

    let (mut client, mut connection) =
        EspMqttClient::new(secrets.mqtt.broker_url.as_str(), &mqtt_config)?;

    // Clone the Arc before moving into thread
    let movement_events_arc = MOVEMENT_EVENTS
        .lock()
        .unwrap()
        .as_ref()
        .expect("MOVEMENT_EVENTS should be initialized")
        .clone();

    // MQTT thread
    std::thread::Builder::new()
        .stack_size(6000)
        .spawn(move || {
            info!("MQTT Listening Loop started");

            // Wait for first event and subscribe after connection
            let mut subscribed = false;

            while let Ok(event) = connection.next() {
                use esp_idf_svc::mqtt::client::EventPayload;

                match event.payload() {
                    EventPayload::Connected(_) => {
                        info!("MQTT Connected!");
                        subscribed = false; // Reset subscription flag on reconnect
                    }
                    EventPayload::BeforeConnect => {
                        info!("MQTT connecting...");
                    }
                    EventPayload::Subscribed(msg_id) => {
                        info!("MQTT Subscribed! Message ID: {}", msg_id);
                        subscribed = true;
                    }
                    EventPayload::Received {
                        id, topic, data, ..
                    } => {
                        if !subscribed {
                            continue;
                        }

                        info!(
                            "MQTT Message received on topic: {} (id: {})",
                            topic.unwrap_or("unknown"),
                            id
                        );
                        if !data.is_empty() {
                            let received_data = std::str::from_utf8(data)?;
                            info!("Data: {:?}", received_data);

                            if let Some(t) = topic {
                                if t == "Bewegung" && received_data == "1" {
                                    let now = SystemTime::now();
                                    let since_the_epoch = now.duration_since(UNIX_EPOCH)?;
                                    let utc_timestamp = since_the_epoch.as_secs();

                                    let (_year, _month, _day, hour, minute, second) =
                                        time_utils::utc_to_berlin(utc_timestamp as i64);
                                    let formatted_time =
                                        time_utils::format_time(hour, minute, second);

                                    let mut events = movement_events_arc.lock().unwrap();
                                    events.push_front(formatted_time);
                                    if events.len() > 6 {
                                        events.pop_back();
                                    }
                                    info!("Movement detected: {}", formatted_time);
                                }
                            }
                        }
                    }
                    EventPayload::Disconnected => {
                        info!("MQTT Disconnected!");
                        subscribed = false;
                    }
                    EventPayload::Error(e) => {
                        error!("MQTT Event Error: {:?}", e);
                    }
                    _ => {}
                }
            }
            info!("MQTT Connection closed");
            Ok::<(), anyhow::Error>(())
        })?;

    info!("MQTT thread started.");

    // Wait for MQTT connection before subscribing
    info!("Waiting for MQTT connection...");
    FreeRtos::delay_ms(2000);

    // Subscribe to movement topic
    let movement_topic = "Bewegung";
    match client.subscribe(movement_topic, embedded_svc::mqtt::client::QoS::AtLeastOnce) {
        Ok(_) => info!("Subscribe request sent for topic: {}", movement_topic),
        Err(e) => error!("Failed to subscribe: {:?}", e),
    }

    // ==================== SNTP SETUP ====================
    let sntp = EspSntp::new_default()?;
    info!("Waiting for SNTP time synchronization...");
    while sntp.get_sync_status() != SyncStatus::Completed {
        FreeRtos::delay_ms(100);
    }
    info!("Time synchronized!");

    // ==================== DISPLAY SETUP ====================
    let sclk = peripherals.pins.gpio18;
    let mosi = peripherals.pins.gpio23;
    let cs = peripherals.pins.gpio15;
    let dc = peripherals.pins.gpio21;
    let mut rst = PinDriver::output(peripherals.pins.gpio22)?;

    // Reset the display
    rst.set_low()?;
    FreeRtos::delay_ms(50);
    rst.set_high()?;
    FreeRtos::delay_ms(200);

    // Configure SPI
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

    // Initialize the display
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
        .map_err(|e| anyhow::anyhow!("Display init failed: {:?}", e))?;

    // Clear the display
    display.clear(Rgb565::BLACK).ok();

    // ==================== STYLES ====================
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .background_color(Rgb565::BLACK)
        .build();

    let symbol_style = MonoTextStyle::new(&PROFONT_24_POINT, Rgb565::YELLOW);

    // ==================== MAIN LOOP ====================
    let mut last_weather_fetch = 0u64;
    let weather_interval = 15 * 60; // 15 minutes

    loop {
        // Clear the display at the beginning of each loop iteration
        display.clear(Rgb565::BLACK).ok();

        // Get the current time
        let now = SystemTime::now();
        let since_the_epoch = now.duration_since(UNIX_EPOCH)?;
        let utc_timestamp = since_the_epoch.as_secs();

        // Convert UTC time to Berlin time
        let (year, month, day, hour, minute, second) =
            time_utils::utc_to_berlin(utc_timestamp as i64);

        // Fetch weather data every 15 minutes
        if utc_timestamp >= last_weather_fetch + weather_interval || last_weather_fetch == 0 {
            info!("Updating Weather...");

            // Reconnect to WiFi if necessary
            if !wifi.is_connected()? {
                info!("WiFi disconnected, reconnecting...");
                wifi.connect()?;
                wifi.wait_netif_up()?;
            }

            // Get weather data
            match get_weather(&secrets.openweather.api_key, &secrets.openweather.city) {
                Ok(weather) => {
                    // Store the fetched weather data
                    *LAST_WEATHER_DATA.lock().unwrap() = Some(weather);

                    // --- MQTT PUBLISH LOGIC ---
                    match serde_json::to_string(&LAST_WEATHER_DATA.lock().unwrap().as_ref().unwrap()) {
                        Ok(payload) => {
                            let topic = format!("weather/{}", secrets.openweather.city);
                            match client.publish(
                                topic.as_str(),
                                embedded_svc::mqtt::client::QoS::AtLeastOnce,
                                false,
                                payload.as_bytes(),
                            ) {
                                Ok(_) => info!("Published weather data to topic: {}", topic),
                                Err(e) => error!("MQTT Publish Error: {:?}", e),
                            }
                        }
                        Err(e) => error!("JSON Serialization Error: {:?}", e),
                    }

                    last_weather_fetch = utc_timestamp;
                }
                Err(e) => {
                    error!("Weather Error: {}", e);
                }
            }
        }

        // --- DISPLAY LOGIC (Weather, Time, Movement) ---
        // Always draw weather data if available
        if let Some(weather) = LAST_WEATHER_DATA.lock().unwrap().as_ref() {
            let icon_code = &weather.weather[0].icon;

            // Set the icon color based on the weather condition
            let icon_color = match &icon_code[..2] {
                "01" | "02" | "11" => Rgb565::YELLOW,
                "09" | "10" => Rgb565::BLUE,
                "13" => Rgb565::WHITE,
                "03" | "04" | "50" => Rgb565::CSS_GRAY,
                _ => Rgb565::WHITE,
            };

            // Display city name
            Text::new(&weather.name, Point::new(10, 60), text_style)
                .draw(&mut display)
                .ok();

            // Display temperature
            let temp_str = format!("{:.1}°C", weather.main.temp);
            Text::new(&temp_str, Point::new(10, 90), text_style)
                .draw(&mut display)
                .ok();

            // Display weather description
            Text::new(
                &weather.weather[0].description,
                Point::new(10, 120),
                text_style,
            )
            .draw(&mut display)
            .ok();

            // Display wind speed
            let wind_str = format!("W: {:.1}m/s", weather.wind.speed);
            Text::new(&wind_str, Point::new(10, 150), text_style)
                .draw(&mut display)
                .ok();

            // Display humidity
            let hum_str = format!("H: {}%", weather.main.humidity);
            Text::new(&hum_str, Point::new(10, 180), text_style)
                .draw(&mut display)
                .ok();

            // Display weather icon
            if let Some(icon_data) = get_weather_icon(&weather.weather[0].icon) {
                let icon_width: usize = 40;
                let icon_height: usize = 40;

                let mut pixels = Vec::with_capacity(icon_width * icon_height);

                for y in 0..icon_height {
                    for x in 0..icon_width {
                        let byte_index = y * (icon_width / 8) + (x / 8);
                        let bit_index = 7 - (x % 8);

                        if byte_index < icon_data.len() {
                            if (icon_data[byte_index] >> bit_index) & 1 == 1 {
                                pixels.push(Pixel(
                                    Point::new(160 + x as i32, 70 + y as i32),
                                    icon_color,
                                ));
                            }
                        }
                    }
                }
                display.draw_iter(pixels.iter().cloned()).ok();
            } else {
                // Fallback to weather symbol if icon is not available
                let symbol = get_weather_symbol(&weather.weather[0].icon);
                Text::new(symbol, Point::new(160, 70), symbol_style)
                    .draw(&mut display)
                    .ok();
            }
        }

        // Display time and date
        let time_str = time_utils::format_time(hour, minute, second);
        let date_str = time_utils::format_date(day, month, year);
        let tz_str = time_utils::get_timezone_str(year, month, day, hour);

        Text::new(
            &format!("{} {}", date_str, tz_str),
            Point::new(10, 20),
            text_style,
        )
        .draw(&mut display)
        .ok();

        Text::new(&time_str, Point::new(10, 40), text_style)
            .draw(&mut display)
            .ok();

        // Display movement events
        let movement_events_arc = MOVEMENT_EVENTS.lock().unwrap();
        if let Some(events_arc) = movement_events_arc.as_ref() {
            let events = events_arc.lock().unwrap();
            let mut y_offset = 220; // Start y_offset at 220 for the first event

            for (i, event) in events.iter().enumerate() {
                let x_pos = if i % 2 == 0 { 10 } else { 120 };
                Text::new(event, Point::new(x_pos, y_offset), text_style)
                    .draw(&mut display)
                    .ok();
                if i % 2 != 0 {
                    y_offset += 20; // Move to next line after every two events
                }
            }
        }

        // Wait for 1 second
        FreeRtos::delay_ms(1000);
    }
}
