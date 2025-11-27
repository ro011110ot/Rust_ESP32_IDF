# WiFi Connection Example

This project demonstrates how to connect an ESP32 to a Wi-Fi network using the `esp-idf-svc` library.

## Description

The application initializes the ESP32's Wi-Fi driver in station mode. It then configures the connection with a specific SSID and authentication method, starts the Wi-Fi service, and attempts to connect to the network.

Once connected, it waits for the network interface to be up, retrieves the assigned IP address, and prints it to the serial monitor. The device then enters an infinite loop, printing a "still running" message every 10 seconds while maintaining the connection.

## Configuration

This example uses a `secrets.toml` file to store Wi-Fi credentials.

1.  Copy the example secrets file:
    ```bash
    cp ../secrets.toml.example ../secrets.toml
    ```
2.  Edit `../secrets.toml` and fill in your details, such as Wi-Fi SSID and password.

## How to Run

1.  Modify the Wi-Fi credentials in `../secrets.toml` as described above.

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
I (XXXX) wifi: WiFi started
I (XXXX) wifi: WiFi connected!
I (XXXX) wifi: IP address: 192.168.1.123
I (XXXX) wifi: Running...
...
```
The IP address will correspond to the one assigned by your router.
