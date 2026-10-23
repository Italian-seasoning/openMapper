//! The program's icon: the logo drawn by `tools/make-icons.py` (assets/icons/app), built
//! into the program, at any size (the window's, and the packages' through
//! `openmapper --icon out.png <size>`).

const PNG: &[u8] = include_bytes!("../assets/icons/app/openmapper-512.png");

/// The icon as RGBA pixels, `size` × `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let img = image::load_from_memory_with_format(PNG, image::ImageFormat::Png).expect("the built-in icon is a PNG").to_rgba8();
    if img.width() == size {
        return img.into_raw();
    }
    image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3).into_raw()
}

/// `openmapper --icon out.png [size]`: the icon written as a PNG (for the packages).
pub fn cli(out: &std::path::Path, size: u32) -> i32 {
    match image::RgbaImage::from_raw(size, size, rgba(size)).map(|img| img.save(out)) {
        Some(Ok(())) => 0,
        Some(Err(e)) => {
            eprintln!("openmapper --icon: {e}");
            1
        }
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn corners_are_clear_and_the_middle_is_not() {
        let px = super::rgba(64);
        let alpha = |x: usize, y: usize| px[(y * 64 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(32, 32), 255);
    }
}
