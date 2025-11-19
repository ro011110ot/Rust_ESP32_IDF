# Weather Station Display (OpenWeatherMap)

This project extends the Wi-Fi and display example to create a functional weather station that fetches and displays real-time data from the OpenWeatherMap API.

## Description

This application builds upon the previous `wifi_display` project with the following key features:

1.  **Weather API Integration**: It connects to the OpenWeatherMap API to fetch current weather data for a specified city. This includes temperature, humidity, wind speed, and a general weather description.
2.  **JSON Parsing**: It uses `serde` and `serde_json` to parse the JSON response from the weather API into structured Rust data types.
3.  **Dynamic Display**: The ST7789 display is used to show the weather information in a clean, readable format.
4.  **Graphical Icons**: Instead of just text, the application displays custom graphical icons corresponding to the current weather conditions (e.g., sun, clouds, rain). These icons are stored as bitmap data within the firmware. The color of the icon also changes based on the weather (e.g., yellow for sun, gray for clouds).
5.  **Periodic Updates**: The weather data is automatically refreshed every 15 minutes.
6.  **Error Handling**: If fetching the weather data fails (due to network issues or an invalid API key), the screen will turn red to signal an error.
7.  **Secure Configuration**: All necessary credentials (Wi-Fi SSID/password, OpenWeatherMap API key, and city) are loaded from the central `secrets.toml` file.

## Hardware Requirements

-   An ESP32 development board.
-   An ST7789 TFT display (240x320 resolution).
-   The wiring is the same as the `wifi_display` project. See that project's `README.md` for details.

## Configuration

This project requires additional secrets for the OpenWeatherMap API.

1.  Make sure you have an account at [OpenWeatherMap](https://openweathermap.org/) and have generated an API key.
2.  Open the `secrets.toml` file in the root of the workspace.
3.  Ensure the `[openweather]` section is present and filled in:
    ```toml
    [openweather]
    api_key = "YOUR_API_KEY_HERE"
    city = "YourCity,YourCountryCode" # e.g., "London,GB"
    ```
4.  Also, ensure your `[wifi]` credentials are correct.

## How to Run

1.  Ensure your hardware is wired correctly.
2.  Ensure your `secrets.toml` is fully configured with Wi-Fi and OpenWeatherMap details.
3.  Navigate to this project's directory:
    ```bash
    cd wifi_display_openweather
    ```
4.  Build, flash, and monitor the output:
    ```bash
    cargo espflash flash --monitor
    ```

### Expected Outcome

-   The device will connect to Wi-Fi.
-   It will then make a request to the OpenWeatherMap API.
-   The display will update to show the city name, temperature, weather description, and a graphical icon representing the current weather.
-   This information will refresh every 15 minutes.
