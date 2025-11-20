# Weather Station & Clock (OpenWeatherMap + SNTP)

This is the most advanced project in this workspace, combining the weather station with a real-time clock synchronized over the internet.

## Description

This application integrates all the features from the previous examples and adds timekeeping:

1.  **Wi-Fi & Display**: Connects to Wi-Fi and drives an ST7789 display.
2.  **Weather API**: Fetches real-time weather data from OpenWeatherMap for a configured city.
3.  **Time Synchronization**: On startup, it connects to an NTP (Network Time Protocol) server using the `esp-idf-svc` SNTP service to synchronize the ESP32's internal clock.
4.  **Timezone Conversion**: It correctly converts the synchronized UTC time to local time for Berlin, Germany (CET/CEST), properly handling daylight saving time changes.
5.  **Combined UI**: The display is updated every second to show:
    *   Current Date and Timezone (e.g., `25.10.2024 CEST`)
    *   Current Time (e.g., `14:35:10`)
    *   Weather information (city, temperature, description, etc.)
    *   A graphical weather icon.
6.  **Efficient Updates**: The main loop runs every second to update the clock display. The more resource-intensive weather fetch is only performed every 15 minutes, providing a responsive clock without constantly hitting the API.

## Hardware Requirements

-   An ESP32 development board.
-   An ST7789 TFT display (240x320 resolution).
-   The wiring is the same as the `wifi_display` project.

## Configuration

This project uses the shared `secrets.toml` file in the root of the workspace. Ensure it is fully configured with both your `[wifi]` and `[openweather]` details.

See the `README.MD` in the `wifi_display_openweather` project for detailed instructions on configuring the secrets file.

## How to Run

1.  Ensure your hardware is wired correctly.
2.  Ensure your `secrets.toml` is fully configured.
3.  Navigate to this project's directory:
    ```bash
    cd wifi_display_openweather_rtc
    ```
4.  Build, flash, and monitor the output:
    ```bash
    cargo espflash flash --monitor
    ```

### Expected Outcome

-   The device will connect to Wi-Fi and synchronize its time.
-   The display will light up and show the current date and time, which updates every second.
-   Shortly after, the weather information will appear.
-   The clock will remain accurate, and the weather will refresh every 15 minutes.
