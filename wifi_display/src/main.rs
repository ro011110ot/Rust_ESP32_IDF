// === IMPORTS ===
// Core-Funktionen für Low-Level Pointer-Operationen
use core::ptr::addr_of_mut;

// Embedded Graphics - Bibliothek zum Zeichnen auf Displays
use embedded_graphics::{
    pixelcolor::Rgb565, // 16-Bit Farbformat (5 Bit Rot, 6 Bit Grün, 5 Bit Blau)
    prelude::*,         // Basis-Traits für Zeichenoperationen
    primitives::{PrimitiveStyle, Rectangle}, // Grundformen wie Rechtecke
};

// Embedded HAL - Hardware Abstraction Layer Traits
use embedded_hal::digital::OutputPin as OutputPinTrait;
// Trait für digitale Ausgangspins
use embedded_hal::spi::SpiDevice;
// Trait für SPI-Geräte

// ESP-IDF Service Bibliothek - Wrapper für ESP-IDF Framework
use esp_idf_svc::hal::{
    delay::FreeRtos,                        // FreeRTOS Delay-Funktionen
    gpio::{AnyIOPin, OutputPin, PinDriver}, // GPIO Pin-Verwaltung
    peripherals::Peripherals,               // Zugriff auf Hardware-Peripherie
    prelude::*,                             // Häufig genutzte Traits
    spi::{config::Config, SpiDeviceDriver, SpiDriver, SpiDriverConfig}, // SPI-Treiber
};
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};

// Logging
use log::*;

//use secret.toml
mod secrets;
// Display-Treiber für ST7789 TFT-Display
use mipidsi::{
    models::ST7789,                        // ST7789 Display-Modell
    options::{ColorInversion, ColorOrder}, // Display-Konfigurationsoptionen
    Builder,                               // Builder-Pattern für Display-Initialisierung
};
use secrets::Secrets;

// === CUSTOM ERROR TYPE ===
// Eigener Fehlertyp, der die embedded-hal 1.0 Error-Traits implementiert
// Notwendig, weil ESP-IDF's EspError diese Traits nicht direkt implementiert

#[derive(Debug)]
struct CustomError;

// Implementierung des SPI-Error-Traits für unseren CustomError
impl embedded_hal::spi::Error for CustomError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        // Gibt immer "Other" zurück - für einfache Fehlerbehandlung ausreichend
        embedded_hal::spi::ErrorKind::Other
    }
}

// Implementierung des Digital-Error-Traits für unseren CustomError
impl embedded_hal::digital::Error for CustomError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        // Gibt immer "Other" zurück
        embedded_hal::digital::ErrorKind::Other
    }
}

// === SPI WRAPPER ===
// Wrapper um ESP-IDF's SpiDeviceDriver, um embedded-hal 1.0 Traits zu implementieren
// Notwendig weil mipidsi embedded-hal 1.0 erwartet, ESP-IDF aber eine eigene API hat
struct SpiWrapper<'a> {
    spi: SpiDeviceDriver<'a, SpiDriver<'a>>, // Der eigentliche ESP-IDF SPI-Treiber
}

// Definiert den Error-Typ für diesen SPI-Wrapper
impl embedded_hal::spi::ErrorType for SpiWrapper<'_> {
    type Error = CustomError;
}

// Implementiert das SpiDevice-Trait - die Hauptschnittstelle für SPI-Kommunikation
impl SpiDevice for SpiWrapper<'_> {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        // Führt eine SPI-Transaktion aus - kann mehrere Operationen enthalten
        for op in operations {
            match op {
                // Schreiboperation: Sendet Daten über SPI
                embedded_hal::spi::Operation::Write(data) => {
                    self.spi.write(data).map_err(|_| CustomError)?;
                }
                // Transfer-Operation: Sendet und empfängt gleichzeitig
                embedded_hal::spi::Operation::Transfer(read, write) => {
                    self.spi.transfer(read, write).map_err(|_| CustomError)?;
                }
                // Transfer-In-Place: Nutzt denselben Buffer für Senden und Empfangen
                embedded_hal::spi::Operation::TransferInPlace(data) => {
                    let temp = data.to_vec(); // Temporäre Kopie für ESP-IDF API
                    self.spi.transfer(data, &temp).map_err(|_| CustomError)?;
                }
                _ => {} // Andere Operationen werden ignoriert
            }
        }
        Ok(())
    }
}

// === DC PIN WRAPPER ===
// Wrapper für den Data/Command Pin des Displays
// Dieser Pin signalisiert dem Display, ob Daten oder Befehle gesendet werden
struct DcPinWrapper<'a> {
    pin: PinDriver<'a, esp_idf_svc::hal::gpio::AnyOutputPin, esp_idf_svc::hal::gpio::Output>,
}

// Definiert den Error-Typ für den DC-Pin
impl embedded_hal::digital::ErrorType for DcPinWrapper<'_> {
    type Error = CustomError;
}

// Implementiert das OutputPin-Trait für digitale Ausgabe
impl OutputPinTrait for DcPinWrapper<'_> {
    // Setzt den Pin auf LOW (0V)
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.pin.set_low().map_err(|_| CustomError)
    }

    // Setzt den Pin auf HIGH (3.3V)
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.pin.set_high().map_err(|_| CustomError)
    }
}

