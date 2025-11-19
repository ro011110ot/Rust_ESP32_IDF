// === IMPORTS ===
use crate::secrets::Secrets;
use core::ptr::addr_of_mut;
use embedded_graphics::{
    mono_font::{iso_8859_1::FONT_10X20, MonoTextStyle},
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

mod secrets;
mod weather_icons; // importiert weather_icons.rs

use weather_icons::get_weather_icon;
// === OPENWEATHERMAP DATA STRUCTURES ===
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
    icon: String,
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

// === CUSTOM ERROR TYPE ===
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

// === MAIN PROGRAM ===
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== Starting WiFi + OpenWeather Display ===");

    let secrets = Secrets::load()?;
    info!("Lade Konfiguration...");
    info!("WiFi SSID: {}", secrets.wifi.ssid);
    info!("OpenWeather Stadt: {}", secrets.openweather.city);

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
    info!("WiFi started");
    wifi.connect()?;
    info!("WiFi connected!");
    wifi.wait_netif_up()?;
    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    info!("IP-Adresse: {:?}", ip_info.ip);

    // ==================== DISPLAY SETUP ====================
    info!("Setting up display...");
    let sclk = peripherals.pins.gpio18;
    let mosi = peripherals.pins.gpio23;
    let cs = peripherals.pins.gpio15;
    let dc = peripherals.pins.gpio21;
    let mut rst = PinDriver::output(peripherals.pins.gpio22)?;
    info!("Pins configured");

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

    info!("Display initialized!");
    display.clear(Rgb565::BLACK).ok();
    info!("=== System Ready! ===");

    // ==================== MAIN LOOP ====================
    let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let symbol_style = MonoTextStyle::new(&PROFONT_24_POINT, Rgb565::YELLOW);

    loop {
        // Reconnect Wi-Fi if disconnected
        if !wifi.is_connected()? {
            warn!("WiFi disconnected, reconnecting...");
            wifi.connect()?;
            wifi.wait_netif_up()?;
        }

        info!("Fetching weather data...");
        match get_weather(&secrets.openweather.api_key, &secrets.openweather.city) {
            Ok(weather) => {
                info!("Successfully fetched weather for {}", weather.name);
                display.clear(Rgb565::BLACK).ok();

                // Inside the main loop, after fetching weather data
                let icon_code = &weather.weather[0].icon;

                // Determine icon color based on type
                let icon_color = match &icon_code[..2] {
                    "01" => Rgb565::YELLOW,                // Sun / Moon
                    "02" => Rgb565::YELLOW,                // Few clouds
                    "03" | "04" => Rgb565::CSS_LIGHT_GRAY, // Clouds
                    "09" | "10" => Rgb565::BLUE,           // Rain
                    "11" => Rgb565::YELLOW,                // Thunder
                    "13" => Rgb565::WHITE,                 // Snow
                    "50" => Rgb565::CSS_GRAY,              // Fog
                    _ => Rgb565::WHITE,
                };

                // ---------------- Display City ----------------
                Text::new(&weather.name, Point::new(10, 30), text_style)
                    .draw(&mut display)
                    .ok();

                // ---------------- Display Temperature ----------------
                let temp_str = format!("{:.1}°C", weather.main.temp);
                Text::new(&temp_str, Point::new(10, 60), text_style)
                    .draw(&mut display)
                    .ok();

                // ---------------- Display Weather Description ----------------
                Text::new(
                    &weather.weather[0].description,
                    Point::new(10, 90),
                    text_style,
                )
                .draw(&mut display)
                .ok();

                // ---------------- Display Wind ----------------
                let wind_str = format!("Wind: {:.1} m/s", weather.wind.speed);
                Text::new(&wind_str, Point::new(10, 120), text_style)
                    .draw(&mut display)
                    .ok();

                // ---------------- Display Humidity ----------------
                let humidity_str = format!("Feuchte: {}%", weather.main.humidity);
                Text::new(&humidity_str, Point::new(10, 150), text_style)
                    .draw(&mut display)
                    .ok();

                // ---------------- Display Weather Icon ----------------
                if let Some(icon_data) = get_weather_icon(&weather.weather[0].icon) {
                    // Icon dimensions
                    let icon_width = 40;
                    let icon_height = 40;

                    // Prepare pixel vector for embedded-graphics
                    let mut pixels = Vec::with_capacity(icon_width * icon_height);

                    for y in 0..icon_height {
                        for x in 0..icon_width {
                            let byte_index = y * (icon_width / 8) + (x / 8);
                            let bit_index = 7 - (x % 8);
                            let pixel_on = (icon_data[byte_index] >> bit_index) & 1 == 1;

                            if pixel_on {
                                pixels.push(Pixel(
                                    Point::new(160 + x as i32, 70 + y as i32),
                                    icon_color,
                                ));
                            }
                        }
                    }

                    // Draw the icon pixels on the display
                    display.draw_iter(pixels.iter().cloned()).ok();
                } else {
                    // Fallback: Use Unicode symbol if icon not found
                    let symbol = get_weather_symbol(&weather.weather[0].icon);
                    Text::new(symbol, Point::new(160, 70), symbol_style)
                        .draw(&mut display)
                        .ok();
                }
            }
            Err(e) => {
                error!("Failed to fetch weather: {}", e);
                display.clear(Rgb565::RED).ok(); // Signal error on display
                Text::new("Error", Point::new(10, 30), text_style)
                    .draw(&mut display)
                    .ok();
            }
        }

        info!("Waiting for next update (15 minutes)...");
        FreeRtos::delay_ms(15 * 60 * 1000); // Wait 15 minutes
    }
}
