//! Cornice della finestra su Windows 11: la barra del titolo prende il
//! grigio-azzurro della fascia in alto di Postamon, così da finestra
//! ingrandita barra e app sono un'unica superficie. I pulsanti, l'aggancio ai
//! bordi e il doppio clic restano quelli di Windows.

/// `--paper` e `--ink` di `src/styles.css`.
const CAPTION: u32 = colorref(0xe8, 0xed, 0xf1);
const TEXT: u32 = colorref(0x17, 0x20, 0x2a);

/// Colore nel formato di Windows (0x00BBGGRR).
const fn colorref(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

#[cfg(windows)]
pub fn paint(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute};

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    for (attribute, color) in [(DWMWA_CAPTION_COLOR, CAPTION), (DWMWA_TEXT_COLOR, TEXT)] {
        // Windows 10 non conosce questi attributi: la chiamata fallisce e la
        // barra resta quella di sistema.
        unsafe {
            DwmSetWindowAttribute(
                hwnd.0 as _,
                attribute as u32,
                (&color as *const u32).cast(),
                size_of::<u32>() as u32,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn colorref_is_bgr() {
        assert_eq!(super::colorref(0x11, 0x22, 0x33), 0x0033_2211);
    }
}
