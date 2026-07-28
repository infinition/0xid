/// Extract icons from executables on Windows.
/// Returns an egui::ColorImage that can be loaded as a texture.

#[cfg(windows)]
pub fn extract_icon(path: &str, size: u32) -> Option<eframe::egui::ColorImage> {
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;

    unsafe {
        // Resolve to absolute path (ExtractIconExW needs it for .lnk)
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
        let abs_str = abs.to_string_lossy();
        let wide_path: Vec<u16> = abs_str.encode_utf16().chain(std::iter::once(0)).collect();

        // ExtractIconExW works for .exe, .dll, .lnk (resolves target), .ico
        let mut large_icon: isize = 0;
        let count = ExtractIconExW(wide_path.as_ptr(), 0, &mut large_icon, std::ptr::null_mut(), 1);
        if count == 0 || large_icon == 0 {
            return None;
        }

        let result = hicon_to_rgba(large_icon, size);
        DestroyIcon(large_icon);
        result
    }
}

#[cfg(windows)]
unsafe fn hicon_to_rgba(hicon: isize, size: u32) -> Option<eframe::egui::ColorImage> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    let hdc_screen = GetDC(0);
    if hdc_screen == 0 {
        return None;
    }
    let hdc = CreateCompatibleDC(hdc_screen);
    if hdc == 0 {
        ReleaseDC(0, hdc_screen);
        return None;
    }

    // Get icon info to access bitmaps
    let mut icon_info: ICONINFO = std::mem::zeroed();
    if GetIconInfo(hicon, &mut icon_info) == 0 {
        DeleteDC(hdc);
        ReleaseDC(0, hdc_screen);
        return None;
    }

    let s = size as i32;
    let pixel_count = (size * size) as usize;
    let mut pixels = vec![0u8; pixel_count * 4];

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: s,
            biHeight: -s, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB as u32,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    };

    let ok = if icon_info.hbmColor != 0 {
        GetDIBits(
            hdc,
            icon_info.hbmColor,
            0,
            size,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        ) != 0
    } else {
        false
    };

    // Cleanup GDI objects
    if icon_info.hbmColor != 0 {
        DeleteObject(icon_info.hbmColor as _);
    }
    if icon_info.hbmMask != 0 {
        DeleteObject(icon_info.hbmMask as _);
    }
    DeleteDC(hdc);
    ReleaseDC(0, hdc_screen);

    if !ok {
        return None;
    }

    // Convert BGRA → RGBA
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2); // B <-> R
    }

    // Check if the icon has any alpha data at all (old-style icons have all-zero alpha)
    let has_alpha = pixels.chunks_exact(4).any(|c| c[3] != 0);
    if !has_alpha {
        // No alpha channel — make all pixels fully opaque
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[3] = 255;
        }
    }

    Some(eframe::egui::ColorImage::from_rgba_unmultiplied(
        [size as usize, size as usize],
        &pixels,
    ))
}

#[cfg(not(windows))]
pub fn extract_icon(_path: &str, _size: u32) -> Option<eframe::egui::ColorImage> {
    None
}

/// Generate a placeholder icon for files that have no embedded icon.
/// Returns a 64×64 retro-styled icon (dark background + green terminal prompt).
pub fn placeholder_icon(size: u32) -> eframe::egui::ColorImage {
    let s = size as usize;
    let mut pixels = vec![0u8; s * s * 4];

    for y in 0..s {
        for x in 0..s {
            let idx = (y * s + x) * 4;
            let border = 3;
            let inner = size as usize - border;

            // Check if we're on the border
            let on_border = x < border || x >= inner || y < border || y >= inner;

            if on_border {
                // Border color: dark green-gray
                pixels[idx] = 20;     // R
                pixels[idx + 1] = 80; // G
                pixels[idx + 2] = 30; // B
                pixels[idx + 3] = 255;
            } else {
                // Background: dark terminal
                pixels[idx] = 15;
                pixels[idx + 1] = 15;
                pixels[idx + 2] = 20;
                pixels[idx + 3] = 255;
            }
        }
    }

    // Draw a ">_" prompt symbol in green at center
    let cx = s / 2;
    let cy = s / 2;
    let col_green = [0u8, 255, 70, 255];  // phosphor green
    let col_cursor = [0u8, 200, 50, 255];

    // ">" character (simple pixel art, ~5x7 at a small offset)
    let glyph: &[(isize, isize)] = &[
        // ">" shape
        (0, -2), (0, -1), (0, 0), (0, 1), (0, 2),
        (1, -1), (1, 0), (1, 1),
        (2, 0),
    ];
    for (dx, dy) in glyph {
        let px = cx as isize + dx;
        let py = cy as isize + dy;
        if px >= 0 && px < s as isize && py >= 0 && py < s as isize {
            let idx = (py as usize * s + px as usize) * 4;
            pixels[idx..idx + 4].copy_from_slice(&col_green);
        }
    }

    // blinking cursor "_" below the >
    let cursor_glyph: &[(isize, isize)] = &[
        (-1, 3), (0, 3), (1, 3),
    ];
    for (dx, dy) in cursor_glyph {
        let px = cx as isize + dx;
        let py = cy as isize + dy;
        if px >= 0 && px < s as isize && py >= 0 && py < s as isize {
            let idx = (py as usize * s + px as usize) * 4;
            pixels[idx..idx + 4].copy_from_slice(&col_cursor);
        }
    }

    eframe::egui::ColorImage::from_rgba_unmultiplied([s, s], &pixels)
}
