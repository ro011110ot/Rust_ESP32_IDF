// === IMPORTS ===
// Core functions for low-level pointer operations
use core::ptr::addr_of_mut;

// Embedded Graphics - Library for drawing on displays
use embedded_graphics::{
    pixelcolor::Rgb565, // 16-bit color format (5 bits Red, 6 bits Green, 5 bits Blue)
    prelude::*,         // Common traits for drawing operations
    primitives::{PrimitiveStyle, Rectangle}, // Basic shapes like rectangles
};

// Embedded HAL - Hardware Abstraction Layer Traits
use embedded_hal::digital::OutputPin as OutputPinTrait;
// Trait for digital output pins
use embedded_hal::spi::SpiDevice;
// Trait for SPI devices

// ESP-IDF Service Library - Wrapper for the ESP-IDF Framework
use esp_idf_svc::hal::{
    delay::FreeRtos,                        // FreeRTOS delay functions
    gpio::{AnyIOPin, OutputPin, PinDriver}, // GPIO pin management
    peripherals::Peripherals,               // Access to hardware peripherals
    prelude::*,                             // Frequently used traits
    spi::{config::Config, SpiDeviceDriver, SpiDriver, SpiDriverConfig}, // SPI drivers
};
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};

// Logging
use log::*;

// Import secrets module (assumes a secrets.rs file exists)
mod secrets;
// Display driver for ST7789 TFT displays
use mipidsi::{
    models::ST7789,                        // ST7789 Display Model
    options::{ColorInversion, ColorOrder}, // Display configuration options
    Builder,                               // Builder pattern for display initialization
};
use secrets::Secrets;

// === CUSTOM ERROR TYPE ===
// Custom error type that implements embedded-hal 1.0 Error traits.
// This is necessary because ESP-IDF's native EspError does not directly implement
// the generic traits required by the display driver (mipidsi).

#[derive(Debug)]
struct CustomError;

// Implementation of the SPI Error trait for our CustomError
impl embedded_hal::spi::Error for CustomError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        // Always returns "Other" - sufficient for simple error handling here
        embedded_hal::spi::ErrorKind::Other
    }
}

// Implementation of the Digital Error trait for our CustomError
impl embedded_hal::digital::Error for CustomError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        // Always returns "Other"
        embedded_hal::digital::ErrorKind::Other
    }
}

// === SPI WRAPPER ===
// Wrapper around ESP-IDF's SpiDeviceDriver to implement embedded-hal 1.0 traits.
// Required because mipidsi expects embedded-hal 1.0, but ESP-IDF has its own API style.
struct SpiWrapper<'a> {
    spi: SpiDeviceDriver<'a, SpiDriver<'a>>, // The actual ESP-IDF SPI driver
}

// Defines the Error type for this SPI Wrapper
impl embedded_hal::spi::ErrorType for SpiWrapper<'_> {
    type Error = CustomError;
}

// Implements the SpiDevice trait - the main interface for SPI communication
impl SpiDevice for SpiWrapper<'_> {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        // Executes an SPI transaction - can contain multiple operations
        for op in operations {
            match op {
                // Write operation: Sends data via SPI
                embedded_hal::spi::Operation::Write(data) => {
                    self.spi.write(data).map_err(|_| CustomError)?;
                }
                // Transfer operation: Sends and receives simultaneously
                embedded_hal::spi::Operation::Transfer(read, write) => {
                    self.spi.transfer(read, write).map_err(|_| CustomError)?;
                }
                // Transfer-In-Place: Uses the same buffer for sending and receiving
                embedded_hal::spi::Operation::TransferInPlace(data) => {
                    let temp = data.to_vec(); // Create a temporary copy for the ESP-IDF API
                    self.spi.transfer(data, &temp).map_err(|_| CustomError)?;
                }
                _ => {} // Ignore other operations
            }
        }
        Ok(())
    }
}

// === DC PIN WRAPPER ===
// Wrapper for the Data/Command pin of the display.
// This pin signals to the display controller whether the incoming bytes are data or commands.
struct DcPinWrapper<'a> {
    pin: PinDriver<'a, esp_idf_svc::hal::gpio::AnyOutputPin, esp_idf_svc::hal::gpio::Output>,
}

// Defines the Error type for the DC pin
impl embedded_hal::digital::ErrorType for DcPinWrapper<'_> {
    type Error = CustomError;
}

// Implements the OutputPin trait for digital output
impl OutputPinTrait for DcPinWrapper<'_> {
    // Sets the pin to LOW (0V) -> Command Mode
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.pin.set_low().map_err(|_| CustomError)
    }

    // Sets the pin to HIGH (3.3V) -> Data Mode
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.pin.set_high().map_err(|_| CustomError)
    }
}

