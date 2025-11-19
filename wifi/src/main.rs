use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use log::*;

fn main() {
    // Link patches required for ESP-IDF functionality
    esp_idf_svc::sys::link_patches();
    // Initialize the default logger (prints to serial output)
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Hello, ESP32 with WiFi!");

    // Take ownership of device peripherals (GPIO, Modem, etc.)
    let peripherals = Peripherals::take().unwrap();

    // Initialize the system event loop (handles Wi-Fi events, IP events, etc.)
    let sys_loop = EspSystemEventLoop::take().unwrap();

    // Initialize the Non-Volatile Storage (NVS) partition
    // Wi-Fi driver needs this to store calibration data and physical settings
    let nvs = EspDefaultNvsPartition::take().unwrap();

    // Create and wrap the Wi-Fi driver
    // We wrap EspWifi in BlockingWifi to use synchronous (blocking) APIs
    // instead of async, which is simpler for this use case.
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(
            peripherals.modem, // The modem peripheral is required for Wi-Fi
            sys_loop.clone(),  // Clone the event loop handle
            Some(nvs),         // Pass the NVS partition
        )
        .unwrap(),
        sys_loop,
    )
    .unwrap();

    // Define WiFi configuration
    let wifi_config = Configuration::Client(ClientConfiguration {
        ssid: "Vodafone Hotspot".try_into().unwrap(),
        password: "".try_into().unwrap(),
        auth_method: AuthMethod::None, // For open Wi-Fi networks (no password)
        ..Default::default()
    });

    // Apply configuration and start the Wi-Fi driver
    wifi.set_configuration(&wifi_config).unwrap();
    wifi.start().unwrap();
    info!("WiFi started");

    // Connect to the configured network
    // This blocks until the connection is established or fails
    wifi.connect().unwrap();
    info!("WiFi connected!");

    // Wait for the network interface to come up (wait for DHCP to assign an IP)
    wifi.wait_netif_up().unwrap();

    // Retrieve and print the assigned IP address
    let ip_info = wifi.wifi().sta_netif().get_ip_info().unwrap();
    info!("IP Address: {:?}", ip_info.ip);

    // Main application loop
    loop {
        // Sleep for 10 seconds to reduce CPU usage
        std::thread::sleep(std::time::Duration::from_secs(10));
        info!("Running...");
    }
}
