# Wi-Fi + Display Example (ST7789)

This project demonstrates how to connect an ESP32 to a Wi-Fi network and display information on an ST7789 TFT screen.

## Description

This application performs the following tasks:

1. **Loads Credentials**: It securely loads Wi-Fi SSID and password from the `secrets.toml` file located in the
   workspace root.
2. **Connects to Wi-Fi**: It establishes a connection to the configured Wi-Fi network and prints the assigned IP address
   to the serial monitor. It also includes basic logic to reconnect if the connection is lost.
3. **Initializes Display**: It sets up and initializes a 240x320 ST7789 TFT display connected via SPI.
4. **Displays Status**: Upon a successful Wi-Fi connection, it draws a green bar at the top of the screen as a visual
   indicator.
5. **Main Loop**: The application enters a loop where it continuously cycles the screen's background color through Red,
   Green, Blue, and Yellow every two seconds, while also monitoring the Wi-Fi connection.

This project serves as a foundational example for building more complex IoT applications that require both network
connectivity and a graphical user interface. It showcases how to bridge the `esp-idf-hal` with libraries that expect
`embedded-hal` 1.0 traits by using custom wrappers.

## Hardware Requirements

- An ESP32 development board.
- An ST7789 TFT display (240x320 resolution).
- Correct wiring between the ESP32 and the display. The pin configuration is defined in `src/main.rs`:
    - **SCLK**: GPIO 18
    - **MOSI (SDA)**: GPIO 23
    - **CS**: GPIO 15
    - **DC**: GPIO 21
    - **RST**: GPIO 22

## Configuration

This project uses the shared `secrets.toml` file in the root of the workspace. Make sure you have copied
`secrets.toml.example` to `secrets.toml` and filled in your Wi-Fi `ssid` and `password`.

## How to Run

1. Ensure your hardware is wired correctly.
2. Ensure your `secrets.toml` is configured.
3. Navigate to this project's directory:
   ```bash
   cd wifi_display
   ```
4. Build, flash, and monitor the output:
   ```bash
   cargo espflash flash --monitor
   ```

### Expected Outcome

- The serial monitor will show logs indicating a successful Wi-Fi connection and the assigned IP address.
- The ST7789 display will first show a green bar at the top, and then the entire screen will start cycling through solid
  colors every two seconds.
