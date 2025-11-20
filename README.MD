# Rust ESP32-IDF Workspace

This repository contains a collection of Rust projects for the ESP32, built using the `esp-idf` framework. Each project is a standalone example demonstrating different features of the ESP32 with Rust.

## Projects

*   `check_spiram`: A simple utility to check if SPIRAM (PSRAM) is available and functional.
*   `wifi`: A basic example of connecting the ESP32 to a Wi-Fi network.
*   `wifi_display`: A project that connects to Wi-Fi and likely displays information on an attached screen.
*   `wifi_display_openweather`: Fetches weather data from the OpenWeatherMap API and displays it.
*   `wifi_display_openweather_rtc`: An extension of the weather display project that incorporates a Real-Time Clock (RTC) for timekeeping.

## How to Compile and Run

These projects are set up to be built and flashed using the `esp-idf` toolchain for Rust.

### Prerequisites

1.  **Rust Toolchain for ESP32**: Make sure you have the Rust compiler and the `xtensa-esp32-espidf` target installed. The recommended way to set this up is by using `espup`.
    ```bash
    espup install
    ```
2.  **ESP-IDF**: The build system relies on the Espressif IoT Development Framework (ESP-IDF). `espup` should handle this installation.

### Configuration

Some projects require credentials for services like Wi-Fi or weather APIs.

1.  Copy the example secrets file:
    ```bash
    cp secrets.toml.example secrets.toml
    ```
2.  Edit `secrets.toml` and fill in your details, such as Wi-Fi SSID, password, and your OpenWeatherMap API key.

### Building and Flashing

To compile a specific project, navigate to its directory and use `cargo`.

1.  **Build the project**:
    ```bash
    cd <project-folder>
    cargo build
    ```
    *Replace `<project-folder>` with the directory of the project you want to build (e.g., `wifi`).*

2.  **Flash to device**:
    To flash the compiled program onto your ESP32 and view the output, use `cargo-espflash`.
    ```bash
    cd <project-folder>
    cargo espflash flash --monitor
    ```
    *(This command flashes the device and then opens a serial monitor to display logs.)*

