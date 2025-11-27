//! This file is a simple application that checks for the available SPIRAM on the ESP32.
//! It prints the total size of the internal DRAM and the external SPIRAM.
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_sys::{
    heap_caps_get_total_size, EspError, MALLOC_CAP_DMA, MALLOC_CAP_INTERNAL, MALLOC_CAP_SPIRAM,
};

/// Entry point of the application.
/// Returns Result<(), EspError> to allow using the '?' operator for easy error handling.
fn main() -> Result<(), EspError> {
    // 1. Initialize IDF services
    // This sets up the system logging so we can see output on the serial monitor.
    EspLogger::initialize_default();

    // 2. Initialize Peripherals
    // We use .take() instead of .new(). This ensures we get safe, exclusive access
    // to the hardware. It returns a Result, so we handle errors with '?'.
    let _peripherals = Peripherals::take()?;

    // --- 3. Memory Query ---

    // We use 'unsafe' here because we are calling C functions from the ESP-IDF framework directly.
    // Rust cannot verify the safety of these external C functions at compile time.

    // Query Internal DRAM:
    // MALLOC_CAP_INTERNAL | MALLOC_CAP_DMA represents the main internal memory
    // capable of DMA operations (where stack and data usually live).
    let internal_ram_bytes =
        unsafe { heap_caps_get_total_size(MALLOC_CAP_INTERNAL | MALLOC_CAP_DMA) };

    // Query External SPIRAM (PSRAM):
    // MALLOC_CAP_SPIRAM refers specifically to the external SPI-connected RAM.
    let external_psram_bytes = unsafe { heap_caps_get_total_size(MALLOC_CAP_SPIRAM) };

    // --- 4. Output Results ---

    println!("\n--- ESP32 Memory Analysis ---");

    // Print Internal Memory size
    println!(
        "🏭 Internal DRAM (Data/Stack): {} Bytes ({:.2} KB)",
        internal_ram_bytes,
        internal_ram_bytes as f32 / 1024.0
    );

    // Print External Memory size
    println!(
        "💾 External SPIRAM (PSRAM):    {} Bytes ({:.2} MB)",
        external_psram_bytes,
        external_psram_bytes as f32 / 1024.0 / 1024.0
    );

    println!("---------------------------------");

    // Logic check to see if SPIRAM is actually active
    if external_psram_bytes > 0 {
        println!("✅ Result: **SPIRAM is PRESENT** and available.");
    } else {
        println!("❌ Result: **No SPIRAM (PSRAM) found**.");
        println!("   (If you expected SPIRAM, check your sdkconfig or board type)");
    }

    Ok(())
}
