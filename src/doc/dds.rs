//! The 8-bit alpha DDS OMSI keeps its ground painting masks in.

/// An uncompressed DDS with one alpha byte per texel (DDPF_ALPHA, mask 0xff), no mipmaps.
pub fn alpha8(size: usize, texels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(128 + texels.len());
    let w = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(b"DDS ");
    w(&mut out, 124); // header size
    w(&mut out, 0x1 | 0x2 | 0x4 | 0x1000 | 0x8); // caps, height, width, pixelformat, pitch
    w(&mut out, size as u32); // height
    w(&mut out, size as u32); // width
    w(&mut out, size as u32); // pitch: one byte per texel
    w(&mut out, 0); // depth
    w(&mut out, 0); // mip count
    for _ in 0..11 {
        w(&mut out, 0);
    }
    // pixel format
    w(&mut out, 32);
    w(&mut out, 0x2); // DDPF_ALPHA
    w(&mut out, 0); // fourcc
    w(&mut out, 8); // bits
    w(&mut out, 0);
    w(&mut out, 0);
    w(&mut out, 0);
    w(&mut out, 0xff); // alpha mask
    w(&mut out, 0x1000); // caps: texture
    for _ in 0..4 {
        w(&mut out, 0);
    }
    out.extend_from_slice(&texels[..size * size]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_back_through_openomsi() {
        let size = 16;
        let texels: Vec<u8> = (0..size * size).map(|i| (i % 251) as u8).collect();
        let bytes = super::alpha8(size, &texels);
        let img = omsi_texture::decode_bytes(&bytes, std::path::Path::new("m.dds")).unwrap();
        assert_eq!((img.width, img.height), (16, 16));
        assert_eq!(img.rgba[4 * 5 + 3], texels[5]);
    }
}
