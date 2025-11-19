// === IMPORTS ===
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
use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::{AnyIOPin, OutputPin, PinDriver},
    peripherals::Peripherals,
    prelude::*,
    spi::{config::Config, SpiDeviceDriver, SpiDriver, SpiDriverConfig},
};
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
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
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

// Import local modules (assumed to be in separate files)
mod secrets;
mod time_utils;
mod weather_icons;

use weather_icons::get_weather_icon;

// === OPENWEATHERMAP DATA STRUCTURES ===
// These structs match the JSON response from the OpenWeatherMap API.
// derive(Deserialize) allows serde to automatically map the JSON to these structs.

#[derive(Deserialize, Debug)]
struct WeatherResponse {
    weather: Vec<Weather>,
    main: Main,
    wind: Wind,
    name: String,
}

#[derive(Deserialize, Debug)]
struct Weather {
    description: String,
    icon: String, // e.g., "01d", "10n"
}

#[derive(Deserialize, Debug)]
struct Main {
    temp: f32,
    humidity: i32,
}

#[derive(Deserialize, Debug)]
struct Wind {
    speed: f32,
}

// === WEATHER SYMBOL MAPPING ===
// Maps the OpenWeatherMap icon code to a Unicode emoji as a fallback
fn get_weather_symbol(icon_code: &str) -> &'static str {
    match icon_code {
        "01d" => "☀",         // Clear sky (day)
        "01n" => "🌙",        // Clear sky (night)
        "02d" => "🌤",         // Few clouds (day)
        "02n" => "☁",         // Few clouds (night)
        "03d" | "03n" => "☁", // Scattered clouds
        "04d" | "04n" => "☁", // Broken clouds
        "09d" | "09n" => "🌧", // Shower rain
        "10d" => "🌦",         // Rain (day)
        "10n" => "🌧",         // Rain (night)
        "11d" | "11n" => "⛈", // Thunderstorm
        "13d" | "13n" => "❄", // Snow
        "50d" | "50n" => "🌫", // Mist
        _ => "❓",            // Unknown
    }
}

// === WEATHER FETCH FUNCTION ===
// Performs an HTTPS GET request to the API
fn get_weather(api_key: &str, city: &str) -> anyhow::Result<WeatherResponse> {
    let url = format!(
        "https://api.openweathermap.org/data/2.5/weather?q={}&appid={}&units=metric&lang=en",
        city, api_key
    );

    // Configure HTTP connection
    // We must attach the CRT bundle to allow SSL/TLS verification (HTTPS)
    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_sys::esp_crt_bundle_attach),
        timeout: Some(core::time::Duration::from_secs(30)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);

    // Send GET request
    let request = client.get(&url)?;
    let mut response = request.submit()?;

    let status = response.status();
    info!("Weather API response status: {}", status);

    // Read response into a buffer
    let mut body_buf = vec![0u8; 4096];
    let bytes_read = response.read(&mut body_buf)?;

    // Convert bytes to string
    let body_str = std::str::from_utf8(&body_buf[..bytes_read])?;

    // Parse JSON
    let weather: WeatherResponse = serde_json::from_str(body_str)?;
    Ok(weather)
}

// === CUSTOM ERROR TYPE ===
// Boilerplate for embedded-hal 1.0 compatibility
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

