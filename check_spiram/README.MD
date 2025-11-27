# Check SPIRAM

This project is a simple utility to check the available memory on an ESP32 device, specifically focusing on detecting external SPIRAM (PSRAM).

## Description

The program queries the ESP-IDF heap capabilities to determine the total size of:
1.  Internal DRAM (Data/Stack memory)
2.  External SPIRAM (PSRAM)

It then prints the sizes in a human-readable format (KB for internal RAM, MB for SPIRAM) to the serial monitor. Based on the result, it reports whether external SPIRAM is available on the board. This is useful for quickly identifying if you are using an ESP32-WROVER module (which typically includes SPIRAM) or a WROOM module (which typically does not).

## How to Run

1.  Navigate to this project's directory:
    ```bash
    cd check_spiram
    ```

2.  Build, flash, and monitor the output:
    ```bash
    cargo espflash flash --monitor
    ```

### Expected Output

If SPIRAM is present, you will see a message indicating its size:
```
--- ESP32 Memory Analysis ---
🏭 Internal DRAM (Data/Stack): 295000 Bytes (288.09 KB)
💾 External SPIRAM (PSRAM): 4194276 Bytes (4.00 MB)
---------------------------------
✅ Result: **SPIRAM is PRESENT** and available (Probably a WROVER module).
```

If no SPIRAM is found, the output will be:
```
--- ESP32 Memory Analysis ---
🏭 Internal DRAM (Data/Stack): 295000 Bytes (288.09 KB)
💾 External SPIRAM (PSRAM): 0 Bytes (0.00 MB)
---------------------------------
❌ Result: **No SPIRAM (PSRAM) found** (Probably a WROOM module).
```
