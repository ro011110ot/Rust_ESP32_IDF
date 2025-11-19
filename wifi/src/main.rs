use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use log::*;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Hello, ESP32 with WiFi!");

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap(),
        sys_loop,
    )
    .unwrap();

    let wifi_config = Configuration::Client(ClientConfiguration {
        ssid: "Vodafone Hotspot".try_into().unwrap(),
        password: "".try_into().unwrap(),
        auth_method: AuthMethod::None, // Für offenes WLAN
        ..Default::default()
    });

    wifi.set_configuration(&wifi_config).unwrap();
    wifi.start().unwrap();
    info!("WiFi gestartet");

    wifi.connect().unwrap();
    info!("WiFi verbunden!");

    wifi.wait_netif_up().unwrap();

    let ip_info = wifi.wifi().sta_netif().get_ip_info().unwrap();
    info!("IP-Adresse: {:?}", ip_info.ip);

    loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        info!("Läuft...");
    }
}
