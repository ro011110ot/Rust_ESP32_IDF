use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use log::*;

// It is recommended to use a secrets.toml file to store credentials.
// Create a secrets.toml file in the root of the project with the following content:
//
// [wifi]
// ssid = "Your_SSID"
// password = "Your_Password"

fn main() {
    // Initialize the ESP-IDF services
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Hello, ESP32 with WiFi!");

    // Take the peripherals
    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    // Create a new WiFi driver
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap(),
        sys_loop,
    )
    .unwrap();

    // Configure the WiFi client
    let wifi_config = Configuration::Client(ClientConfiguration {
        ssid: "SSID".try_into().unwrap(),
        password: "PASSWORD".try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });

    // Set the WiFi configuration
    wifi.set_configuration(&wifi_config).unwrap();
    // Start the WiFi driver
    wifi.start().unwrap();
    info!("WiFi started");

    // Connect to the WiFi network
    wifi.connect().unwrap();
    info!("WiFi connected!");

    // Wait for the network interface to be up
    wifi.wait_netif_up().unwrap();

    // Get the IP address
    let ip_info = wifi.wifi().sta_netif().get_ip_info().unwrap();
    info!("IP address: {:?}", ip_info.ip);

    // Main loop
    loop {
        // Sleep for 10 seconds
        std::thread::sleep(std::time::Duration::from_secs(10));
        info!("Running...");
    }
}