// === SPI WRAPPER ===
// Wraps ESP-IDF SPI driver to implement embedded-hal traits for the display driver
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
    pin: PinDriver<'a, esp_idf_svc::hal::gpio::AnyOutputPin, esp_idf_svc::hal::gpio::Output>,
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
    // Initialize system
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== Starting WiFi + OpenWeather + Clock ===");

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

    // ==================== SNTP SETUP ====================
    // Initialize Simple Network Time Protocol to fetch time
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

    // Reset display
    rst.set_low()?;
    FreeRtos::delay_ms(50);
    rst.set_high()?;
    FreeRtos::delay_ms(200);

    // Initialize SPI
    let spi_config = Config::new().baudrate(26.MHz().into());
    let spi_driver = SpiDriver::new(
        peripherals.spi2,
        sclk,
        mosi,
        None::<AnyIOPin>,
        &SpiDriverConfig::new(),
    )?;
    let spi_device = SpiDeviceDriver::new(spi_driver, Some(cs), &spi_config)?;

    // Create Wrappers
    let spi_wrapper = SpiWrapper { spi: spi_device };
    let dc_wrapper = DcPinWrapper {
        pin: PinDriver::output(dc.downgrade_output())?,
    };

    // Buffer allocation in static memory
    static mut DISPLAY_BUFFER: [u8; 240 * 10 * 2] = [0u8; 240 * 10 * 2];

    // Create Display Interface
    let di = unsafe {
        mipidsi::interface::SpiInterface::new(
            spi_wrapper,
            dc_wrapper,
            &mut *addr_of_mut!(DISPLAY_BUFFER),
        )
    };

    // Initialize Display Driver
    let mut display = Builder::new(ST7789, di)
        .display_size(240, 320)
        .display_offset(0, 0)
        .color_order(ColorOrder::Rgb)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut FreeRtos)
        .map_err(|e| anyhow::anyhow!("Display init failed: {:?}", e))?;

    display.clear(Rgb565::BLACK).ok();

    // ==================== STYLES ====================
    // 10x20 Font for text
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .background_color(Rgb565::BLACK)
        .build();

    // 24 point font for fallback symbols
    let symbol_style = MonoTextStyle::new(&PROFONT_24_POINT, Rgb565::YELLOW);

    // ==================== MAIN LOOP ====================
    let mut last_weather_fetch = 0u64;
    let weather_interval = 15 * 60; // Update weather every 15 minutes (in seconds)

    loop {
        // Get current UTC time
        let now = SystemTime::now();
        let since_the_epoch = now.duration_since(UNIX_EPOCH)?;
        let utc_timestamp = since_the_epoch.as_secs();

        // Convert UTC to local time (handled in external time_utils module)
        let (year, month, day, hour, minute, second) =
            time_utils::utc_to_berlin(utc_timestamp as i64);

        // Check if we need to update weather
        if utc_timestamp >= last_weather_fetch + weather_interval || last_weather_fetch == 0 {
            info!("Updating Weather...");

            // Reconnect WiFi if lost
            if !wifi.is_connected()? {
                wifi.connect().ok();
                wifi.wait_netif_up().ok();
            }

            // Fetch Weather
            match get_weather(&secrets.openweather.api_key, &secrets.openweather.city) {
                Ok(weather) => {
                    display.clear(Rgb565::BLACK).ok();

                    let icon_code = &weather.weather[0].icon;

                    // Determine icon color based on weather condition
                    let icon_color = match &icon_code[..2] {
                        "01" | "02" | "11" => Rgb565::YELLOW,   // Sun/Lightning
                        "09" | "10" => Rgb565::BLUE,            // Rain
                        "13" => Rgb565::WHITE,                  // Snow
                        "03" | "04" | "50" => Rgb565::CSS_GRAY, // Clouds/Mist
                        _ => Rgb565::WHITE,
                    };

                    // Draw City Name
                    Text::new(&weather.name, Point::new(10, 60), text_style)
                        .draw(&mut display)
                        .ok();

                    // Draw Temperature
                    let temp_str = format!("{:.1}°C", weather.main.temp);
                    Text::new(&temp_str, Point::new(10, 90), text_style)
                        .draw(&mut display)
                        .ok();

                    // Draw Description
                    Text::new(
                        &weather.weather[0].description,
                        Point::new(10, 120),
                        text_style,
                    )
                    .draw(&mut display)
                    .ok();

                    // Draw Wind Speed
                    let wind_str = format!("W: {:.1}m/s", weather.wind.speed);
                    Text::new(&wind_str, Point::new(10, 150), text_style)
                        .draw(&mut display)
                        .ok();

                    // Draw Humidity
                    let hum_str = format!("H: {}%", weather.main.humidity);
                    Text::new(&hum_str, Point::new(10, 180), text_style)
                        .draw(&mut display)
                        .ok();

                    // === ICON DRAWING ===
                    // Checks if a bitmap is available in `weather_icons.rs`.
                    // If yes, it draws pixel by pixel. If no, it draws a text symbol.
                    if let Some(icon_data) = get_weather_icon(&weather.weather[0].icon) {
                        // Fix: Define explicit types to avoid casting issues
                        let icon_width: usize = 40;
                        let icon_height: usize = 40;

                        let mut pixels = Vec::with_capacity(icon_width * icon_height);

                        for y in 0..icon_height {
                            for x in 0..icon_width {
                                // Calculate bit position in the byte array
                                let byte_index = y * (icon_width / 8) + (x / 8);
                                let bit_index = 7 - (x % 8);

                                if byte_index < icon_data.len() {
                                    // Check if bit is set
                                    if (icon_data[byte_index] >> bit_index) & 1 == 1 {
                                        pixels.push(Pixel(
                                            Point::new(160 + x as i32, 70 + y as i32),
                                            icon_color,
                                        ));
                                    }
                                }
                            }
                        }
                        // Draw all accumulated pixels at once
                        display.draw_iter(pixels.iter().cloned()).ok();
                    } else {
                        // Fallback: Draw a text symbol (Emoji)
                        let symbol = get_weather_symbol(&weather.weather[0].icon);
                        Text::new(symbol, Point::new(160, 70), symbol_style)
                            .draw(&mut display)
                            .ok();
                    }

                    last_weather_fetch = utc_timestamp;
                }
                Err(e) => {
                    error!("Weather Error: {}", e);
                }
            }
        }

        // === CLOCK UPDATE ===
        // Runs every iteration (approx every second)
        let time_str = time_utils::format_time(hour, minute, second);
        let date_str = time_utils::format_date(day, month, year);
        let tz_str = time_utils::get_timezone_str(year, month, day, hour);

        // Draw Date
        Text::new(
            &format!("{} {}", date_str, tz_str),
            Point::new(10, 20),
            text_style,
        )
        .draw(&mut display)
        .ok();

        // Draw Time
        Text::new(&time_str, Point::new(10, 40), text_style)
            .draw(&mut display)
            .ok();

        // Wait 1 second
        FreeRtos::delay_ms(1000);
    }
}
