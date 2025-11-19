use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_sys::{
    // Funktionen und Konstanten für die Heap-Prüfung
    heap_caps_get_total_size,
    MALLOC_CAP_DMA,
    MALLOC_CAP_INTERNAL,
    MALLOC_CAP_SPIRAM,
};

fn main() -> anyhow::Result<()> {
    // 1. Initialisierung der IDF Services (für Logging und Systemfunktionen)
    EspLogger::initialize_default();
    let _peripherals = Peripherals::new()?;

    // --- 2. Speicherabfrage ---

    // Die Kombination MALLOC_CAP_INTERNAL | MALLOC_CAP_DMA erfasst den Großteil des nutzbaren internen DRAM
    let internal_ram_bytes =
        unsafe { heap_caps_get_total_size(MALLOC_CAP_INTERNAL | MALLOC_CAP_DMA) };

    // MALLOC_CAP_SPIRAM erfasst den gesamten externen PSRAM (SPIRAM)
    let external_psram_bytes = unsafe { heap_caps_get_total_size(MALLOC_CAP_SPIRAM) };

    // --- 3. Ausgabe der Ergebnisse ---

    println!("\n--- ESP32 Speicherauswertung ---");

    // Ausgabe des internen RAM in KB
    println!(
        "🏭 Interner DRAM (Data/Stack): {} Bytes ({:.2} KB)",
        internal_ram_bytes,
        internal_ram_bytes as f32 / 1024.0
    );

    // Ausgabe des externen SPIRAM in MB
    println!(
        "💾 Externer SPIRAM (PSRAM): {} Bytes ({:.2} MB)",
        external_psram_bytes,
        external_psram_bytes as f32 / 1024.0 / 1024.0
    );

    println!("---------------------------------");

    if external_psram_bytes > 0 {
        println!("✅ Ergebnis: **SPIRAM ist VORHANDEN** und verfügbar (Wahrscheinlich ein WROVER-Modul).");
    } else {
        println!("❌ Ergebnis: **Kein SPIRAM (PSRAM) gefunden** (Wahrscheinlich ein WROOM-Modul).");
    }

    Ok(())
}