// === MAIN PROGRAM ===
fn main() -> anyhow::Result<()> {
    // Link patches required for ESP-IDF functionality
    esp_idf_svc::sys::link_patches();
    // Initialize the default logger (prints to serial output)
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== Starting WiFi + ST7789 Display ===");

    // Load secrets (embedded at compile time via secrets.toml/rs)
    let secrets = Secrets::load()?;

    info!("Loading configuration...");
    info!("WiFi SSID: {}", secrets.wifi.ssid);

    let peripherals = Peripherals::take()?;

    // === WiFi Setup ===
    info!("Starting WiFi...");
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    // WiFi configuration from secrets.toml
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

    // Wait until the network interface is up and has an IP
    wifi.wait_netif_up()?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    info!("IP Address: {:?}", ip_info.ip);

    // ==================== DISPLAY SETUP ====================
    info!("Setting up display...");

    // === SPI Pin Configuration ===
    // SPI (Serial Peripheral Interface) is used for display communication
    let sclk = peripherals.pins.gpio18; // SPI Clock (SCL on display)
    let mosi = peripherals.pins.gpio23; // Master Out Slave In (SDA on display)
    let cs = peripherals.pins.gpio15; // Chip Select (activates the display)

    // === Control Pins ===
    let dc = peripherals.pins.gpio21; // Data/Command Pin (distinguishes data from commands)
    let mut rst = PinDriver::output(peripherals.pins.gpio22)?; // Reset Pin

    info!("Pins configured");

    // === Hardware Reset of the Display ===
    // Reset Sequence: LOW -> Wait -> HIGH -> Wait
    rst.set_low()?; // Activate reset (Display OFF)
    FreeRtos::delay_ms(50); // Wait 50ms
    rst.set_high()?; // Deactivate reset (Display starts)
    FreeRtos::delay_ms(200); // Wait 200ms for display to be ready

    // === SPI Bus Configuration ===
    // 26 MHz clock frequency for fast data transmission
    let spi_config = Config::new().baudrate(26.MHz().into());

    // Create the SPI driver with configured pins
    // None::<AnyIOPin> means: no MISO (Master In Slave Out), as the display does not send data back
    let spi_driver = SpiDriver::new(
        peripherals.spi2, // Use SPI2 hardware unit
        sclk,             // Clock Pin
        mosi,             // Data Output
        None::<AnyIOPin>, // No Data Input (Display is write-only)
        &SpiDriverConfig::new(),
    )?;

    // Create an SPI Device with Chip-Select Pin
    let spi_device = SpiDeviceDriver::new(spi_driver, Some(cs), &spi_config)?;

    // === Create Wrapper Instances ===
    // These wrappers adapt ESP-IDF's API to match embedded-hal 1.0 expectations
    let spi_wrapper = SpiWrapper { spi: spi_device };
    let dc_wrapper = DcPinWrapper {
        pin: PinDriver::output(dc.downgrade_output())?, // Configure DC pin as output
    };

    // === Display Buffer ===
    // Static buffer for display operations.
    // Size calculation: 240 pixels width * 10 lines * 2 bytes/pixel (RGB565) = 4800 Bytes.
    // Defined as `static mut` to prevent it from being allocated on the stack,
    // which avoids stack overflow on embedded devices with limited stack memory.
    static mut DISPLAY_BUFFER: [u8; 240 * 10 * 2] = [0u8; 240 * 10 * 2];

    // === Create Display Interface ===
    // `unsafe` block is required because we are accessing `static mut`.
    // `addr_of_mut!` creates a raw pointer, which is then dereferenced to a reference.
    let di = unsafe {
        mipidsi::interface::SpiInterface::new(
            spi_wrapper,                        // SPI Communication
            dc_wrapper,                         // Data/Command Pin
            &mut *addr_of_mut!(DISPLAY_BUFFER), // Buffer for batch operations
        )
    };

    // === Initialize Display ===
    let mut display = Builder::new(ST7789, di) // Use ST7789 Controller
        .display_size(240, 320) // Display Resolution: 240x320 Pixels
        .display_offset(0, 0) // No offset (starts at 0,0)
        .color_order(ColorOrder::Rgb) // RGB Color Order
        .invert_colors(ColorInversion::Inverted) // Invert colors (often needed for IPS TFTs)
        .init(&mut FreeRtos) // Initialize using FreeRTOS Delay
        .map_err(|e| anyhow::anyhow!("Display init failed: {:?}", e))?;

    info!("Display initialized!");

    // === Initial Display Content ===
    // Fill the display with black
    display.clear(Rgb565::BLACK).ok();

    // Draw a green rectangle to indicate "Wi-Fi Connected"
    Rectangle::new(Point::new(0, 0), Size::new(240, 60))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::GREEN))
        .draw(&mut display)
        .ok();

    info!("=== System Ready! ===");

    // ==================== MAIN LOOP ====================
    // Array of colors to cycle through
    let colors = [
        ("RED", Rgb565::RED),
        ("GREEN", Rgb565::GREEN),
        ("BLUE", Rgb565::BLUE),
        ("YELLOW", Rgb565::YELLOW),
    ];

    let mut idx = 0; // Index for the color array

    loop {
        // === Monitor WiFi Connection ===
        // If Wi-Fi is disconnected, attempt to reconnect
        if !wifi.is_connected()? {
            warn!("WiFi disconnected, reconnecting...");
            wifi.connect()?;
        }

        // Get current color from array
        let (name, color) = colors[idx];
        info!("Displaying: {} - WiFi: Connected", name);

        // === Update Display ===
        // Fill the entire display with the current color
        display.clear(color).ok();

        // Wait for 2 seconds
        FreeRtos::delay_ms(2000);

        // Move to the next color index (with wrap-around)
        idx = (idx + 1) % colors.len();
    }
}
