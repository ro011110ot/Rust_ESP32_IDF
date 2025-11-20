# WiFi Connection Example

This project demonstrates how to connect an ESP32 to a Wi-Fi network using the `esp-idf-svc` library.

## Description

The application initializes the ESP32's Wi-Fi driver in station mode. It then configures the connection with a specific SSID and authentication method, starts the Wi-Fi service, and attempts to connect to the network.

Once connected, it waits for the network interface to be up, retrieves the assigned IP address, and prints it to the serial monitor. The device then enters an infinite loop, printing a "still running" message every 10 seconds while maintaining the connection.

## Configuration

This example uses hardcoded Wi-Fi credentials directly in the `src/main.rs` file. To connect to your own network, you must modify the following lines:

```rust
// src/main.rs

// ...
    let wifi_config = Configuration::Client(ClientConfiguration {
        ssid: "Your-SSID".try_into().unwrap(),
        password: "Your-Password".try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal, // Or another method like AuthMethod::None for open networks
        ..Default::default()
    });
// ...
```

-   Replace `"Your-SSID"` with your network's name.
-   Replace `"Your-Password"` with your network's password.
-   Change `auth_method` if your network uses a different security protocol (e.g., `AuthMethod::None` for an open network or `AuthMethod::WPAWPA2Personal` for mixed mode).

## How to Run

1.  Modify the Wi-Fi credentials in `src/main.rs` as described above.

2.  Navigate to this project's directory:
    ```bash
    cd wifi
    ```

3.  Build, flash, and monitor the output:
    ```bash
    cargo espflash flash --monitor
    ```

### Expected Output

After the device boots and connects, you will see log messages similar to this:
```
...
I (XXXX) wifi: WiFi gestartet
I (XXXX) wifi: WiFi verbunden!
I (XXXX) wifi: IP-Adresse: 192.168.1.123
I (XXXX) wifi: Läuft...
...
```
The IP address will correspond to the one assigned by your router.
