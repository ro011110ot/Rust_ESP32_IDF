from PIL import Image, ImageDraw
import math

icon_size = 40

def save_icon(icon_name, draw_function):
    img = Image.new("1", (icon_size, icon_size), 1)
    draw = ImageDraw.Draw(img)
    draw_function(draw)
    img.save(f"{icon_name}.png")
    img.show()

def sun(draw):
    draw.ellipse([8, 8, 32, 32], outline=0, width=2)
    for angle in range(0, 360, 45):
        x1 = 20 + 12 * round(math.cos(math.radians(angle)))
        y1 = 20 + 12 * round(math.sin(math.radians(angle)))
        x2 = 20 + 16 * round(math.cos(math.radians(angle)))
        y2 = 20 + 16 * round(math.sin(math.radians(angle)))
        draw.line([x1, y1, x2, y2], fill=0)

def moon(draw):
    draw.ellipse([8, 8, 32, 32], outline=0, width=2)
    draw.ellipse([14, 8, 32, 32], fill=1)  # Sichel-Mond-Effekt

def partly_sunny(draw):
    sun(draw)
    draw.ellipse([4, 12, 28, 28], fill=1)  # Teilweise Wolke

def cloud(draw):
    draw.ellipse([8, 12, 32, 28], fill=0)
    draw.ellipse([4, 8, 16, 20], fill=0)
    draw.ellipse([24, 8, 36, 20], fill=0)

def rain(draw):
    cloud(draw)
    for x in range(12, 32, 8):
        draw.line([x, 28, x, 36], fill=0)

def thunder(draw):
    cloud(draw)
    points = [
        (16, 22),
        (18, 26),
        (14, 26),
        (20, 34),
        (16, 34),
        (22, 42)
    ]
    draw.line(points, fill=0, width=2)

def snow(draw):
    draw.line([20, 8, 20, 32], fill=0)
    draw.line([8, 20, 32, 20], fill=0)
    draw.line([8, 8, 32, 32], fill=0)
    draw.line([8, 32, 32, 8], fill=0)

def fog(draw):
    wave_amplitude = 3
    wave_length = 8
    for i in range(3):
        y_base = 10 + i * 10
        points = []
        for x in range(0, icon_size + 1):
            y = y_base + wave_amplitude * math.sin(2 * math.pi * x / wave_length)
            points.append((x, y))
        draw.line(points, fill=0, width=1)

# Speichern der Icons
save_icon('sun', sun)
save_icon('moon', moon)
save_icon('partly_sunny', partly_sunny)
save_icon('cloud', cloud)
save_icon('rain', rain)
save_icon('thunder', thunder)
save_icon('snow', snow)
save_icon('fog', fog)
