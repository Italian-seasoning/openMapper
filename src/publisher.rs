//! Explicit publisher authoring for signed and optional protected packs.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn publisher_output_authenticates_through_the_actual_mesh_loader_and_uses_fresh_keys() {
        let root = std::env::temp_dir().join(format!("openmapper-publisher-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let original = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        std::fs::write(root.join("source.obj"), original).unwrap();
        let mesh = openmapper_mesh::primitives::Primitive::Cube { size: glam::Vec3::ONE }.build();
        let converted = openmapper_mesh::export::to_x(&mesh, &[]).0;
        std::fs::write(root.join("playable.x"), converted).unwrap();
        let signer = omsi_pack::SigningKey::generate(&ring::rand::SystemRandom::new()).unwrap();
        let signing = root.join("signing.pk8");
        signer.save_private_key(&signing).unwrap();
        let spec = root.join("spec.json");
        std::fs::write(&spec, serde_json::to_vec(&serde_json::json!({
            "protected": true,
            "metadata": {"publisher_id":"owned-fixture","author":"Fixture","pack_id":"mesh","version":"1.0.0","license":"LicenseRef-Owned-Fixture","redistribution":false,"key_id":null,"dependencies":[]},
            "assets":[{"id":"mesh","path":"Sceneryobjects/Fixture/model/cube.x","file":"playable.x","source":"source.obj"}]
        })).unwrap()).unwrap();
        let out = root.join("mesh.oopp");
        let key_path = root.join("content-key.bin");
        let manifest = super::publish(&spec, &out, &signing, Some(&key_path)).unwrap();
        assert_eq!(manifest.assets[0].source_sha256, omsi_pack::sha256_hex(original));
        assert_ne!(manifest.assets[0].source_sha256, manifest.assets[0].output_sha256);
        let key: [u8; 32] = std::fs::read(&key_path).unwrap().try_into().unwrap();
        let mut trust = omsi_pack::TrustedPublishers::default();
        trust.insert("owned-fixture", signer.key_id(), signer.public_key());
        let pack = Arc::new(omsi_pack::VerifiedPack::open_for_authoring(&out, &trust, Some(key)).unwrap());
        let mount = root.join("virtual");
        omsi_cfg::vfs::mount_publisher(&mount, pack, BTreeMap::new()).unwrap();
        let mesh_path = mount.join("Sceneryobjects/Fixture/model/cube.x");
        assert!(omsi_cfg::vfs::is_sealed(&mesh_path));
        let decoded = omsi_o3d::load_mesh(&mesh_path).unwrap();
        assert_eq!(decoded.triangles.len(), 12);
        assert!(!mesh_path.exists(), "protected asset was extracted to disk");
        let second = super::publish(&spec, &root.join("second.oopp"), &signing, Some(&root.join("second-key.bin"))).unwrap();
        assert_ne!(manifest.key_id, second.key_id);
        assert!(super::publish(&spec, &out, &signing, Some(&key_path)).is_err());
        omsi_cfg::vfs::unmount_sealed(&mount);
        std::fs::remove_dir_all(root).unwrap();
    }
}
