//! Trusted publisher provenance and ordinary export policy.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicBool;
    use omsi_pack::{AssetInput, PackMetadata, SigningKey, TrustedPublishers};

    #[test]
    fn renamed_restricted_asset_exports_a_dependency_and_keeps_the_ordinary_rights_override() {
        let root = std::env::temp_dir().join(format!("om-publisher-policy-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Packs")).unwrap();
        let signer = SigningKey::generate(&ring::rand::SystemRandom::new()).unwrap();
        let mut trust = TrustedPublishers::default();
        trust.insert("owned-fixture", signer.key_id(), signer.public_key());
        trust.save_file(&root.join("Publishers.json")).unwrap();
        let data = b"[friendlyname]\nOwned sign\n";
        let pack = root.join("Packs/signs.oopp");
        omsi_pack::create_pack(&pack, PackMetadata {
            publisher_id: "owned-fixture".into(), author: "Fixture".into(), pack_id: "signs".into(),
            version: "1.0.0".into(), license: "LicenseRef-Owned-Fixture".into(),
            redistribution: false, key_id: None, dependencies: vec![],
        }, &[AssetInput { id: "sign".into(), path: "Sceneryobjects/Fixture/sign.sco".into(), source_sha256: omsi_pack::sha256_hex(b"original fixture source"), bytes: data.to_vec() }], &signer, None, &AtomicBool::new(false)).unwrap();
        let renamed = root.join("renamed.sco");
        std::fs::write(&renamed, data).unwrap();
        let alias = "Sceneryobjects/MyMap/renamed.sco".to_string();
        let mut files = BTreeMap::from([(alias.clone(), renamed.clone())]);
        let policy = super::filter(&mut files, &root, false).unwrap();
        assert!(files.is_empty(), "restricted bytes were copied");
        assert_eq!(policy.references.dependencies[0].pack_id, "signs");
        assert_eq!(policy.references.assets[0].path, alias);
        assert_eq!(policy.references.assets[0].asset_id, "sign");
        assert!(!policy.warnings.is_empty());

        // Retaining the trusted index still recognizes a renamed copy without its manifest.
        std::fs::remove_file(pack.join("manifest.json")).unwrap();
        let mut referenced = BTreeMap::from([(alias.clone(), renamed.clone())]);
        assert!(!super::filter(&mut referenced, &root, false).unwrap().references.assets.is_empty());
        let mut overridden = BTreeMap::from([(alias.clone(), renamed.clone())]);
        let declared = super::filter(&mut overridden, &root, true).unwrap();
        assert_eq!(overridden.len(), 1);
        assert!(declared.references.dependencies.is_empty());

        // A changed output invalidates the exact hash match immediately.
        std::fs::write(&renamed, b"different owned source bytes").unwrap();
        let mut changed = BTreeMap::from([(alias, renamed)]);
        assert!(super::filter(&mut changed, &root, false).unwrap().references.assets.is_empty());
        assert_eq!(changed.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_ordinary_rights_declaration_cannot_export_a_protected_virtual_asset() {
        let root = std::env::temp_dir().join(format!("om-protected-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let key = std::sync::Arc::new(omsi_cfg::vfs::SealKey::new(&omsi_cfg::vfs::random_key().unwrap()));
        let blob = root.join("blob");
        std::fs::write(&blob, omsi_cfg::vfs::seal(&key, b"fixture", b"owned bytes").unwrap()).unwrap();
        let mount = root.join("virtual");
        omsi_cfg::vfs::mount_sealed(&mount, key, vec![omsi_cfg::vfs::SealedFile { rel: "Sceneryobjects/Fixture/a.sco".into(), blob, aad: b"fixture".to_vec(), size: 11 }]);
        let mut files = BTreeMap::from([("Sceneryobjects/Fixture/a.sco".into(), mount.join("Sceneryobjects/Fixture/a.sco"))]);
        assert!(super::filter(&mut files, &root, true).is_err());
        omsi_cfg::vfs::unmount_sealed(&mount);
        std::fs::remove_dir_all(root).unwrap();
    }
}
