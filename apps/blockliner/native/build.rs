//! Embeds the app icon in the Windows executable, for Explorer, the taskbar
//! and the Start-menu shortcut (the window icon is set at runtime). Nothing
//! happens on other targets, or when cross-compiling to Windows from a
//! non-Windows host, which `winresource` can't do.

fn main() {
    println!("cargo:rerun-if-changed=../assets/icon.png");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows::embed_icon("../assets/icon.png");
    }
}

#[cfg(windows)]
mod windows {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    use image::imageops::FilterType;
    use image::ExtendedColorType;

    /// Sizes in the .ico (256 is the largest an .ico entry can be).
    const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

    pub fn embed_icon(png: &str) {
        let icon = image::open(png).expect("app icon");
        let frames: Vec<IcoFrame> = SIZES
            .iter()
            .map(|&size| {
                let rgba = icon
                    .resize_exact(size, size, FilterType::Lanczos3)
                    .into_rgba8();
                IcoFrame::as_png(rgba.as_raw(), size, size, ExtendedColorType::Rgba8)
                    .expect("ico frame")
            })
            .collect();
        let ico = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("app.ico");
        IcoEncoder::new(std::fs::File::create(&ico).expect("app.ico"))
            .encode_images(&frames)
            .expect("app.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().expect("utf-8 OUT_DIR"));
        res.compile().expect("embedding the icon");
    }
}