// === HAUPTPROGRAMM ===
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=== Starting WiFi + ST7789 Display ===");

    // Secrets laden (zur Compile-Zeit eingebettet)
    let secrets = Secrets::load()?;

    info!("Lade Konfiguration...");
    info!("WiFi SSID: {}", secrets.wifi.ssid);

    let peripherals = Peripherals::take()?;

    // === WiFi Setup mit Secrets ===
    info!("Starting WiFi...");
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    // WiFi-Konfiguration aus secrets.toml
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

    // === SPI Pin-Konfiguration ===
    // SPI (Serial Peripheral Interface) wird für die Display-Kommunikation genutzt
    let sclk = peripherals.pins.gpio18; // SPI Clock (SCL auf dem Display)
    let mosi = peripherals.pins.gpio23; // Master Out Slave In (SDA auf dem Display)
    let cs = peripherals.pins.gpio15; // Chip Select (aktiviert das Display)

    // === Steuerungs-Pins ===
    let dc = peripherals.pins.gpio21; // Data/Command Pin (unterscheidet Daten von Befehlen)
    let mut rst = PinDriver::output(peripherals.pins.gpio22)?; // Reset Pin

    info!("Pins configured");

    // === Hardware-Reset des Displays ===
    // Reset-Sequenz: LOW -> Warten -> HIGH -> Warten
    rst.set_low()?; // Reset aktivieren (Display aus)
    FreeRtos::delay_ms(50); // 50ms warten
    rst.set_high()?; // Reset deaktivieren (Display startet)
    FreeRtos::delay_ms(200); // 200ms warten bis Display bereit ist

    // === SPI-Bus Konfiguration ===
    let spi_config = Config::new().baudrate(26.MHz().into()); // 26 MHz Taktfrequenz für schnelle Datenübertragung

    // Erstellt den SPI-Treiber mit den konfigurierten Pins
    // None::<AnyIOPin> bedeutet: kein MISO (Master In Slave Out), da das Display keine Daten zurücksendet
    let spi_driver = SpiDriver::new(
        peripherals.spi2, // Nutzt SPI2 Hardware-Einheit
        sclk,             // Clock Pin
        mosi,             // Daten-Ausgang
        None::<AnyIOPin>, // Kein Daten-Eingang (Display sendet nicht zurück)
        &SpiDriverConfig::new(),
    )?;

    // Erstellt ein SPI-Device mit Chip-Select Pin
    let spi_device = SpiDeviceDriver::new(spi_driver, Some(cs), &spi_config)?;

    // === Wrapper-Instanzen erstellen ===
    // Diese Wrapper passen ESP-IDF's API an embedded-hal 1.0 an
    let spi_wrapper = SpiWrapper { spi: spi_device };
    let dc_wrapper = DcPinWrapper {
        pin: PinDriver::output(dc.downgrade_output())?, // DC-Pin als Ausgabe konfigurieren
    };

    // === Display-Buffer ===
    // Statischer Buffer für Display-Operationen
    // Größe: 240 Pixel breit * 10 Zeilen * 2 Bytes/Pixel (RGB565) = 4800 Bytes
    // Wird als static mut definiert, damit er nicht auf dem Stack liegt (Stack-Overflow Vermeidung)
    static mut DISPLAY_BUFFER: [u8; 240 * 10 * 2] = [0u8; 240 * 10 * 2];

    // === Display-Interface erstellen ===
    // unsafe Block nötig, da wir auf static mut zugreifen
    // addr_of_mut! erzeugt einen Raw Pointer, der dann zu einer Referenz dereferenziert wird
    let di = unsafe {
        mipidsi::interface::SpiInterface::new(
            spi_wrapper,                        // SPI-Kommunikation
            dc_wrapper,                         // Data/Command Pin
            &mut *addr_of_mut!(DISPLAY_BUFFER), // Buffer für Batch-Operationen
        )
    };

    // === Display initialisieren ===
    let mut display = Builder::new(ST7789, di) // ST7789 Controller
        .display_size(240, 320) // Display-Auflösung: 240x320 Pixel
        .display_offset(0, 0) // Kein Offset (beginnt bei 0,0)
        .color_order(ColorOrder::Rgb) // RGB Farbreihenfolge
        .invert_colors(ColorInversion::Inverted) // Farben invertiert (häufig bei TFT nötig)
        .init(&mut FreeRtos) // Initialisierung mit FreeRTOS Delay
        .map_err(|e| anyhow::anyhow!("Display init failed: {:?}", e))?;

    info!("Display initialized!");

    // === Initiale Display-Anzeige ===
    // Füllt das Display mit Schwarz
    display.clear(Rgb565::BLACK).ok();

    // Zeichnet einen grünen Balken als "WiFi verbunden" Indikator
    Rectangle::new(Point::new(0, 0), Size::new(240, 60))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::GREEN))
        .draw(&mut display)
        .ok();

    info!("=== System Ready! ===");

    // ==================== HAUPTSCHLEIFE ====================
    // Array mit Farben, die durchgewechselt werden
    let colors = [
        ("RED", Rgb565::RED),       // Rot
        ("GREEN", Rgb565::GREEN),   // Grün
        ("BLUE", Rgb565::BLUE),     // Blau
        ("YELLOW", Rgb565::YELLOW), // Gelb
    ];

    let mut idx = 0; // Index für Farb-Array

    loop {
        // === WiFi-Verbindung überwachen ===
        // Falls WiFi getrennt wurde, neu verbinden
        if !wifi.is_connected()? {
            warn!("WiFi disconnected, reconnecting...");
            wifi.connect()?;
        }

        // Aktuelle Farbe aus dem Array holen
        let (name, color) = colors[idx];
        info!("Displaying: {} - WiFi: Connected", name);

        // === Display-Aktualisierung ===
        // Füllt das gesamte Display mit der aktuellen Farbe
        display.clear(color).ok();

        // Warte 2 Sekunden
        FreeRtos::delay_ms(2000);

        // Zum nächsten Farb-Index wechseln (mit Wrap-Around)
        idx = (idx + 1) % colors.len();
    }
}
