from PIL import Image

ICON_MAP = {
    "01d": "sun",
    "01n": "moon",
    "02d": "partly_sunny",
    "02n": "cloud",
    "03d": "cloud",
    "03n": "cloud",
    "04d": "cloud",
    "04n": "cloud",
    "09d": "rain",
    "09n": "rain",
    "10d": "rain",
    "10n": "rain",
    "11d": "thunder",
    "11n": "thunder",
    "13d": "snow",
    "13n": "snow",
    "50d": "fog",
    "50n": "fog",
}

ICON_SIZE = 40  # px
BYTES_PER_ICON = ICON_SIZE * ICON_SIZE // 8  # 200 bytes

def png_to_bytes(filename):
    img = Image.open(filename).convert("1")  # 1-bit monochrome
    pixels = img.load()
    data = []
    for y in range(ICON_SIZE):
        for x_byte in range(ICON_SIZE // 8):
            byte = 0
            for bit in range(8):
                x = x_byte * 8 + bit
                pixel = pixels[x, y]
                if pixel == 0:  # schwarz -> 1
                    byte |= 1 << (7 - bit)
            data.append(byte)
    return data

with open("weather_icons.rs", "w") as f:
    f.write("// Auto-generated Rust file\n\n")

    # Arrays für alle Icons
    for icon_name in set(ICON_MAP.values()):
        data = png_to_bytes(f"{icon_name}.png")
        f.write(f"pub const {icon_name.upper()}: [u8; {BYTES_PER_ICON}] = [\n")
        for i in range(0, len(data), 16):
            line = ", ".join(f"0x{b:02X}" for b in data[i:i+16])
            f.write(f"    {line},\n")
        f.write("];\n\n")

    # Funktion zur Auswahl per Code
    f.write("pub fn get_weather_icon(code: &str) -> Option<&'static [u8; 200]> {\n")
    f.write("    match code {\n")
    for code, icon in ICON_MAP.items():
        f.write(f"        \"{code}\" => Some(&{icon.upper()}),\n")
    f.write("        _ => None,\n")
    f.write("    }\n")
    f.write("}\n")
