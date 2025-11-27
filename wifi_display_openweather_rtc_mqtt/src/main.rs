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

// === HAL Imports (Fixes PinDriver, FreeRtos, Peripherals, SpiDriver, etc.) ===
use esp_idf_hal::{
    delay::FreeRtos,                        // Fixes FreeRtos
    gpio::{AnyIOPin, OutputPin, PinDriver}, // Fixes PinDriver, AnyIOPin
    peripherals::Peripherals,               // Fixes Peripherals
    prelude::*,                             // Beinhaltet .Hz() und .MHz() Traits
    spi::{
        config::Config,  // Fixes spi::config::Config
        SpiDeviceDriver, // Fixes SpiDeviceDriver
        SpiDriver,       // Fixes SpiDriver
        SpiDriverConfig, // Fixes SpiDriverConfig
    },
};
// =======================================================

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
use std::time::{SystemTime, UNIX_EPOCH};

mod secrets;
mod time_utils;
mod weather_icons;

use weather_icons::get_weather_icon;

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
        "https://api.openweathermap.org/data/2.5/weather?q={}&appid={}&units=metric&lang=de",
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

//noinspection ALL
// === MAIN PROGRAM ===
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== Starting WiFi + OpenWeather + Clock + MQTT ===");

    let secrets = Secrets::load()?;
    let peripherals = Peripherals::take()?;

    // === WiFi Setup ===
    info!("Starting WiFi...");
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

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
    info!("WiFi connected!");

    // ==================== MQTT SETUP (Auth & Threading) ====================
    info!("Starting MQTT client...");

    // 1. MQTT Konfiguration erstellen
    let mut mqtt_config = MqttClientConfiguration::default();

    // 2. Zugangsdaten zur Konfiguration hinzufügen
    mqtt_config.username = Some(secrets.mqtt.mqtt_user.as_str());
    mqtt_config.password = Some(secrets.mqtt.mqtt_pw.as_str());
    mqtt_config.client_id = Some("esp32-weather-client-rust");

    // 3. Client und Connection trennen (Tuple Destructuring)
    let (mut client, mut connection) =
        EspMqttClient::new(secrets.mqtt.broker_url.as_str(), &mqtt_config)?;

    // 4. Die Connection muss in einem eigenen Thread laufen
    std::thread::Builder::new()
        .stack_size(6000)
        .spawn(move || {
            info!("MQTT Listening Loop started");
            while let Ok(event) = connection.next() {
                // Wichtig: verwende das Modul-Event, nicht das Trait-Event
                use esp_idf_svc::mqtt::client::EventPayload;

                match event.payload() {
                    EventPayload::Received {
                        id, topic, data, ..
                    } => {
                        info!(
                            "MQTT Message received on topic: {} (id: {})",
                            topic.unwrap_or("unknown"),
                            id
                        );
                        if !data.is_empty() {
                            info!("Data: {:?}", std::str::from_utf8(data));
                        }
                    }
                    EventPayload::Connected(_) => {
                        info!("MQTT Connected!");
                    }
                    EventPayload::Disconnected => {
                        info!("MQTT Disconnected!");
                    }
                    EventPayload::Error(e) => {
                        error!("MQTT Event Error: {:?}", e);
                    }
                    _ => {}
                }
            }
            info!("MQTT Connection closed");
        })?;

    info!("MQTT client started.");
    // ===========================================================================

    // ==================== SNTP SETUP ====================
    let sntp = EspSntp::new_default()?;
    info!("Warte auf SNTP Zeit-Synchronisierung...");
    while sntp.get_sync_status() != SyncStatus::Completed {
        FreeRtos::delay_ms(100);
    }
    info!("Zeit synchronisiert!");

    // ==================== DISPLAY SETUP (Standard Pins) ====================
    let sclk = peripherals.pins.gpio18;
    let mosi = peripherals.pins.gpio23;
    let cs = peripherals.pins.gpio15;
    let dc = peripherals.pins.gpio21;
    let mut rst = PinDriver::output(peripherals.pins.gpio22)?;

    rst.set_low()?;
    FreeRtos::delay_ms(50);
    rst.set_high()?;
    FreeRtos::delay_ms(200);

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
    let weather_interval = 15 * 60; // 15 Minuten

    loop {
        let now = SystemTime::now();
        let since_the_epoch = now.duration_since(UNIX_EPOCH)?;
        let utc_timestamp = since_the_epoch.as_secs();

        let (year, month, day, hour, minute, second) =
            time_utils::utc_to_berlin(utc_timestamp as i64);

        if utc_timestamp >= last_weather_fetch + weather_interval || last_weather_fetch == 0 {
            info!("Updating Weather...");

            if !wifi.is_connected()? {
                wifi.connect().ok();
                wifi.wait_netif_up().ok();
            }

            match get_weather(&secrets.openweather.api_key, &secrets.openweather.city) {
                Ok(weather) => {
                    // --- DISPLAY LOGIC ---
                    display.clear(Rgb565::BLACK).ok();

                    let icon_code = &weather.weather[0].icon;

                    let icon_color = match &icon_code[..2] {
                        "01" | "02" | "11" => Rgb565::YELLOW,
                        "09" | "10" => Rgb565::BLUE,
                        "13" => Rgb565::WHITE,
                        "03" | "04" | "50" => Rgb565::CSS_GRAY,
                        _ => Rgb565::WHITE,
                    };

                    Text::new(&weather.name, Point::new(10, 60), text_style)
                        .draw(&mut display)
                        .ok();

                    let temp_str = format!("{:.1}°C", weather.main.temp);
                    Text::new(&temp_str, Point::new(10, 90), text_style)
                        .draw(&mut display)
                        .ok();

                    Text::new(
                        &weather.weather[0].description,
                        Point::new(10, 120),
                        text_style,
                    )
                    .draw(&mut display)
                    .ok();

                    let wind_str = format!("W: {:.1}m/s", weather.wind.speed);
                    Text::new(&wind_str, Point::new(10, 150), text_style)
                        .draw(&mut display)
                        .ok();

                    let hum_str = format!("H: {}%", weather.main.humidity);
                    Text::new(&hum_str, Point::new(10, 180), text_style)
                        .draw(&mut display)
                        .ok();

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
                        let symbol = get_weather_symbol(&weather.weather[0].icon);
                        Text::new(symbol, Point::new(160, 70), symbol_style)
                            .draw(&mut display)
                            .ok();
                    }

                    // --- MQTT PUBLISH LOGIC ---
                    match serde_json::to_string(&weather) {
                        Ok(payload) => {
                            let topic = format!("weather/{}", secrets.openweather.city);
                            // `client` ist der EspMqttClient
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
                    // -------------------------

                    last_weather_fetch = utc_timestamp;
                }
                Err(e) => {
                    error!("Weather Error: {}", e);
                }
            }
        }

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

        FreeRtos::delay_ms(1000);
    }
}
